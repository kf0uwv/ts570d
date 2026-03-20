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

## Architecture
- serial/: Custom io_uring RS-232 implementation
- radio/: TS-570D protocol handling
- ui/: Ratatui terminal interface
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