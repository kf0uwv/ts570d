# 2. TS-570D domain types live in `radio`; `ui` depends on `radio`

Date: 2026-07-15

## Status

Accepted (supersedes the earlier rule "`ui` depends only on `framework`")

## Context

`framework/src/radio.rs` defined the controller/UI-facing `Radio` trait plus
`Frequency`, `Mode`, `InformationResponse`, and `MemoryChannelEntry`. These are
TS-570D-specific: the frequency range (500 kHz–60 MHz), the mode encoding (1–9,
"8 unused on TS-570D"), and the 37-character `IF` response layout are all model
details. Keeping them in `framework` contradicted [ADR 0001](0001-generic-cat-framework.md).

The original design placed them in `framework` so `ui` could depend on `framework`
alone. That constraint is what forced the coupling.

## Decision

Move the `Radio` trait and the TS-570D domain types (`Frequency`, `Mode`,
`InformationResponse`, `MemoryChannelEntry`, `RadioError`, `RadioResult`,
`NopRadio`) from `framework` into `radio` (`radio/src/radio_trait.rs`), re-exported
at the `radio` crate root.

`ui` now depends on **both `framework` and `radio`** and imports these types from
`radio`. `ui` still must not depend on `serial`.

## Consequences

- `framework` no longer contains any TS-570D type, satisfying ADR 0001.
- The dependency graph stays acyclic: `radio → framework`, `ui → {framework, radio}`,
  `emulator → {framework, radio}`, `serial → framework`.
- The CLAUDE.md dependency rules were updated: `ui` may depend on `radio`; the single
  command table and all domain types live in `radio`.
- A future second radio would define its own domain types; there is no shared
  `Frequency`/`Mode` in `framework` to reconcile (deliberately not generalized).
