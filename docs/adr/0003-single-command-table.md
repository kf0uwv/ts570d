# 3. One command table for controller and emulator

Date: 2026-07-15

## Status

Accepted

## Context

Two divergent command tables existed:

- the controller validated against `radio::commands::COMMAND_TABLE` — 70
  `CommandMetadata` entries with `code` + `supports_read`/`supports_write` booleans;
- the emulator dispatched against the generic `TS570D_COMMAND_TABLE` — 63
  `CommandDefinition` entries with query/set/action `CommandForm`s.

They disagreed: 9 commands existed only for the controller (`FC FN NL ST SP OS BK
QR MF`), 2 only for the emulator (`LM PB`), and `MR`/`SM`/`KY` were classified
differently. This is a latent bug — controller and emulator already disagreed on
the supported command set — and it duplicated the source of truth.

A wrinkle: the query/set/action form model cannot express a "read that takes a
selector parameter" (e.g. `SM0;`, `MR...;`). The form parser treats any payload as a
`Set`, so read/write capability cannot always be inferred from forms alone.

## Decision

Unify onto a single `TS570D_COMMAND_TABLE` in `radio/src/ts570d_radio.rs`, using the
TS-570D manual as authoritative, and preserving existing wire behavior (Option 1):

- Add explicit `readable`/`writable` fields to `framework::CommandDefinition`.
  `is_readable()`/`is_writable()` return them; the `definition!` macro derives them
  from the presence of forms by default, with explicit overrides where the manual
  disagrees (`MR`, `SM`, `KY`). This is how the selector-parameter reads are modelled.
- Make the table the documented **superset**: add the 9 controller-only commands.
  The emulator does not yet emulate them and answers `?;` — the same wire result as
  when they were absent (both the unknown-command path and the handler default emit
  `?;`).
- The controller (`radio::RadioClient`) validates against this single table; the
  legacy `radio/src/commands.rs` is deleted.
- `radio/tests/command_table.rs` locks in integrity: unique ids/codes, well-formed
  codes, a capability per command, and that every table command is dispatched and
  never reported `UnknownCommand`.

## Consequences

- One source of truth for both the controller and the emulator.
- No wire-behavior change: `readable`/`writable` reproduce the previous controller
  validation exactly; the emulator still answers `?;` for the unemulated commands.
- The typed response parser (`radio/src/protocol/*`) stays radio-specific and is not
  folded into the generic table.
- Follow-up (not done here, to keep the change behavior-preserving): implement the 9
  controller-only commands in the emulator, and revisit `MR`/`SM`/`KY` semantics
  against the manual if a stricter model is wanted.
