# 1. Radio-independent generic CAT framework

Date: 2026-07-15

## Status

Accepted

## Context

The `framework` crate mixed generic CAT (Computer Aided Transceiver) concerns with
TS-570D-specific protocol and emulator semantics. This coupled reusable
command-table, parsing, dispatch, and response-building logic to one radio model
and blocked reuse for a second transceiver.

We want the reusable engine to be extractable into a shared library later, without
dragging TS-570D command definitions, state, or modes along with it.

## Decision

`framework` is a **radio-independent generic CAT engine**. It owns, generic over a
radio-defined `CommandId`:

- the command table model — `CommandTable<C>`, `CommandDefinition<C>`, `CommandForm`,
  `CommandOperation`;
- syntactic parsing and structural validation → `CommandRequest<C>` / `ParameterValues`;
- the dispatch lifecycle — `CatFramework<R>`;
- response construction — `ResponseBuilder`, `CommandOutcome`;
- the delegation traits — `CatCommandCatalog` and `CatRadio`;
- generic errors and the `Transport` trait.

A radio crate implements `CatRadio` (associated `CommandId`, `Event`, `Error`) to
supply command definitions and semantics. `framework` never matches on TS-570D
commands and contains no TS-570D command ids, modes, frequencies, state, or handlers.

## Consequences

- The framework processes a command (framing, lookup, parse, validate, delegate,
  format); the radio decides what a command means.
- `framework/src/cat.rs`, `transport.rs`, and `errors.rs` can be lifted into a shared
  library with minimal change; a second radio only needs its own `CommandId` enum,
  command table, state machine, and `CatRadio` impl.
- Framework unit tests use a fake in-crate `CommandId`/table (never import `radio`),
  proving the boundary.
- `cargo tree -p framework` must show no local crate dependency.

## Target dependency graph

Acyclic, pointing inward toward the generic crate:

```text
ui ──▶ framework
 │       ▲
 └──▶ radio ──▶ framework
serial ─────────▶ framework
emulator ──▶ { framework, radio }
app (src/main.rs) ──▶ all crates (wiring only)
```

`framework` depends on no local crate. `radio` never imports `serial`; transport is
injected via generics. See also [ADR 0002](0002-domain-types-in-radio.md).

## Command processing sequence

```text
receive complete semicolon-terminated frame
  → CommandTable<C>::parse: lookup code, classify operation, structural validation
  → CommandRequest<C> handed to CatRadio::handle_command (radio semantics + state)
  → ResponseBuilder emits the wire response
  → CommandOutcome<E> returned (response disposition + events)
```

The framework never `match`es on a TS-570D command; dispatch is a radio-local concern.
