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

This project uses strict dependency inversion. Crates depend on **traits**, never on concrete implementations.

```
framework  (no local crate dependencies)
  └── defines: Transport trait, Radio trait
  └── defines: TransportError, RadioError, RadioResult, domain types (Frequency, Mode, etc.)

serial  (depends on: framework only)
  └── implements: Transport trait for SerialPort

radio  (depends on: framework only)
  └── implements: Radio trait for Ts570d<T: Transport>
  └── Ts570d is generic over T: Transport — never imports serial directly

ui  (depends on: framework only)
  └── uses: Radio trait abstraction (ui::run<R: Radio>(radio: &mut R))
  └── NEVER imports from radio or serial crates

emulator  (depends on: nix, serial internals for PTY — test infrastructure only)

app/src/main.rs  (depends on: all crates — the wiring layer only)
  └── creates Ts570d<SerialPort> and passes &mut radio to ui::run()
```

### Rules (violation is a blocking issue)
1. **`framework`** has NO local crate dependencies. It defines traits and types only.
2. **`radio`** NEVER imports from `serial`. Transport is injected by the app via generics.
3. **`ui`** NEVER imports from `radio` or `serial`. It uses the `Radio` trait from `framework`.
4. **`app/main.rs`** is the ONLY place concrete types are wired together.
5. Unit tests use **mock/fake implementations** of the trait — never the real impl from another crate.
   - `radio` tests use an in-crate `FakeTransport` (not `serial::SerialPort`)
   - `ui` tests use an in-crate `MockRadio` impl of the `Radio` trait

### Radio trait scope
The `Radio` trait in `framework` contains **abstract radio concepts** applicable to any transceiver:
frequency control, mode, PTT, meters, gain controls, power, scan, RIT/XIT, noise blanker,
memory channels, squelch, preamplifier, attenuator, VOX, etc.

TS-570D-specific features (keyer, voice synthesizer, antenna tuner, menu access) live in the
`radio` crate as inherent methods on `Ts570d`, NOT in the `Radio` trait.

## Architecture
- serial/: Custom io_uring RS-232 implementation
- radio/: TS-570D protocol handling + Radio trait implementation
- ui/: Ratatui terminal interface (depends only on framework)
- emulator/: Virtual TTY and radio emulator

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