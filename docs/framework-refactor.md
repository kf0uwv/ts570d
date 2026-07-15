# CAT Framework Refactor

## Motivation

The workspace currently mixes generic CAT transport/controller concerns with TS-570D-specific protocol and emulator semantics. The refactor extracts the reusable command-table, parsing, dispatch, response-building, and framework error lifecycle into `framework` while keeping TS-570D state and command meaning in `radio`.

No second-radio implementation is added in this work.

## Implementation Status

This refactor establishes the generic CAT command-table and dispatch boundary and routes the emulator through `CatFramework<Ts570dRadio>`. TS-570D emulator state and command handlers now reside in the `radio` crate.

Remaining extraction work is deliberately documented rather than hidden:

```text
framework/src/radio.rs still contains legacy UI-facing TS-570D domain types.
radio/src/commands.rs still contains the legacy controller-side metadata table.
radio/src/protocol/* still contains TS-570D response parsing for the controller.
ui diagnostics still exercise the existing Radio trait methods rather than iterating the generic catalog.
```

These remaining items require a separate UI/controller API migration to avoid changing externally visible behavior in the same step.

## Previous Architecture

```text
app
├── radio
│   ├── controller-side TS-570D command metadata
│   ├── response parser/framer
│   └── typed Ts570d<T: Transport> client
├── serial
│   └── Transport implementation
├── ui
│   └── framework::radio::Radio trait diagnostics/UI
├── emulator
│   ├── raw CAT command match
│   └── emulator-local RadioState
└── framework
    ├── Transport trait
    ├── UI-facing Radio trait
    └── TS-570D domain types such as Frequency and Mode
```

Dependency direction before refactor:

```text
framework
  └── no local crate dependencies, but contains TS-570D domain concepts

radio ───────▶ framework
serial ──────▶ framework
ui ──────────▶ framework
emulator ────▶ radio
app ─────────▶ framework, radio, serial, ui
```

## Current Inventory

The current command table is `radio/src/commands.rs` as `COMMAND_TABLE: &[CommandMetadata]`. Each entry contains `code`, `supports_read`, `supports_write`, and `description`.

Read, set, and action commands are currently distinguished by two booleans only. Action commands such as `TX`, `RX`, `RC`, `RU`, and `RD` are modeled as write-only commands rather than a separate operation.

Controller-side request formatting is in `radio/src/client.rs`. It validates command codes against `CommandMetadata::find`, writes `CODE;` for queries, writes `CODEparams;` for sets, and reads until `;` for query responses.

Controller-side response parsing is in `radio/src/protocol/parser.rs`; response framing is in `radio/src/protocol/framing.rs`.

Emulator request framing is in `emulator/src/io.rs`. Emulator command execution is in `emulator/src/commands.rs` as a raw `match` on command strings. Emulator state is in `emulator/src/radio_state.rs`.

UI diagnostics are in `ui/src/terminal.rs` and call the UI-facing `framework::radio::Radio` trait directly. The README describes this as 99 CAT command round trips.

## New Architecture

Target dependency direction:

```text
              ui
              │
              ▼
          application
              │
   ┌──────────┼──────────┐
   ▼          ▼          ▼
 radio      serial   framework
 TS-570D   transport generic CAT
   │          │          ▲
   └──────────┴──────────┘

emulator depends on framework and radio.
```

More specifically:

```text
framework
  ├── command metadata model
  ├── command table lookup
  ├── syntactic parsing
  ├── structural parameter validation
  ├── generic dispatch lifecycle
  ├── response builder
  ├── framework errors
  └── CatCommandCatalog / CatRadio traits

radio
  ├── Ts570dCommandId
  ├── TS570D_COMMAND_TABLE entries
  ├── Ts570dRadio emulator/state-machine implementation
  ├── Ts570dState
  ├── TS-570D handlers and semantic validation
  └── existing controller-side typed client APIs

emulator
  ├── PTY/serialport hosting
  ├── logging
  ├── TUI display
  └── CatFramework<Ts570dRadio> execution
```

## Command Processing Sequence

```text
receive complete semicolon-terminated frame
    ↓
lookup command in CommandTable<C>
    ↓
classify operation/form
    ↓
parse and structurally validate parameters
    ↓
build CommandRequest<'_, C>
    ↓
delegate to CatRadio::handle_command
    ↓
radio mutates TS-570D state and writes response values
    ↓
ResponseBuilder emits wire response
    ↓
return CommandOutcome<E>
```

## Generic Versus TS-570D Responsibilities

Generic framework responsibilities are command framing support where generic, command lookup, syntactic parsing, structural validation, generic dispatch lifecycle, response construction helpers, and framework-level errors.

TS-570D responsibilities are command identifiers, command definitions, radio state, state transitions, TS-570D-specific validation, command semantics, response values, protocol-specific error behavior, emulator defaults, and unsolicited/event behavior.

## Future Extraction Plan

The following can later move into a shared library repository:

```text
framework/src/command.rs
framework/src/dispatch.rs
framework/src/cat_radio.rs
framework/src/errors.rs generic CAT errors
generic command definitions and parameter schemas
generic parser and response builder
CatCommandCatalog / CatRadio traits
CatFramework dispatch lifecycle
```

The following must remain in this repository:

```text
radio/src/commands.rs TS-570D entries
radio/src/ts570d_radio.rs TS-570D emulator state machine
radio/src/ts570d.rs controller client
radio/src/protocol/* typed TS-570D response parsing
emulator/* PTY, logging, and TUI host
ui/*
serial/*
src/main.rs
```

## Adding Another Radio Later

A second radio would define its own command identifier enum, static command table, state machine, errors, events, and `CatRadio` implementation. It would reuse `framework` parsing and dispatch but would not require changes to TS-570D handlers or state.

Example shape using a fake radio:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeCommandId {
    Frequency,
}

struct FakeRadio;

impl framework::CatCommandCatalog for FakeRadio {
    type CommandId = FakeCommandId;

    fn command_table(&self) -> &'static framework::CommandTable<Self::CommandId> {
        &FAKE_COMMAND_TABLE
    }
}
```
