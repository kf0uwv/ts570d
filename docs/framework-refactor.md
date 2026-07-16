# CAT Framework Refactor

## Motivation

The workspace currently mixes generic CAT transport/controller concerns with TS-570D-specific protocol and emulator semantics. The refactor extracts the reusable command-table, parsing, dispatch, response-building, and framework error lifecycle into `framework` while keeping TS-570D state and command meaning in `radio`.

No second-radio implementation is added in this work.

## Implementation Status

The `framework` crate is now radio-independent, and both the emulator and the
controller run through the single generic command table.

Completed:

```text
framework contains only the generic CAT engine, Transport, errors, and app state.
TS-570D domain types (Radio trait, Frequency, Mode, InformationResponse,
    MemoryChannelEntry) moved to radio/src/radio_trait.rs; ui now depends on radio.
Emulator dispatches through CatFramework<Ts570dRadio>.
Controller (radio::RadioClient) validates against the single TS570D_COMMAND_TABLE.
The legacy radio/src/commands.rs (CommandMetadata) table is removed — one table remains.
radio/tests/command_table.rs locks in table integrity (unique ids/codes, dispatch coverage).
```

Deliberately deferred (recorded, not hidden):

```text
radio/src/protocol/* still holds the typed TS-570D response parser — this is
    legitimately radio-specific and stays in the radio crate.
ui diagnostics (ui/src/terminal.rs) still drive the typed Radio trait methods
    rather than iterating the generic catalog; behavior is unchanged.
The single table is the documented superset: 9 commands (FC FN NL ST SP OS BK QR MF)
    are present for the controller but not yet emulated — the emulator answers "?;"
    for them, exactly as before they were added. Implementing them is a follow-up.
```

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

## Inventory (after refactor)

The single command table is `radio/src/ts570d_radio.rs` as
`TS570D_COMMAND_TABLE: CommandTable<Ts570dCommandId>`, built from
`CommandDefinition<Ts570dCommandId>` entries. Each entry carries the wire code,
name/description, per-operation `CommandForm`s (query/set/action/response), and
explicit controller `readable`/`writable` capability.

Query, set, and action commands are distinguished by `CommandOperation`. Action
commands (`TX`, `RX`, `RC`, `RU`, `RD`, `UP`, `DN`) use `action_forms`.

Controller-side request formatting is in `radio/src/client.rs`. It validates
command codes against `TS570D_COMMAND_TABLE` (via `is_readable()`/`is_writable()`),
writes `CODE;` for queries, `CODEparams;` for sets, and reads until `;`.

Controller-side response parsing is the typed `radio/src/protocol/parser.rs`;
response framing is `radio/src/protocol/framing.rs`. These remain radio-specific.

The emulator state machine and TS-570D handlers live in
`radio/src/ts570d_radio.rs` and `radio/src/ts570d_radio_handlers.rs`; the emulator
binary hosts a PTY and drives `CatFramework<Ts570dRadio>`.

UI diagnostics are in `ui/src/terminal.rs` and call the UI-facing `radio::Radio`
trait directly (behavior unchanged by this refactor).

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

The following can later move into a shared library repository as-is:

```text
framework/src/cat.rs — generic command table, definitions, forms, parser,
    dispatch lifecycle (CatFramework), response builder, CatCommandCatalog /
    CatRadio traits, and generic CAT errors
framework/src/transport.rs — Transport trait
framework/src/errors.rs — FrameworkError / TransportError
```

The following must remain in this (or a TS-570D-specific) repository:

```text
radio/src/ts570d_radio.rs — Ts570dCommandId, TS570D_COMMAND_TABLE, Ts570dRadio state
radio/src/ts570d_radio_handlers.rs — TS-570D command semantics
radio/src/ts570d.rs — controller client
radio/src/radio_trait.rs — Radio trait + TS-570D domain types (Frequency, Mode, ...)
radio/src/protocol/* — typed TS-570D response parsing
radio/tests/command_table.rs — TS-570D table integrity tests
emulator/* — PTY, logging, and TUI host
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
