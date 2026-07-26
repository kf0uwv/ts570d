# 6. Windows entry point and concurrency model for `ui::run`'s two-task design

Date: 2026-07-26

## Status

Accepted

## Context

`radio-cat-rs`'s [ADR 0004](https://github.com/kf0uwv/radio-cat-rs/blob/main/docs/adr/0004-windows-serial-backend.md)
and [ADR 0006](https://github.com/kf0uwv/radio-cat-rs/blob/main/docs/adr/0006-windows-network-transport.md)
gave `cat-transport-serial`, `cat-transport-tcp`, `cat-transport-udp`, and
`cat-server` working Windows backends, and explicitly named this repo's own
follow-on requirement without dispatching or authorizing it: `ts570d` still
needs its own Windows entry point, because `#[monoio::main]` cannot exist on
Windows at all (`monoio` requires io_uring, a Linux kernel interface).
`radio-cat-rs`'s ADR 0004 §1 sketched the shape this should take: "a future
Windows port would need to replace `monoio::spawn`'s cooperative task with a
genuine `std::thread::spawn`-based worker feeding results back over a channel
to the UI thread's `block_on` loop, to preserve the 'key events stay
responsive during a slow poll' property." This ADR is that follow-on design
— and it deviates from that sketch for a concrete, load-bearing reason found
during implementation (see Decision §1).

### What was read before deciding

- `ui/src/terminal.rs` (read in full, 2497 lines). `pub async fn run<R: Radio
  + 'static>(radio: R) -> UiResult<()>` creates two `Rc<RefCell<VecDeque<T>>>`
  channels (`cmd_ch`, `update_ch`), `monoio::spawn`s a `radio_task(radio,
  cmd_rx, update_tx)` future, then `.await`s `ui_task(terminal, cmd_ch,
  update_ch)` directly in the caller's own task. Once `ui_task` resolves
  (always on the `KeyResult::Quit` path, immediately after sending
  `RadioCmd::Quit`), `run` drops the radio task's `monoio::spawn` handle —
  canceling it without waiting for it, since awaiting it could block up to
  ~40s if it's mid-poll. `radio_task` loops forever: poll ~20 getters
  (`poll_radio_state`, each a real awaited CAT round trip), update connection
  health, send a `RadioDisplay` snapshot, drain+process queued `RadioCmd`s
  (including a 107-step `run_diagnostics_task` that can run for several
  rounds, aborted early via a **synchronous** `crossterm::event::poll` check
  for `[Esc]` between steps — unrelated to `ui_task`'s own key handling),
  then `monoio::time::sleep(200ms)`. `ui_task` loops forever: drain queued
  `RadioUpdate`s, draw one frame, a **synchronous, blocking**
  `crossterm::event::poll(Duration::from_millis(10))` call (already
  blocking-the-thread on Linux today — not a new concern this ADR
  introduces), then `monoio::time::sleep(5ms)`. Both tasks run cooperatively
  on **one** OS thread under `monoio`'s thread-per-core model — the entire
  reason `Rc`/`RefCell` (not `Arc`/`Mutex`) is safe for the channels between
  them.
- `radio/src/ts570d.rs` (read in full). `Ts570d<S: CatSession>` wraps
  `CatClient<Ts570dCommandId, SharedSession<S>>`, and
  `SharedSession<S>(Rc<RefCell<Option<S>>>)` is **deliberately** `Rc`-based
  (its own doc comment: shares one session between `CatClient`'s private
  field and `Ts570d`'s own direct access, needed for `Ts570d::flush_rx` and
  wire-byte-level test assertions). This means **`Ts570d<S>` is `!Send`,
  unconditionally, regardless of which `S` it wraps** (serial, TCP, or any
  future transport) — `Rc<RefCell<_>>` is never `Send` no matter what it
  contains. This one fact drives Decision §1 below.
- `radio/src/radio_trait.rs`: `Radio` is `#[async_trait(?Send)]`, no `Send`
  bound anywhere, and (confirmed directly, not assumed) has **no**
  `ModemControlLines`-equivalent supertrait or bound — `ts570d` has no
  CW-keying feature (per this repo's own CLAUDE.md, "Radio trait scope"),
  unlike `ft991a`'s `CwKeying: ModemControlLines` bound. This matters for
  Task 4 (TCP client mode, `--server <host:port>`) as much as this ADR:
  `ts570d`'s TCP client mode needs no `cat_transport_core::NoModemControlLines`
  wrapper at all, since nothing in `ui::run`'s bound requires it — confirmed
  by `src/main.rs`'s `TcpClientSession` needing no such wrapper.
- `radio-cat-rs/cat-transport-core/src/completion.rs` and
  `cat-server/src/block_on.rs`/`worker_windows.rs` (read in full): the
  shared single-slot completion primitive (`Waker`-based, satisfied from any
  thread via the standard `Waker` contract) and a ~15-line single-future
  thread-parking `block_on`, both already proven (unit-tested, and used in
  production by `cat-server`'s own Windows path) as the building blocks
  every Windows backend in `radio-cat-rs` is built on. `cat-server`'s own
  `block_on` is a private (`mod block_on`, no `pub use`) implementation
  detail of that crate — not reusable from `ts570d` directly — so `ts570d`
  needs its own copy of this same small, well-understood shape (see
  `src/win_runtime.rs` below).
- `ft991a/src/main.rs`: `ft991a`'s Windows `main` is a **single sequential
  loop**, no `monoio::spawn` anywhere, so its `block_on` only ever drives one
  future at a time. That shape is insufficient here — `ts570d`'s two-task
  design is a different, harder problem, exactly as `radio-cat-rs`'s ADR
  0004 flagged.

## Decision

### 1. Keep both UI tasks on one OS thread on Windows too — a hand-rolled two-future cooperative scheduler, not a `std::thread::spawn` worker

`radio-cat-rs`'s ADR 0004 §1 sketched "a genuine `std::thread::spawn`-based
worker" for `ts570d`'s eventual Windows port. That shape does not compile:
`std::thread::spawn`'s closure bound is `F: Send + 'static`, and moving the
`radio: R` value into a new OS thread requires `R: Send`. `R` is (for every
transport `ts570d` uses or will use) some `Ts570d<S>`, and `Ts570d<S>` is
unconditionally `!Send` because of `SharedSession`'s `Rc<RefCell<Option<S>>>`
— not a Windows-specific property, not fixable by choosing a different `S`.

Two alternatives were weighed:

1. **Make `SharedSession` `Send` (`Arc<Mutex<Option<S>>>` instead of
   `Rc<RefCell<Option<S>>>`), then use a real `std::thread::spawn` worker** —
   **rejected**. This is exactly the kind of change `radio-cat-rs`'s own ADR
   0006 made for `cat-server`'s `ClientRegistry`/`DedupCache` — but
   `cat-server` did it by keeping **two separate, platform-gated
   implementations** (`Rc`/`RefCell` on Linux, `Arc`/`Mutex` on Windows), not
   by changing the shared, heavily-tested type unconditionally. Doing the
   same here would mean touching `radio/src/ts570d.rs` — a file with ~100
   existing unit tests and zero Windows-specific need of its own — to widen
   every lock to a `Mutex` unconditionally, purely to serve a Windows-only
   UI-layer concern that a same-thread design (below) solves without
   touching `radio` at all, and without ever needing `S: Send` either. The
   unnecessary blast radius on shared, cross-platform-tested code is real
   and avoidable.
2. **Keep both tasks on one OS thread, replacing `monoio::spawn`'s
   cooperative scheduling with a small hand-rolled one** — **chosen**. The
   property that matters — "the UI task is always polled promptly, even
   while the radio task is deep inside a slow multi-step diagnostic run" —
   is a statement about *polling fairness between two `Future`s*, not about
   *which OS thread executes them*. `monoio::spawn` achieves it via
   cooperative round-robin polling on one thread; a hand-rolled scheduler
   that does the same round-robin polling, without `monoio`, achieves the
   identical property with no `Send` requirement anywhere, since `Rc`/
   `RefCell` are perfectly sound as long as both futures stay on the thread
   that owns them — exactly `ts570d`'s existing invariant, preserved
   unchanged. This also means **`ui`'s existing `Chan<T> =
   Rc<RefCell<VecDeque<T>>>` channel type, `RadioCmd`/`RadioUpdate`, and the
   entire body of `radio_task`/`ui_task`/`run_diagnostics_task` are reused
   completely unchanged** — the ~2000 lines of diagnostic-step logic in
   `ui/src/terminal.rs` needed zero modification.

**Design** (`ui/src/win_sched.rs`, new file, not gated to Windows — see
"Where this code lives" below):

```rust
pub(crate) fn block_on_two<U, R>(mut ui_fut: Pin<Box<U>>, mut radio_fut: Pin<Box<R>>) -> U::Output
where
    U: Future,
    R: Future<Output = ()> + ?Sized,
{
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut radio_done = false;
    loop {
        if let Poll::Ready(out) = ui_fut.as_mut().poll(&mut cx) {
            return out; // radio_fut is dropped here — cancels it, same as Linux's `drop(radio_handle)`
        }
        if !radio_done {
            if let Poll::Ready(()) = radio_fut.as_mut().poll(&mut cx) {
                radio_done = true;
            }
        }
        std::thread::park_timeout(Duration::from_millis(2));
    }
}
```

`radio_fut` is `Pin<Box<dyn Future<Output = ()>>>` at the call site in
`terminal::run`'s Windows variant (type-erased, since it closes over a
generic `R: Radio`), so `block_on_two`'s `R: ?Sized` bound is required —
`Pin<Box<R>>` defaults to `R: Sized`, which a `dyn Future` does not satisfy.
(Caught by `cargo clippy --workspace --all-targets -- -D warnings` during
implementation, not anticipated up front — recorded here so the reasoning
isn't lost.)

`ThreadWaker` is the same shape `cat_transport_core::completion`'s and
`cat_server::block_on`'s own production code already use (`Wake` that calls
`Thread::unpark()`), so a completion future woken from one of
`cat-transport-serial`'s/`cat-transport-tcp`'s Windows worker threads
correctly unparks this loop promptly instead of waiting out the full 2ms.
The `park_timeout(2ms)` bound (well under the existing 5ms UI-yield and
10ms crossterm-poll windows already baked into `ui_task`) is a safety net
for the sleep replacement below, which registers no waker of its own:

```rust
pub(crate) struct WinSleep { deadline: Instant }

impl Future for WinSleep {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        if Instant::now() >= self.deadline { Poll::Ready(()) } else { Poll::Pending }
    }
}
```

`ui/src/terminal.rs`'s two `monoio::time::sleep(...).await` call sites
(inside `radio_task` and `ui_task`) become calls to a small platform-gated
`yield_sleep(duration)` wrapper (`monoio::time::sleep` on Linux, `WinSleep`
on Windows) — the only change to those two functions' bodies; everything
else in `terminal.rs` is untouched.

**Why this genuinely preserves the responsiveness property, not just
approximates it:** every single iteration of `block_on_two`'s loop polls
`ui_fut` *first*, unconditionally, before touching `radio_fut` at all. A slow
`radio_task` (a multi-second, 107-step diagnostic run) only ever shows up to
this loop as "`radio_fut.poll()` returns `Pending` quickly" — because each of
`radio_task`'s own internal `.await` points (a CAT round trip via
`cat-transport-serial`'s/`cat-transport-tcp`'s Windows completion-based I/O,
or `WinSleep`) is itself a correctly-non-blocking `Future` that returns
`Poll::Pending` rather than blocking the OS thread. `radio_task` blocking
this thread synchronously (defeating the whole design) can only happen if
some transport implementation violates its own `Future` contract by
blocking inside `poll` — which none of `cat-transport-serial`'s,
`cat-transport-tcp`'s, or `cat-transport-udp`'s Windows backends do (per
`radio-cat-rs` ADR 0004/0006). This is the identical correctness argument
`monoio::spawn`'s cooperative model already rests on today (a task that
blocks synchronously inside `poll` starves every other task on the same
thread) — this ADR doesn't weaken it, it reimplements the same contract by
hand.

**Known, accepted difference from Linux:** `event::poll(Duration::from_millis(10))`
inside `ui_task` is (already, on both platforms, unchanged by this ADR) a
*synchronous* blocking call — not an `.await` point — so it blocks
`block_on_two`'s loop (and, symmetrically, `monoio`'s executor on Linux) for
up to 10ms per UI iteration regardless of platform. This is pre-existing
behavior this ADR does not touch or worsen.

### 2. `run_server_mode`'s single top-level future gets its own trivial `block_on` — no two-future scheduling needed

`server::run(session, config)` is one call with no sibling task to stay
responsive against on the calling thread (any per-connection/per-datagram
concurrency is `cat-server`'s own internal concern — see Task 5's
cross-reference below for `server`'s own Windows implementation, which uses
genuine OS threads directly). `src/main.rs`'s `run_app()` — the shared
argument-parsing/session-construction/`ui::run`/`server::run` future used by
both the local-TUI and server-mode paths — needs exactly one thing on
Windows: a way to drive that top-level future to completion at all, since
`#[monoio::main]` doesn't exist there. That is `src/win_runtime.rs`'s
`block_on` (a plain single-future version of `block_on_two` above, no
`ui`/`radio` sibling task involved — copied rather than shared with
`ui::win_sched::block_on_two` since the two have different signatures and
`ui`/the app binary crate cannot depend on each other in the direction that
sharing would require).

### 3. Where this code lives

- **`ui/src/win_sched.rs`** (new file): `block_on_two`, `WinSleep`,
  `ThreadWaker`. Mirrors `radio-cat-rs`'s own established convention for
  code that happens to have no actual platform-specific syscalls in it (see
  `cat-transport-tcp`'s/`cat-transport-udp`'s `windows.rs` modules): **not**
  `#[cfg(target_os = "windows")]`-gated at the module level, so it gets
  real, executable test coverage on every platform this workspace builds
  for (three unit tests, all passing on Linux CI today), even though its
  only production caller (`ui/src/terminal.rs::run`'s Windows variant) is
  itself gated. The module is marked `#[cfg_attr(not(target_os =
  "windows"), allow(dead_code))]` at its `mod` declaration in
  `ui/src/lib.rs` so a Linux build doesn't warn (and fail `-D warnings`)
  about legitimately-unused-there code.
- **`ui/src/terminal.rs`**: `pub async fn run` gains `#[cfg(target_os =
  "linux")]`; a new `#[cfg(target_os = "windows")] pub fn run` (same name,
  so `ui/src/lib.rs`'s `pub use terminal::run;` needs no platform branching
  and `src/main.rs` calls `ui::run(radio)` identically on both platforms —
  `.await`ed on Linux, called directly as a plain synchronous function on
  Windows) builds `radio_task`/`ui_task` as boxed futures and drives them
  via `win_sched::block_on_two`. The two `monoio::time::sleep` call sites
  become a tiny `yield_sleep` wrapper, gated the same way. No other change
  to this 2500-line file.
- **`src/main.rs`** / **`src/win_runtime.rs`** (new file): `run_app()` is
  the shared entry-point future (arg parsing, opening the chosen transport,
  calling `ui::run`/`server::run`) used by both platforms.
  `#[cfg(target_os = "linux")] #[monoio::main(timer_enabled = true)] async
  fn main() { run_app().await }` vs. `#[cfg(target_os = "windows")] fn
  main() { win_runtime::block_on(run_app()); }`. This mirrors `ft991a`'s
  precedent of a platform-gated `main` at the same granularity, while
  `win_runtime.rs`'s ~30-line `block_on` is new, `ts570d`-specific code
  matching `cat-server::block_on`'s shape exactly.

## Consequences

- **Linux behavior is byte-for-byte unchanged.** `ui/src/terminal.rs`'s
  existing `run`/`radio_task`/`ui_task`/diagnostic code is untouched except
  for `#[cfg(target_os = "linux")]` gates and the `monoio::time::sleep` →
  `yield_sleep` indirection (identical body on Linux). Verified by `cargo
  test --workspace` (277 `radio` tests, 21 `ui` tests including 3 new
  `win_sched` tests, `server`/`rigctl_radio` tests, 100 integration tests,
  all passing, zero regressions) after every change.
- **No new external dependency.** `std::thread`, `std::sync::Arc`,
  `std::task::{Context, Poll, Wake, Waker}`, `std::pin::Pin`,
  `std::time::{Duration, Instant}` only — matching every Windows addition in
  `radio-cat-rs` to date. `monoio` becomes a
  `[target.'cfg(target_os = "linux")'.dependencies]` entry in `ui/Cargo.toml`
  (previously unconditional), and every `#[monoio::test]`-based test module
  in `radio`/`server` is gated `#[cfg(all(test, target_os = "linux"))]`.
- **`radio/src/ts570d.rs` is not modified by this ADR.** `SharedSession`
  stays exactly as it is today on both platforms; this was a deliberate goal
  of choosing Decision §1's option 2, not an oversight.
- **Diagnostic abort (`Esc`) behavior is identical in spirit, not identical
  in latency profile.** On Linux, `check_esc()` runs on the same thread as
  everything else, cooperatively, roughly once per diagnostic step. On
  Windows, `check_esc()` still runs inside `radio_task` (unchanged call
  site, unchanged code) — it was never routed through `ui_task` on either
  platform, so this ADR changes nothing about how it works. No latency
  regression is expected; this could not be verified with a Windows runtime
  in this sandbox.
- **Residual risk, stated plainly:** this scheduler has not been run on real
  Windows hardware — no Windows machine is available in this sandbox (see
  this repo's own README/CLAUDE.md precedent for `cargo check --target
  x86_64-pc-windows-gnu` being the verification ceiling here). Its
  correctness argument rests on (a) the `Waker` contract, identical to every
  other Windows primitive `radio-cat-rs` ships and already relies on, and
  (b) every transport `Future` this loop polls genuinely returning `Pending`
  instead of blocking — true by construction for `cat-transport-serial`'s/
  `cat-transport-tcp`'s Windows backends today, but a **future contributor
  extending `radio_task` with a new blocking call would silently break
  responsiveness** with no compiler error to catch it. This is the same
  hazard `monoio`'s own cooperative model has always had on Linux; this ADR
  does not introduce a new category of risk, but does not eliminate it
  either.
- `cargo check --target x86_64-pc-windows-gnu --workspace --exclude emulator`
  is the verification method for this ADR's own code (`emulator` is
  excluded because its virtual-TTY PTY hosting — `serialport::TTYPort`,
  Unix-only by construction — is genuinely out of scope for any of this
  repo's five Windows-support tasks; see this repo's own CLAUDE.md
  "Linux-Specific" section, which already documented this before this ADR).

## Cross-reference

### Task 4 (TCP client mode, `--server <host:port>`)

Depends on this ADR's finding that `ui::run`'s `Radio` bound has no
`ModemControlLines`-equivalent requirement — discovered while reading
`radio/src/radio_trait.rs` for this ADR's own purposes. `src/main.rs`'s
`TcpClientSession` (a plain `CatSession<Error = TransportError>` adapter
around `cat_transport_tcp::TcpCatSession`, mapping `TcpSessionError` by
hand since no blanket `From` conversion exists across crates) is used
directly as `Ts570d<TcpClientSession>`, with no `NoModemControlLines`
wrapper needed at all — unlike `ft991a`, which needs one because of its
`CwKeying: ModemControlLines` bound.

### Task 5 (headless server mode on Windows): a real, upstream gap in `cat-rigctl`

While extending `server`'s Windows support, `cargo check --target
x86_64-pc-windows-gnu -p server` initially failed with `monoio` unresolved
inside `radio-cat-rs`'s `cat-rigctl` crate. This is a **different** gap than
the one this ADR closes for `ui`: `cat-rigctl`'s own `Cargo.toml` already
correctly target-gates `monoio` to Linux, but `cat-rigctl`'s *source*
(`rigctl.rs`, `lib.rs`) calls `monoio::spawn`/`monoio::net` and
`cat_server::tcp`/`udp` (the Linux-gated modules) **unconditionally**, with
no Windows counterpart at all — unlike `cat-transport-serial`/
`cat-transport-tcp`/`cat-transport-udp`/`cat-server` itself, which
`radio-cat-rs`'s ADR 0004/0006 all gave working Windows backends. `ts570d`'s
own `server` crate compounded this: it depended on `cat-rigctl`
unconditionally (not target-gated) and delegated **all** of `server::run` —
raw TCP, raw UDP, and the rigctld bridge alike — entirely through it.

Per this repo's own CLAUDE.md Rule 7 ("If a change to the generic engine or
transport layer is needed, it belongs in `radio-cat-rs`, not as a local
fork/vendor here"), reimplementing `cat-rigctl`'s Hamlib dispatch/framing
logic inside `ts570d` to get it working on Windows was rejected — that would
duplicate protocol-critical logic this repo does not own the source of truth
for. Instead: `cat-rigctl` is now target-gated to
`[target.'cfg(target_os = "linux")'.dependencies]` in `server/Cargo.toml`
(alongside `rigctl_radio.rs`, gated `#[cfg(target_os = "linux")]` in
`server/src/lib.rs`, since it implements `cat_rigctl::RigctlRadio`), and
`server::run` gained a genuine (not stubbed) Windows implementation built
directly on `cat-server`'s already-cross-platform public primitives
(`cat_server::build`, `ClientRegistry`, `tcp_windows::serve`,
`udp_windows::serve`) — bringing up `--raw-tcp-port`/`--raw-udp-port` for
real on Windows, with one dedicated `std::thread` per listener (mirroring
`tcp_windows`/`udp_windows`'s own per-connection-thread model one level up)
racing on a `std::sync::mpsc` channel, the direct `std` analog of the Linux
path's `futures::future::select_all`. Only `--rigctl-port` (the WSJT-X
Hamlib bridge) is rejected on Windows with a clear, actionable error at
`run()`-time, since that specific piece has no Windows-capable
implementation to call into anywhere in this dependency graph today.

**This is a genuine, currently-unresolved gap in `radio-cat-rs`**, not
something this repo can close on its own: `cat-rigctl` needs its own
Windows backend (following the same worker-thread-per-connection shape
`cat-server`'s `tcp_windows`/`udp_windows` already established) before
`ts570d server --rigctl-port` can work on Windows. Recorded here for
whoever picks up that follow-on work in `radio-cat-rs`.
