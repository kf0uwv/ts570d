# 4. Extraction boundary for a shared CAT library

Date: 2026-07-15

## Status

Accepted

## Context

[ADR 0001](0001-generic-cat-framework.md) makes `framework` radio-independent so it
can later move into a shared library reused by other transceivers. We want the
boundary recorded now, while the design is fresh, so a future extraction is a move
rather than a redesign. No second radio is implemented in this work.

## Decision

The following are designed to move into a shared library **as-is**:

```text
framework/src/cat.rs        generic command table, definitions, forms, parser,
                            dispatch lifecycle (CatFramework), response builder,
                            CatCommandCatalog / CatRadio traits, generic CAT errors
framework/src/transport.rs  Transport trait
framework/src/errors.rs     FrameworkError / TransportError
```

The following stay in this (TS-570D-specific) repository:

```text
radio/src/ts570d_radio.rs           Ts570dCommandId, TS570D_COMMAND_TABLE, state machine
radio/src/ts570d_radio_handlers.rs  TS-570D command semantics
radio/src/ts570d.rs                 controller client
radio/src/radio_trait.rs            Radio trait + domain types (see ADR 0002)
radio/src/protocol/*                typed TS-570D response parsing
radio/tests/command_table.rs        TS-570D table integrity tests
emulator/*  ui/*  serial/*  src/main.rs
```

## Adding a second radio

A second radio reuses `framework` unchanged and provides only its own:

- `CommandId` enum, static `CommandTable`, state machine, `Event`/`Error` types;
- a `CatRadio` implementation supplying command definitions and semantics.

Illustrative shape (a fake radio, not a real model):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeCommandId { Frequency }

struct FakeRadio;

impl framework::CatCommandCatalog for FakeRadio {
    type CommandId = FakeCommandId;
    fn command_table(&self) -> &'static framework::CommandTable<Self::CommandId> {
        &FAKE_COMMAND_TABLE
    }
}
```

## Consequences

- Extraction is a file move plus a `Cargo.toml` split; no API redesign is required.
- TS-570D domain concepts are deliberately **not** generalized into `framework`
  pre-emptively (see ADR 0002) — a second radio brings its own.
- Extraction itself is out of scope here; this ADR records readiness only.
