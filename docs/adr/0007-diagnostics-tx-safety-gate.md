# 7. Diagnostics TX safety gate: operator acknowledgment + callsign-gated CW test

Date: 2026-07-26

## Status

Accepted

## Context

The `[D]` diagnostics screen (`ui/src/terminal.rs`'s `run_diagnostics_task`)
runs a 107-step, 3-round self-test against the connected radio. Two of those
steps are not benign read/write round trips — they genuinely operate the
transmitter:

- Step 56 (`transmit`/`receive`) keys PTT and immediately un-keys it.
- Step 82 (`send_cw`) keyed CW with the literal text `"TEST"` — a real
  over-the-air (or over-the-wire, against a physical rig) transmission with
  **no station identification**. Amateur radio regulations require
  transmissions to identify the transmitting station; an unidentified test
  transmission is a compliance problem, not just a cosmetic one.

Separately, the diagnostic run previously started **immediately** when `[D]`
was pressed (`KeyResult::StartDiag` transitioned straight into
`ControlState::Diagnostic(DiagState::Running{..})` and fired
`RadioCmd::StartDiagnostics` unconditionally). An operator who fat-fingers
`[D]`, or simply doesn't realize this screen transmits, gets no warning
before the rig keys up. Transmitting into an open circuit or a badly
mismatched antenna is a well-known way to damage a solid-state transceiver's
final amplifier stage — this is an equipment-safety problem, not just a
UX one.

### What was read before deciding

- `ui/src/control.rs`'s `ControlState`/`KeyResult`/`handle_key` state
  machine, in full — including the existing `TextInput { prompt, buffer,
  error, action }` mechanism already used for one-line text prompts (e.g.
  the interactive "CW message (up to 24 chars):" send-CW menu action), and
  the `'d'`/`'D'` handler that used to jump straight to
  `ControlState::Diagnostic(DiagState::Running{..})`.
- `ui/src/terminal.rs`'s `run_diagnostics_task`, `RadioCmd`, `RadioUpdate`,
  and the `diag_action!`/`diag_get!`/`diag_set_get!` macros that build
  `DiagResult { label, round, passed, detail }` at each of the 107 steps.
- `ui/src/diag.rs`'s `DiagResult`/`DiagState` data model and
  `ui/src/layout.rs`'s `build_summary_lines`/`draw_diag_panel` rendering,
  which grouped by label and rendered strictly `OK` (all rounds passed) or
  `FAILED` (any round didn't) — no third state existed.

## Decision

**1. A hard-to-miss warning gate, requiring explicit acknowledgment.**
`[D]` now transitions to a new `ControlState::DiagWarning` instead of
starting anything. `layout::draw_diag_warning_panel` replaces the whole
Controls panel with a red-bordered, centered warning stating plainly that
the run will key the transmitter and that the radio **must** be connected to
a proper antenna or dummy load, with the amplifier-damage rationale spelled
out. `Enter`/`y`/`Y` proceeds; `Esc` cancels straight back to `Menu` with
nothing sent to the radio task — no `RadioCmd` is ever constructed on the
cancel path.

**2. A callsign prompt, reusing the existing `TextInput` mechanism.**
Acknowledging the warning transitions to `ControlState::TextInput` with a
new `InputAction::DiagCallsign` and the prompt "Callsign for CW test (blank
to skip):" — deliberately reusing the same input widget already used
elsewhere in this file rather than inventing a new one. `Esc` here also
cancels to `Menu` (the TextInput state's existing generic Esc behavior)
with nothing started. Confirming with `Enter` is special-cased in
`handle_key` (not routed through `validate_text_input`/`ExecuteAction`,
since it starts a diagnostic run rather than issuing one radio command):
the buffer is trimmed and length-checked (must fit the `KY` command's
24-character limit once prefixed with `"TEST "`), then `KeyResult::StartDiag(Option<String>)`
carries the callsign (or `None`) to the radio task via
`RadioCmd::StartDiagnostics { callsign }`.

**3. Step 82 branches on the callsign instead of always sending bare `"TEST"`.**
If a non-empty callsign was supplied, `send_cw("TEST <callsign>")` runs
exactly as the old bare `"TEST"` call did (three times, once per round,
via `diag_action!`). If not, the step is **not attempted** — no CAT command
is sent — and is recorded as **skipped**, not as a pass or a failure. This
does not abort the run; every other step proceeds normally.

**4. Skip is a first-class third result state, not an overloaded pass.**
`DiagResult` (and `RadioUpdate::DiagProgress`) gained a `skipped: bool`
field alongside the existing `passed: bool`/`detail: String`. All ~18
existing construction sites (three macros, fourteen inline match arms, one
in `ui_task`'s update handler) set `skipped: false`; only the new step-82
skip branch sets `skipped: true` (with `passed: true`, since a skip is
not a failure, and `detail` explaining why). `layout::build_summary_lines`
renders a label whose rounds are *all* skipped as `...SKIPPED` (yellow),
distinct from `...OK` (green) and `...FAILED` (red), with the skip reason
shown inline. The `Done` panel's summary line now reports
`"{passed}/{total} passed, {skipped} skipped, {failed} failed"` instead of
just passed/failed, so a skip is never silently folded into either bucket.

## Consequences

- Every diagnostic run now requires two explicit keypresses beyond `[D]`
  itself (acknowledge the warning, confirm the callsign prompt — even to
  leave it blank) before anything is sent to the radio. This is an
  intentional friction increase for a screen that transmits.
- The CW keying test is only ever run with station identification. An
  operator who skips it gets a complete diagnostic run minus that one step,
  clearly marked, rather than a silently-illegal unidentified transmission
  or a run that aborts entirely.
- `RadioCmd::StartDiagnostics` and `KeyResult::StartDiag` both changed from
  unit variants to carrying `Option<String>` — a small, contained signature
  change with no fallout beyond `ui/src/control.rs` and `ui/src/terminal.rs`
  (verified: no other crate references either symbol).
- No behavior changed for any of the other 105 diagnostic steps, including
  the PTT key/unkey at step 56 — the warning gate covers that risk generically
  ("this run will key the transmitter") rather than adding a second,
  narrower gate.
- Verified manually against the built-in emulator (`--background --log-file`
  mode): `[D]` → warning → `Esc` cancels with zero commands sent; `[D]` →
  proceed → blank callsign → run completes with the CW step shown as
  `...SKIPPED` and zero `KY` commands in the emulator's command log; `[D]`
  → proceed → `W1AW` → run completes 107/107 passed with the emulator log
  showing `KYTEST W1AW;` sent on all three rounds.
