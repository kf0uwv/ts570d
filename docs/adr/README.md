# Architecture Decision Records

Decisions are recorded as [ADRs](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions)
(Michael Nygard format). Each file is one decision; numbers are stable and never reused.

| ADR | Title | Status |
|-----|-------|--------|
| [0001](0001-generic-cat-framework.md) | Radio-independent generic CAT framework | Accepted |
| [0002](0002-domain-types-in-radio.md) | TS-570D domain types live in `radio`; `ui` depends on `radio` | Accepted |
| [0003](0003-single-command-table.md) | One command table for controller and emulator | Accepted |
| [0004](0004-extraction-boundary.md) | Extraction boundary for a shared CAT library | Accepted |
| [0005](0005-network-transport-readiness.md) | Network transport and server/control mode readiness | Accepted |

## Refactor status (branch `refactor/generic-cat-framework`)

**Done:** ADR 0001–0003 implemented. `framework` is generic; TS-570D domain types
and the single `TS570D_COMMAND_TABLE` live in `radio`; the emulator runs
`CatFramework<Ts570dRadio>`; the controller validates against the single table;
legacy `radio/src/commands.rs` removed.

**Verified** (macOS-buildable crates): tests framework 16 / radio 295 / ui 18 /
emulator 10; `clippy -D warnings` and `cargo fmt --check` clean.

**Owed to Linux CI** (io_uring; cannot run on macOS — pre-existing): full-workspace
test/clippy, `serial` + `pin_test`, the emulator PTY test, `tests/integration.rs`,
release builds, and the CAT diagnostics run.

**Remaining / follow-up:**
- (Optional) migrate `ui/src/terminal.rs` diagnostics to iterate the generic
  catalog (behavior currently unchanged).
- Implement the 9 controller-catalog commands the emulator does not yet emulate
  (`FC FN NL ST SP OS BK QR MF`) — see ADR 0003.
- Implement `CatSession` (`SerialCatSession`, `ScriptedCatSession`) and migrate
  `RadioClient`/`Ts570d` onto it — see ADR 0005; tracked in
  `planning/architect/task_plan.md`.
