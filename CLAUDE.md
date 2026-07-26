# Kenwood TS-570D Radio Control - Agent Guidelines

## Superpowers Coding Model (MANDATORY)
- Use planning-with-files skill for ALL implementation work
- Follow TDD, frequent commits, verification-before-completion
- Check for applicable skills BEFORE any action

## Planning-with-Files Requirement
- Each agent and subagent must maintain their own planning-with-files in a directory under `./planning/` with their name
- Example: `./planning/architect/`, `./planning/ui/`, `./planning/kenwood/`, `./planning/serial/`, `./planning/code_review/`
- Planning files include: `task_plan.md`, `findings.md`, `progress.md` in each agent's directory
- This prevents conflicts between agents working on different aspects of the project
- Planning files must be created and maintained before any implementation work

## Planning Directory Ownership and Boundaries
- Each agent owns ONLY their planning directory under `./planning/{agent_name}/`
- Agents must NEVER edit planning files in other agents' directories
- All planning work MUST use planning-with-files skill
- Planning files must be created BEFORE any implementation work
- Each agent is responsible for: `task_plan.md`, `findings.md`, `progress.md` in their own directory only
- Any violation of these boundaries is a critical issue

## Architect Review Workflow (MANDATORY)
- ALL subagents must write their implementation plan to their `task_plan.md` BEFORE writing any code
- Plans are reviewed by the architect and user before work proceeds
- Subagents execute ONE task at a time, reporting results before moving to the next
- The architect coordinates parallelization across subagents
- No subagent proceeds past planning without architect approval

## Core Technologies
- monoio: io_uring async runtime, **Linux only** (target-gated everywhere —
  see "Windows support" below). Windows uses a hand-rolled thread-parking
  executor instead (`src/win_runtime.rs`, `ui/src/win_sched.rs`).
- ratatui + crossterm: Terminal UI
- io_uring serial implementation (external, `radio-cat-rs`'s `cat-transport-serial` — this repo has no local serial transport code). Native Win32 COM-port I/O on Windows, same crate, same public API.
- Built-in emulator with virtual TTY (Linux/Unix-only — see "Windows support")
- Tokio should NEVER be used in this project

## Essential Commands
- Build: `cargo build` / `cargo build --release`
- Test: `cargo test` / `cargo test test_name`
- Lint: `cargo clippy` / `cargo fmt`
- Emulator: `cargo run --bin emulator`
- Windows type-check: `make windows-check` (`cargo check --target x86_64-pc-windows-gnu --workspace --exclude emulator`; requires `rustup target add x86_64-pc-windows-gnu` once)
- Windows package (on/for a Windows host with binaries already built): `make windows-package`

## Windows support

`ts570d` (the main TUI binary) and the `server` crate both build and run on
Windows, in addition to Linux:

- Local serial (`--port`) uses `cat-transport-serial`'s native Win32 COM-port
  backend (radio-cat-rs ADR 0004) — same public API as Linux, no branching
  in application code.
- Remote client mode (`--server <host:port>`) uses `cat-transport-tcp`'s
  Windows backend (radio-cat-rs ADR 0006).
- Headless server mode (`ts570d server ...`) supports `--raw-tcp-port`/
  `--raw-udp-port`/`--rigctl-port` on Windows too, via `cat_rigctl::run`'s
  own Windows backend (radio-cat-rs ADR 0006's 2026-07-26 amendment) — the
  WSJT-X/Hamlib bridge now works identically on both platforms; see
  `docs/adr/0006-windows-concurrency-model.md`'s amendment for the history.
- `ui`'s two-task concurrent design (`monoio::spawn`'d radio-poll task +
  UI-render task on Linux) is replaced on Windows by a hand-rolled two-future
  cooperative scheduler (`ui/src/win_sched.rs`) that preserves the same "UI
  stays responsive during a slow radio poll or diagnostic run" property
  without requiring `Send` (which `Ts570d<S>` can never satisfy — see below).
- The `emulator` crate is **not** Windows-portable and is not part of this
  support: it hosts a pseudo-terminal pair (`serialport::TTYPort`,
  `std::os::unix`), a Unix-only concept with no Windows equivalent attempted
  here. `pin-test` (now a shared `cat-transport-serial` binary in
  radio-cat-rs) is fully cross-platform.

See `docs/adr/0006-windows-concurrency-model.md` for the full design
rationale (including why a real `std::thread::spawn` worker was rejected —
`radio::Ts570d<S>`'s `SharedSession` is unconditionally `!Send`) and its
documented residual risk. There is no Windows machine in this project's
development environment; Windows correctness is verified with `cargo check
--target x86_64-pc-windows-gnu` (type-check only) plus the CI `windows-check`
job — real hardware/runtime validation happens on the release workflow's
`windows-latest` build and, ultimately, users running the released binary.

## Crate Dependency Model (MANDATORY — ALL AGENTS MUST FOLLOW)

As of the 2026-07-16/17 network-transport-readiness refactor, the generic CAT
engine and transport layer have been **extracted** to the sibling repository
`radio-cat-rs` (https://github.com/kf0uwv/radio-cat-rs) and are consumed here
as external git dependencies. The local `framework` and `serial` crates no
longer exist — do not recreate them. See `docs/adr/0004-extraction-boundary.md`
and `docs/adr/0005-network-transport-readiness.md` for the history, and
`radio-cat-rs`'s own ADRs for what each external crate is responsible for.

```
cat-framework        (external, from radio-cat-rs — NOT part of this repo)
  └── generic CAT engine — CommandTable<C>, CommandDefinition<C>, CommandForm,
      CommandOperation, CommandRequest, ParameterValues, ResponseBuilder,
      CommandOutcome, CatCommandCatalog / CatRadio traits, CatFramework<R>
  └── contains NO radio-specific command ids, modes, frequencies, state, or handlers

cat-transport-core    (external) — Transport / CatSession traits, TransportError
cat-transport-serial  (external) — SerialCatSession, SerialPort, SerialConfig (io_uring)
cat-client            (external) — CatClient<C: CommandId, S: CatSession>, ClientError<E>
                        (replaces this repo's old local RadioClient)

radio  (depends on: cat-framework, cat-client, cat-transport-core — never serial-specific)
  └── defines: Ts570dCommandId, TS570D_COMMAND_TABLE (the single command table)
  └── defines: Ts570dRadio (CatRadio impl + emulator state machine), Ts570dState, Ts570dEvent
  └── defines: Radio trait + TS-570D domain types (Frequency, Mode, InformationResponse,
               MemoryChannelEntry, RadioError, RadioResult) — controller/UI-facing
  └── defines: Ts570d<S: CatSession>, wrapping CatClient<Ts570dCommandId, S> internally
  └── Ts570d is generic over S: CatSession — never imports cat-transport-serial directly

ui  (depends on: radio only — no direct cat-framework/cat-transport-* imports today)
  └── uses: radio::Radio trait abstraction (ui::run<R: Radio>(radio: &mut R))
  └── uses: radio domain types (Frequency, Mode, ...) for display
  └── NEVER imports a concrete transport crate

emulator  (depends on: cat-framework, radio)
  └── runs CatFramework<Ts570dRadio>; owns PTY hosting, logging, TUI display

src/main.rs  (depends on: all crates — the wiring layer only)
  └── --port:   creates Ts570d<SerialCatSession<SerialPort>> and passes it to ui::run()
  └── --server: creates Ts570d<TcpClientSession> (app-local CatSession<Error=TransportError>
                adapter around cat_transport_tcp::TcpCatSession) and passes it to ui::run()
  └── server subcommand: wires SerialCatSession<SerialPort> into server::run()
```

### Rules (violation is a blocking issue)
1. Do NOT recreate a local `framework` or `serial` crate, even temporarily — the
   generic engine and transport traits live in `radio-cat-rs` now. Depend on the
   git-based workspace dependencies (`cat-framework`, `cat-client`,
   `cat-transport-core`, `cat-transport-serial`) instead.
2. **`radio`** never imports a concrete transport crate directly — `Ts570d<S>` is
   generic over `S: CatSession`, injected by the app via generics.
3. **`radio`** owns the single source of truth for the command table
   (`TS570D_COMMAND_TABLE`). There must be exactly ONE command table.
4. **`ui`** may depend on `radio` (for the `Radio` trait and domain types) but NEVER
   on a concrete transport crate. It uses the `Radio` trait, not concrete sessions.
5. **`src/main.rs`** is the ONLY place concrete types are wired together.
6. Unit tests use **mock/fake or scripted implementations** of the relevant trait —
   never a real transport impl from another crate.
   - `radio` tests use an in-crate `FakeTransport`/`ScriptedCatSession`-style double
   - `ui` tests use an in-crate `MockRadio` impl of the `Radio` trait
7. If a change to the generic engine or transport layer is needed, it belongs in
   `radio-cat-rs`, not as a local fork/vendor here — see that repo's ADR 0001.

### Generic framework vs. TS-570D responsibilities
The generic `framework` knows how to **process** a command: framing, command lookup,
syntactic parsing, structural parameter validation, generic dispatch lifecycle, and
response construction — all generic over a radio-defined `CommandId`.

The `radio` crate knows what a command **means**: command identifiers, command definitions,
radio state and transitions, state-dependent validation, command semantics, response values,
and protocol-specific errors. It implements `framework::CatRadio` to receive parsed commands.

### Radio trait scope
The `Radio` trait (defined in the `radio` crate, controller/UI-facing) contains abstract
radio concepts: frequency control, mode, PTT, meters, gain controls, power, scan, RIT/XIT,
noise blanker, memory channels, squelch, preamplifier, attenuator, VOX, etc.

TS-570D-specific features (keyer, voice synthesizer, antenna tuner, menu access) live in the
`radio` crate as inherent methods on `Ts570d`, NOT in the `Radio` trait.

## Architecture
- radio/: TS-570D command table, CatRadio impl, controller client (Ts570d<S: CatSession>), Radio trait + domain types
- ui/: Ratatui terminal interface (depends on radio only). `win_sched.rs`: Windows-only two-future cooperative scheduler replacing `monoio::spawn`.
- server/: Headless network server mode (`ts570d server ...`) — thin wiring over `radio-cat-rs`'s `cat-rigctl`/`cat-server` crates. Cross-platform: `--raw-tcp-port`/`--raw-udp-port`/`--rigctl-port` all work on Windows too (see "Windows support").
- emulator/: Virtual TTY + radio emulator, runs CatFramework<Ts570dRadio>. Linux/Unix-only (pseudo-terminals).
- `pin-test` diagnostic binary: no longer local to this repo — it's a shared `[[bin]]` in `radio-cat-rs`'s `cat-transport-serial` crate (`cargo run -p cat-transport-serial --bin pin-test`, or `make pintest`).
- src/main.rs: the wiring layer — `run_app()` shared by both platforms; `#[monoio::main]` on Linux, `win_runtime::block_on` on Windows (`src/win_runtime.rs`). Also defines `--server <host:port>` remote client mode's `TcpClientSession` adapter.
- Generic CAT engine, transport traits, and serial/TCP/UDP transports are external
  dependencies from `radio-cat-rs` (git) — no local `framework`/`serial` crate

## Code Style
- Imports: std → external → local
- Error handling: thiserror + Result<T, E>
- Naming: snake_case/PascalCase conventions
- Async: monoio runtime on Linux; a hand-rolled thread-parking executor on Windows (see "Windows support") — never tokio, never a general-purpose async-executor crate on either platform

## Testing Strategy
- Unit tests for individual components
- Integration tests with virtual TTY (Linux-only, `tests/integration.rs` is `#![cfg(target_os = "linux")]`)
- Performance benchmarks for io_uring
- Linux-only testing with emulator; Windows verified via `cargo check --target x86_64-pc-windows-gnu` (type-check only — see "Windows support")

## Linux-Specific
(Applies to the Linux build path specifically — see "Windows support" above for what differs on Windows.)
- io_uring kernel requirements (5.1+)
- Serial port permissions and udev rules
- Virtual TTY via pseudo-terminals (emulator + integration tests — Unix-only, no Windows equivalent)
- Zero-copy optimizations for serial I/O