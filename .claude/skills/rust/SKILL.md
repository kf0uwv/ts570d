---
name: rust
description: Use when writing Rust in this repo or its siblings — the shared CAT framework, a radio's command table, a transport, or a console. Covers the architecture rules, the runtime, and the read-the-manual discipline.
---

# Rust in this fleet

Three radio repositories (`ts570d`, `ft991a`, `ic7100`) sit on one shared
library (`radio-cat-rs`). This describes how they actually work.

> **This file replaces a generic template.** The version before it
> recommended `tokio-serial` — which this project forbids — and gave
> Kenwood protocol examples that were not this radio's. If something here
> disagrees with the code, the code is right and this should be fixed.

## The rules that are not negotiable

- **Never tokio.** Linux uses `monoio` (io_uring), target-gated. Windows
  uses a hand-rolled thread-parking executor. See `docs/adr/0006`.
  `#[monoio::main]` needs `timer_enabled = true` wherever a broker runs,
  or the runtime panics the first time something times out — with a
  message about the runtime, a long way from the cause.
- **The wiring layer is the only place a transport is named.** Rule 5.
  `src/main.rs` chooses; every crate below it is generic over its session.
- **A radio crate never imports a concrete transport.**
- **Changes to the engine or a transport belong in `radio-cat-rs`**, never
  vendored locally.

## Read the manual before writing a command

Every command, mode byte and meter calibration cites the page it came
from. This is not ceremony:

- On CI-V there are **no names, only numbers**. A wrong byte does not
  fail — it runs a different real command.
- On the ASCII protocols a wrong field width parses as a valid value.

Manuals live in each repo's `docs/manuals/`. `ts570d` and `ft991a` have
agent files (`kenwood.md`, `yaesu.md`) enforcing the same discipline.

**Tests cannot check this for you.** An assertion checks that bytes match
what the *test author expected*; it cannot check that the expectation
matches what the manufacturer documented. Both radios' repos have a
`wire`-style example that prints what the radio actually says, for reading
against the manual — that is what found the CI-V byte-order trap after
every test was green.

## The three protocol shapes

| radio | wire | framing |
|---|---|---|
| TS-570D, FT-991A | ASCII, `FA00014074000;` | read until `;` |
| IC-7100 | binary CI-V, `FE FE 88 E0 03 FD` | addressed; read until `FD` |

CI-V differs in ways the ASCII shape never had to handle: frames are
addressed (four radios share a bus), the radio **echoes** the
controller's own frame, and byte order is not uniform — **frequency is
little-endian BCD and everything else is big-endian**. See
`cat_framework::civ`.

Anything that stringifies bytes (`from_utf8_lossy`) is correct for ASCII
and destroys a CI-V frame. That mistake has been made three times in this
codebase, at three different layers.

## Where things live

```
cat-framework      the engine, and CatWireFormat (AsciiLineFormat, CivFormat)
cat-transport-*    serial / TCP / UDP / RFC 2217
cat-client         generic request/response
cat-server         the broker
cat-rigctl         the Hamlib bridge
cat-native         the typed console protocol
cat-layout         a console's arrangement and palette, as data
cat-ui             what a console shows, renderer-independent
cat-ui-ratatui     the terminal console (shared by every radio)
cat-ui-egui        the GPU console (likewise)
```

A radio crate holds its command table, capabilities, state machine, typed
client, and its **own console layout and theme** — the server publishes
those, and both consoles render them. See radio-cat-rs ADR 0020.

## Consoles are derived, not written

A console reads the capability document: which tabs exist, which meters,
which bands, which modes. If you find yourself hardcoding a band list or a
mode list in a renderer, that is the bug — it was there for months and was
a TS-570D's nine HF bands showing on an FT-991A and an IC-7100.

## Testing

- Unit tests use fakes of the relevant trait, never a real transport.
- `ui` tests use a `MockRadio`; `radio` tests use a scripted session.
- Emulators exist so a console can be tested against a radio nobody owns —
  and their meters are computed from a synthetic band, so an S-meter that
  disagreed with the spectrum would fail rather than pass quietly.
- Verify by looking: the consoles have offscreen renderers
  (`cargo run -p gui --example render -- out.png`) because a layout is the
  thing assertions are worst at.
