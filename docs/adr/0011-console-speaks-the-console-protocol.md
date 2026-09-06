# ADR 0011 — `--server` speaks the console protocol

Status: Accepted (2026-09-02)

## Context

`ts570d --server <host:port>` connected to a `ts570d server`'s
`--raw-tcp-port` using `cat-transport-tcp`: the Kenwood command set, framed
and sent over a socket. It worked, and it was the wrong protocol for a
console.

Raw CAT is a byte pipe. Over it a console cannot read the radio's
capabilities, cannot receive spectrum frames, and cannot ask what the
radio's host has attached. So the terminal console in network mode was a
readout and little else, while the graphical console — talking the native
protocol to `--console-port` on the same server — had all three.

That gap was recorded in `docs/renderer-parity.md` as a fidelity ground.
It was not one. Nothing about a terminal prevents any of it; the console
was pointed at the wrong port.

## Decision

`--server` speaks the **native console protocol**. Point it at a server's
`--console-port`.

The old behaviour survives as `--server-raw`, unchanged, because it is the
only way to reach a raw listener and deleting it would break setups that
point at one. Its help text says plainly what it gives up.

`src/native_console.rs` implements `radio::Radio` over
`cat_native::Client`. Three things make that work:

1. **Getters read a cache, not the wire.** A console asks about forty
   questions per redraw. `ReadState` answers all of them in one message,
   which is the point of it existing: a readout assembled from forty round
   trips could show a frequency from one instant beside a mode from
   another, describing a radio that never existed.

2. **The adapter drives itself.** `Radio` has no refresh hook — it is a set
   of questions, asked in whatever order a console draws — so every getter
   calls `tick`, which drains the reader thread and, at most ten times a
   second, asks for fresh state.

3. **Most of the trait stays `NotImplemented`.** Keyer speed, CTCSS,
   speech processor and dozens more have no wire representation. `ui`'s
   `poll!` was changed to skip `NotImplemented` rather than record it:
   a field this link cannot reach is a statement about reach, not a fault,
   and reporting forty of them would put a permanent error banner over a
   console working exactly as it can.

## Consequences

### The waterfall arrives already correct

The server computes the FFT — the dongle is on its machine — and centres
each frame on the dial it reads from the same state the console reads.
`RemoteSpectrum::retune` is therefore **deliberately empty**. Re-centring
on this side would apply the correction twice and put every signal at
double its true offset: a plausible-looking picture that is wrong
everywhere except the centre. There is a test that fails with exactly that
sentence.

### An attach is asked for, not done

Over this protocol the *server* opens the device, so `Attached::Remote`
carries no feed and the console says "asked the radio's host for …"
rather than "attached …". The host's answer — including "device or
resource busy", in its own words — arrives afterwards through
`ConsoleSources::notices`, a queue for things a network link says at a
moment nobody chose.

### Audio stays local-only

The protocol carries spectrum frames and no audio, so the AF panels are
dark in this mode and the server refuses an audio attach naming which half
is missing. See `docs/renderer-parity.md`.

### One mode mapping, in `radio`

`to_mode`/`from_mode` moved from `server/src/console.rs` into
`radio::capabilities`, because there are now two callers on opposite ends
of the same socket. A radio that disagreed with itself about what
`CwLower` means depending on which end you asked would be a genuinely
confusing bug to chase.
