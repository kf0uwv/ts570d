# CAT Framework Refactor — Status & Remaining Work

Companion to [framework-refactor.md](framework-refactor.md). Records what is done,
what remains, and how the work was verified, on branch
`refactor/generic-cat-framework`.

## Summary

The `framework` crate is now a radio-independent generic CAT engine. All TS-570D
specifics live in `radio`, and a single `TS570D_COMMAND_TABLE` backs both the
controller and the emulator. The change is behavior-preserving on the wire.

## Completed

| Area | Change | Commit |
|------|--------|--------|
| Dependency rules | CLAUDE.md rewritten: `framework` is generic; `ui` may depend on `radio`; one command table in `radio` | d01f04c |
| Domain types | `Radio` trait + `Frequency`/`Mode`/`InformationResponse`/`MemoryChannelEntry`/`RadioError`/`RadioResult`/`NopRadio` moved `framework` → `radio/src/radio_trait.rs`; `ui` now depends on `radio` | d01f04c |
| Single table | Controller + emulator unified onto `TS570D_COMMAND_TABLE`; legacy `radio/src/commands.rs` deleted; `CommandDefinition` gained explicit `readable`/`writable`; integrity test added | e553363 |
| Docs | `framework-refactor.md` + README updated to the single-table architecture | 7b7d215 |

### Command-table reconciliation (Option 1, manual-authoritative)

The controller (70 cmds) and emulator (63 cmds) previously used two divergent
tables. They were unified into one documented superset:

- Added 9 commands the controller knew but the emulator did not: `FC FN NL ST SP
  OS BK QR MF`. The emulator does not yet emulate these and answers `?;` — the
  same wire result as before they existed (verified: unknown-command and
  handler-default paths both emit `?;`).
- Reconciled `MR` / `SM` / `KY` read/write to the manual. Because the
  query/set/action form model cannot express a "read via selector parameter"
  (e.g. `SM0;`, `MR...;`), controller read/write is stored explicitly on each
  `CommandDefinition` (`readable`/`writable`) rather than inferred from forms.

## Verified

On the macOS-buildable crates (framework, radio, ui, emulator lib):

- Tests: framework 16, radio 295, ui 18, emulator 10 pass.
- `cargo clippy --all-targets --all-features -- -D warnings`: clean.
- `cargo fmt --all -- --check`: clean.

## Not runnable on macOS (needs Linux CI — all pre-existing limitations)

- `serial` lib + `serial/src/bin/pin_test.rs` (io_uring / `libc::__errno_location`).
- `emulator` `pty::tests::test_pty_creation` (expects `/dev/pts/N`).
- `tests/integration.rs` (io_uring).
- `cargo build --release`, and the full-workspace CAT diagnostics run.

## Remaining work

1. **(Optional) Diagnostics polish** — `ui/src/terminal.rs` diagnostics still use a
   fixed list of typed `Radio` calls rather than iterating the generic catalog.
   Behavior is unchanged; migrate only if desired, preserving wire behavior.
2. **Linux CI final validation** — run the full-workspace fmt/check/test/clippy,
   release builds, io_uring integration + emulator PTY tests, and CAT diagnostics.
3. **Follow-up issue** — implement the 9 controller-only commands in the emulator
   (currently `?;`), and revisit `MR/SM/KY` semantics against the manual if a
   stricter model is wanted. Deliberately not fixed in this behavior-preserving
   refactor.
