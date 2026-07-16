# 5. Network transport and server/control mode readiness

Date: 2026-07-15

## Status

Accepted

## Context

A long-term requirement (see the network-transport-and-server-modes addendum) is
that the control application must eventually run in two modes:

- **server mode** — owns the physical serial connection, brokers requests from
  remote clients over TCP and/or UDP;
- **control mode** — the interactive UI/controller, usable against a serial
  port, a TCP session, a UDP session, or a mock, without knowing which.

No TCP or UDP transport is implemented in this refactor. This ADR records the
boundary decisions needed so that server mode and network transports can be
added later **without** redesigning `framework`, `radio`, or `ui`. It extends
[ADR 0001](0001-generic-cat-framework.md) and [ADR 0004](0004-extraction-boundary.md);
it does not supersede them.

Before this ADR, `radio::RadioClient<T>` and `radio::Ts570d<T>` were already
generic over `framework::Transport` rather than a concrete serial type, and
`ui` already depended on `radio`'s `Radio` trait rather than any transport —
so requirements 1–4 below were largely satisfied structurally. What was
missing was: (a) an explicit session-level abstraction above raw byte I/O so a
future transport can own its own framing instead of inheriting the
semicolon-scanning loop that `RadioClient` currently performs directly against
`Transport`, and (b) a written record of the boundary so future TCP/UDP work
is additive.

## Decision

### 1. Introduce `CatSession`, a request/response abstraction above `Transport`

`framework::transport::Transport` (byte-level `read`/`write`/`flush`) remains
the lowest-level I/O primitive and is unchanged. A new trait sits above it:

```rust
#[async_trait(?Send)]
pub trait CatSession {
    type Error;

    async fn execute(
        &mut self,
        request: &[u8],
        response: &mut Vec<u8>,
    ) -> Result<ResponseDisposition, Self::Error>;
}
```

`ResponseDisposition` is the existing `framework::cat` type (already used
server-side by `CatFramework`/`CatRadio` dispatch) — reused here rather than
inventing a parallel type, since both sides answer the same question ("was a
response written, was there none, or was there a protocol error").

- `SerialCatSession<T: Transport>` — wraps a `Transport` and performs the
  existing framing: write the request, read bytes until a terminating `;`.
  This is a move of the framing logic currently inlined in
  `RadioClient::read_response`, not new behavior.
- `MockCatSession` / `ScriptedCatSession` — an in-memory implementation for
  tests, matching expected request bytes against a script and returning
  canned responses, with timeout/disconnect/malformed-response simulation
  (see "Testing implications" in the addendum).
- Future `TcpCatSession`, `UdpCatSession` implement the same trait with their
  own framing (length-prefixed envelopes for TCP; datagram envelopes for
  UDP) — framing is a per-implementation concern, never inherited from the
  serial byte-loop.

`radio::RadioClient<S: CatSession>` depends on `CatSession` instead of
reaching into `Transport` directly. This satisfies addendum requirements 8–9:
the trait does not assume one read returns one response, and CAT framing is
decoupled from serial device enumeration.

### 2. Reaffirm the transport-independence boundary already in place

- `ui` depends on `framework` + `radio`'s `Radio` trait — never on `serial`,
  and never will on `tcp`/`udp` transports either (ADR 0002 requirement
  extended to future transports).
- `radio::RadioClient<S>` / `Ts570d<S>` stay generic over the session trait;
  no concrete transport type is named outside `src/main.rs` wiring.
- A mock/scripted session is usable in tests today
  (`radio/src/client.rs`'s `MockTransport` moves to a `CatSession`-level
  `ScriptedCatSession` as part of this work) — addendum requirement 5.
- The trait must not assume Unix file descriptors (requirement 6) or a
  persistent connection unless explicitly expressed (requirement 7):
  `CatSession` expresses neither; a future UDP session can implement it
  without pretending to be connection-oriented.

### 3. Server-mode concerns stay out of the radio state machine

`radio::ts570d_radio.rs` (the `CatRadio`/state-machine implementation used by
both the controller and the emulator) gains no request-broker, client-id,
authentication, or queueing concepts (addendum requirement 10). A future
`cat-server` component (request broker, client session management, physical
radio session ownership — see "Recommended crate boundaries" in the
addendum) sits **above** `radio`, using the same `CatSession`-based client
internally and serializing access through one worker, exactly as `radio` is
used by direct control mode today. This means the physical radio session
(`radio::Ts570d<S>`) must remain ownable by a future broker without change
(requirement 11), and command execution must remain serializable through one
ordered session, which it already is (requirement 12) — `RadioClient`/`Ts570d`
hold `&mut self` and there is no interior concurrency to reconcile.

### 4. Crate/module boundary (reaffirms ADR 0004, adds transport split)

No new crates are created in this refactor. The target boundary, restated
with the transport split from the addendum:

```text
framework        generic CAT engine (cat.rs), CatSession + Transport traits,
                  generic errors — unchanged scope from ADR 0001/0004
radio             Ts570dCommandId, TS570D_COMMAND_TABLE, state machine,
                  RadioClient<S: CatSession>, Ts570d<S: CatSession>
serial            SerialCatSession (via Transport) — Linux io_uring today;
                  a future cat-transport-serial would add Windows COM support
                  behind the same Transport/CatSession traits
emulator, ui, src/main.rs   unchanged scope
```

Future extraction (unchanged from ADR 0004, now including transport crates):
`cat-framework`, `cat-client`, `cat-transport-core`, `cat-transport-serial`,
`cat-transport-tcp`, `cat-transport-udp`, `cat-server` — these may live in
the `radio-cat-rs` shared-library repository once extraction is warranted.
None are created prematurely here (addendum requirement 14; ADR 4
"Consequences").

### 5. Known deviation, tracked but not fixed in this refactor

`framework::transport::Transport` is `#[async_trait(?Send)]` and
`framework/src/lib.rs` re-exports `monoio` directly — the generic framework
crate currently names one async runtime. The addendum recommends a
runtime-compatible associated-future design instead
(`AsyncCatClientTransport` with a GAT `SendFuture`) so `framework` does not
force monoio on a future TCP/UDP implementation, and calls out that `serial`
is Linux-only (io_uring) with Windows COM support deferred. This refactor
does **not** change the runtime binding — "use the simplest design compatible
with the existing runtime," per the addendum — but records it here as the one
open item a future cat-transport-core extraction must resolve (either adopt
the GAT-future trait shape, or confirm monoio's public API is acceptable to
depend on directly in the shared library).

### 6. Architecture documentation

`docs/architecture/network-readiness.md` (added alongside this ADR) diagrams
how serial, TCP, and UDP attach later on both the control-mode and
server-mode sides, per addendum requirement 15.

## Consequences

- `RadioClient`/`Ts570d` become generic over `CatSession` instead of
  `Transport` directly; existing serial behavior is preserved exactly
  (`SerialCatSession` reproduces today's read-until-`;` framing).
- Tests gain a reusable `ScriptedCatSession` and a transport-conformance test
  shape that future `TcpCatSession`/`UdpCatSession` implementations can reuse
  (addendum "Transport conformance tests").
- No TCP, UDP, server broker, envelope protocol, or authentication is
  implemented here. These remain future work, unblocked by this ADR rather
  than designed here.
- The `radio-cat-rs` repository is the eventual home for the crates listed
  in section 4; `ft991a` is the eventual second `CatRadio` implementation.
  Neither repository's extraction happens in this refactor (see the
  companion ADRs recorded in each of those repositories).
