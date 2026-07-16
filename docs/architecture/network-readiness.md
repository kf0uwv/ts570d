# Network transport and server/control mode readiness

See [ADR 0005](../adr/0005-network-transport-readiness.md) for the decision
record. This document is the diagram referenced there (addendum requirement
15): how serial, TCP, and UDP attach later without changing `framework`,
`radio`, or `ui`.

## Control mode (today: serial only)

```text
ui
 │  (Radio trait, radio domain types)
 ▼
radio::Ts570d<S: CatSession>
 │  (typed get/set methods)
 ▼
radio::RadioClient<S: CatSession>
 │  (command-table validation, wire formatting)
 ▼
framework::CatSession   ◀── the new boundary (this refactor)
 │
 ├── SerialCatSession<T: Transport>   (today, via serial::SerialPort)
 ├── MockCatSession / ScriptedCatSession   (tests, today)
 ├── TcpCatSession    (future — cat-transport-tcp)
 └── UdpCatSession    (future — cat-transport-udp)
```

`ui` never names a transport type. `src/main.rs` is the only place a concrete
`CatSession` implementation is chosen and wired in — today that is always
`SerialCatSession<serial::SerialPort>`. Adding `ts570d control --tcp ...` /
`--udp ...` later is a change to `src/main.rs`'s argument parsing and wiring
only.

## Server mode (future, not implemented here)

```text
TS-570D (physical)
    │ serial
    ▼
serial::SerialPort  ──▶  SerialCatSession  ──▶  radio::Ts570d<SerialCatSession>
                                                       │
                                                       ▼
                                          cat-server request broker
                                          (single worker, ordered access,
                                           client session management)
                                                       ▲
                                    ┌──────────────────┼──────────────────┐
                                    │                                     │
                         TCP server transport                 UDP server transport
                        (cat-transport-tcp)                  (cat-transport-udp)
                                    ▲                                     ▲
                              TCP clients                           UDP clients
                       (ts570d control --tcp host:port)     (ts570d control --udp host:port)
```

Only server mode owns the physical serial connection remotely. The broker is
the single ordering point; `radio::ts570d_radio.rs` (the `CatRadio` state
machine shared by the controller and the emulator) is unaware the broker
exists — it answers commands the same way whether called directly or via the
broker's worker.

## What does not change when TCP/UDP are added

- `framework::cat` (command table, parsing, dispatch, response building) —
  no changes; it never encoded transport concerns.
- `radio::ts570d_radio.rs` / `ts570d_radio_handlers.rs` — no server-mode,
  socket, or client-id concepts are added here.
- `ui` — continues to depend only on the `Radio` trait; it is not told
  whether the session underneath is serial, TCP, or UDP.

## What is added, later, additively

- `cat-transport-tcp` / `cat-transport-udp` implementations of `CatSession`
  (or the lower-level `Transport`, with their own framing) — length-prefixed
  frames for TCP, envelope datagrams with request/session IDs and a
  deduplication cache for UDP.
- `cat-server` — the request broker and client session management described
  in the addendum; depends on `radio` and a `CatSession` implementation, not
  the reverse.
- CLI subcommands (`ts570d server ...`, additional `ts570d control --tcp/--udp
  ...` flags) in `src/main.rs`.
