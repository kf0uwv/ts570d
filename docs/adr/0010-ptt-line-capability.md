# 10. Keying from the port's DTR/RTS line: an optional `PttLine` capability, and DTR low at open

Date: 2026-09-01

## Status

Accepted

## Context

Most of the PTT interfaces this station uses are not CAT at all. The one on
the bench — a ZS1AN "Alternative" build — drives an LTV4N35's LED from the
serial **DTR** pin through a diode and a 1 k resistor, and drops the
transistor across ACC2 pin 9 (PKS). Nothing in the CAT command table can
reach that: the line is a property of the *port*, not of the protocol.

Two things followed from that, and this ADR records both.

### 1. The port was keying the radio just by being opened

`cat-transport-serial`'s `SerialConfig` defaults `initial_dtr: true`, which
is the historical Unix behaviour (a UART raises DTR on open). On a station
keying PTT from DTR, that is **key-down the instant the program starts** —
measured at +5 V on the DTR pin with the port merely open, and the reason
the radio was found transmitting with nothing running but the console.

### 2. There was no way to key the line at all, and no way to test one

`ts570d` never touched DTR or RTS after open, so the interface could not be
exercised from the console. Worse, it could not be *tested*: a Linux
pseudo-terminal implements no modem-control ioctl on either end
(`TIOCMGET`/`TIOCMBIS`/`TIOCMBIC` all answer `ENOTTY`), so the emulator —
which is a PTY pair — could not observe a line even in principle. Every
claim about DTR behaviour was therefore unverified. ADR 0009 changed that
by serving the radio's COM port as an RFC 2217 device server with genuine
DTR/RTS/CTS; this work is what that unblocked.

### What was read before deciding

- `cat_transport_core::ModemControlLines` and `NoModemControlLines`, and
  `cat-transport-serial`'s forwarding `impl` on `SerialCatSession<T>`.
- `radio/src/ts570d.rs`'s `SharedSession`: an `Rc<RefCell<Option<S>>>` whose
  `take()`/`put_back()` pair hands the session to an in-flight CAT command
  and `expect()`s if it is asked for while checked out.
- `ui/src/control.rs`'s `ControlState`/`KeyResult`/`handle_key`, and the
  `DiagWarning` gate ADR 0007 introduced.
- `ui/src/terminal.rs`'s two-task design and `docs/adr/0006`'s account of
  why it is two tasks.
- `emulator/tests/acc2_if.rs`, which holds the virtual hardware to the
  ACC2-IF datasheet and is the worked example this feature is tested against.
- The TS-570D instruction manual p. 70 on the DB9 pinout, and in particular
  that the radio's **RTS input is receive-enable**.

## Decision

**1. `initial_dtr: false` at every port this program opens.** Local serial
and RFC 2217 alike (`src/main.rs`). CAT on this radio needs no DTR, so
nothing is lost, and a station keyed from DTR no longer transmits because a
program started. `initial_rts` stays **true** — see decision 4.

**2. A `PttLine` optional capability in `radio`, not on `Radio`.**
`radio/src/ptt_line.rs` defines `PttLineKind { Dtr, Rts }`,
`HandshakeState { cts, dsr, dcd }`, and:

```rust
pub trait PttLine {
    fn set_ptt_line(&self, line: PttLineKind, asserted: bool) -> RadioResult<()>;
    fn read_handshake(&self) -> RadioResult<HandshakeState>;
    fn ptt_line_available(&self) -> bool;
}
```

Every method has a **default body reporting the capability absent**, so a
transport with no lines implements it in one line and a console hides the
control rather than offering one that always errors. It is implemented for
`Ts570d<S>` where `S: CatSession<Error = TransportError> + ModemControlLines`.
`radio` names `cat-transport-core` only — the crate-dependency rule it has
always been allowed to depend on — and never a concrete transport (Rule 2).

**3. Availability is a probe, not a type test.** `ptt_line_available` reads
CTS and reports whether the port answered. `NoModemControlLines` satisfies
the bound and errors from every method; a pseudo-terminal implements the
ioctls not at all. Both would pass a type test and neither has a line.

**4. The idle state is DTR deasserted and RTS *asserted*, not "both low".**
`PttLineKind::idle_level` says so, and it is not symmetry-breaking for its
own sake: the radio's RTS input is receive-enable and it withholds its CAT
responses while that line is down (manual p. 70;
`emulator/tests/acc2_if.rs::rts_low_inhibits_the_radios_cat_responses`).
A console that "put everything back to zero" on the way out would leave
itself unable to talk to the radio.

**5. `[P]` in the TUI, behind the same TX gate as `[D]`.**
`ControlState::DiagWarning` becomes `DiagWarning(WarnedAction)` — one gate,
now naming which of the two things behind it is about to key the
transmitter. Acknowledging it opens `ControlState::PttLine { line,
asserted, cts, dsr, error }`: space/Enter toggles, `d`/`r` choose the line,
Esc releases and returns. The item appears only when the capability is
present.

Three details are load-bearing:

- `asserted` is what the port was last *observed* to do. `handle_key` does
  not flip it; the UI task flips it after the line actually moved. A failed
  assert must never render as key-down, and a failed release must never
  render as key-up.
- The line cannot be re-selected while it is up. Switching from DTR to RTS
  with DTR asserted would leave DTR up with nothing on screen pointing at
  it — one keystroke from keying two things at once.
- CTS and DSR are read back live while the screen is up, so an operator who
  sees the line go up with CTS down knows the radio, not the cable, is what
  is missing.

**6. The line is driven from the UI task, not through the radio-command
channel.** Everything else the console does goes through `RadioCmd` to the
radio task. A PTT line does not, for two reasons. First, latency: the radio
task services commands between poll cycles, so a keystroke would wait up to
a whole cycle in each direction, and an unkey that arrives a second late is
a safety defect, not a UX one. Second, unwinding: the guard that releases
the line on a panic has to live in the task that panics.

This means a `&self` capability method can now be called while the radio
task holds the session across an `.await`. `SharedSession::take()` `expect()`s
in that case, which would turn a well-timed keypress into a process abort,
so `PttLine` goes through a new non-panicking `SharedSession::try_with` and
reports `RadioError::Busy`. `Busy` is not shown to the operator: the UI
retries every 5 ms for up to a second, which covers one CAT command
(tens of milliseconds) many times over.

**7. `ui::run_with_ptt_line`, alongside `ui::run`.** `ui::run`'s bound
could not simply be widened to `R: Radio + PttLine`. `src/main.rs`'s
`--server` arm builds `Ts570d<TcpClientSession>`, and `TcpClientSession` — a
main.rs-local type — implements no `ModemControlLines`; Rust has no
specialization, so one `impl PttLine for Ts570d<S>` cannot be real for
sessions with lines and inert for those without. Widening the bound would
therefore have required wrapping the TCP session in `NoModemControlLines`
at the call site.

Instead the capability is passed **beside** the radio, as a
`Box<dyn PttLine>` from `Ts570d::ptt_line_handle()` — a clone of the same
`Rc`-backed session. This is needed anyway (decision 6: the radio task owns
the radio, and the key that unkeys the transmitter does not), and it leaves
`ui::run` and the `--server` arm untouched.

## Consequences

- A station keyed from DTR no longer transmits when the console starts, and
  that is now covered by a test rather than by a measurement someone
  remembers taking.
- The TUI can key the interface, watch CTS, and release it, without the
  operator dropping to `ts570d-line` in another window.
- `RadioError` gains `Busy`. It is producible only by `PttLine`, and only
  because `PttLine` is the one capability whose methods take `&self`.
- A capability that lives outside the `Radio` trait is now the established
  pattern here for anything that is a property of the port rather than of
  the protocol.
- **The `Drop` guard is a last resort and is not a guarantee.** It cannot
  retry — this console is single-threaded, so blocking in `drop` would
  guarantee the radio task never lets the session go — and the release
  profile sets `panic = "abort"`, where `Drop` does not run at all. Every
  path an operator can take (Esc, `[Q]`) releases the line through the
  retrying path first, and closing the port drops the lines regardless. What
  is genuinely uncovered is a panic in the UI task, in a release build, at
  the exact moment the radio task holds the session.
- Adding the `[P]` item to the TUI without a GUI counterpart is a renderer-
  parity exception; see `docs/renderer-parity.md`.

## Not done

- **No polarity inversion flag.** "Assert" means the pin is driven to its
  active level; whether that is +V or 0 V at the interface depends on the
  adapter, and the one on this bench has not been characterised. Adding an
  `--invert-ptt-line` before measuring would be guessing in configuration.
- **No hold-to-talk.** The screen is a latch, because a terminal reports key
  releases only in modes crossterm does not have enabled here. A latch that
  is honest about being a latch beats a push-to-talk that silently sticks.
- **Not wired in `src/main.rs`.** `ui::run_with_ptt_line` exists and is
  tested; the two `--port` and `--cat-dtr` call sites that would pass it a
  handle are owned by another change in flight.
