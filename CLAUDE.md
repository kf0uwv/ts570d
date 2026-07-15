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
- monoio: io_uring async runtime
- ratatui + crossterm: Terminal UI
- Custom io_uring serial implementation
- Built-in emulator with virtual TTY
- Tokio should NEVER be used in this project
- 
## Essential Commands
- Build: `cargo build` / `cargo build --release`
- Test: `cargo test` / `cargo test test_name`
- Lint: `cargo clippy` / `cargo fmt`
- Emulator: `cargo run --bin emulator`

## Crate Dependency Model (MANDATORY — ALL AGENTS MUST FOLLOW)

This project uses strict dependency inversion. `framework` is a **radio-independent
generic CAT engine**. All TS-570D-specific knowledge lives in the `radio` crate.

```
framework  (NO local crate dependencies — generic, radio-independent)
  └── defines: generic CAT engine — CommandTable<C>, CommandDefinition<C>, CommandForm,
               CommandOperation, CommandRequest, ParameterValues, ResponseBuilder,
               CommandOutcome, CatCommandCatalog / CatRadio traits, CatFramework<R>
  └── defines: Transport trait, FrameworkError, TransportError, ApplicationStateMachine
  └── contains NO radio-specific command ids, modes, frequencies, state, or handlers

serial  (depends on: framework only)
  └── implements: Transport trait for SerialPort

radio  (depends on: framework only)
  └── defines: Ts570dCommandId, TS570D_COMMAND_TABLE (the single command table)
  └── defines: Ts570dRadio (CatRadio impl + emulator state machine), Ts570dState, Ts570dEvent
  └── defines: Radio trait + TS-570D domain types (Frequency, Mode, InformationResponse,
               MemoryChannelEntry, RadioError, RadioResult) — controller/UI-facing
  └── implements: Radio trait for Ts570d<T: Transport> (controller client)
  └── Ts570d is generic over T: Transport — never imports serial directly

ui  (depends on: framework + radio)
  └── uses: radio::Radio trait abstraction (ui::run<R: Radio>(radio: &mut R))
  └── uses: radio domain types (Frequency, Mode, ...) for display
  └── NEVER imports from serial

emulator  (depends on: framework + radio)
  └── runs CatFramework<Ts570dRadio>; owns PTY hosting, logging, TUI display

app/src/main.rs  (depends on: all crates — the wiring layer only)
  └── creates Ts570d<SerialPort> and passes &mut radio to ui::run()
```

### Rules (violation is a blocking issue)
1. **`framework`** has NO local crate dependencies and contains NO radio-specific types.
   It defines the generic CAT engine, `Transport`, and generic errors/state only.
2. **`framework`** NEVER depends on `radio`, `serial`, `ui`, or `emulator`. Verify with
   `cargo tree -p framework` — no local crate must appear.
3. **`radio`** NEVER imports from `serial`. Transport is injected by the app via generics.
4. **`radio`** owns the single source of truth for the command table
   (`TS570D_COMMAND_TABLE`). There must be exactly ONE command table.
5. **`ui`** may depend on `radio` (for the `Radio` trait and domain types) but NEVER on
   `serial`. It uses the `Radio` trait, not concrete transports.
6. **`app/main.rs`** is the ONLY place concrete types are wired together.
7. Unit tests use **mock/fake implementations** of the relevant trait — never the real impl
   from another crate.
   - `framework` tests use an in-crate fake `CommandId`/`CatRadio` (NEVER import `radio`)
   - `radio` tests use an in-crate `FakeTransport` (not `serial::SerialPort`)
   - `ui` tests use an in-crate `MockRadio` impl of the `Radio` trait

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
- framework/: Generic radio-independent CAT engine (command table, parser, dispatch, response builder) + Transport trait
- serial/: Custom io_uring RS-232 implementation (implements Transport)
- radio/: TS-570D command table, CatRadio impl, controller client, Radio trait + domain types
- ui/: Ratatui terminal interface (depends on framework + radio)
- emulator/: Virtual TTY + radio emulator, runs CatFramework<Ts570dRadio>

## Code Style
- Imports: std → external → local
- Error handling: thiserror + Result<T, E>
- Naming: snake_case/PascalCase conventions
- Async: monoio runtime throughout

## Testing Strategy
- Unit tests for individual components
- Integration tests with virtual TTY
- Performance benchmarks for io_uring
- Linux-only testing with emulator

## Linux-Specific
- io_uring kernel requirements (5.1+)
- Serial port permissions and udev rules
- Virtual TTY via pseudo-terminals
- Zero-copy optimizations for serial I/O