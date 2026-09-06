# 9. ACC2-IF virtual hardware in the emulator

Date: 2026-09-01

## Status

**Accepted.** Implemented in `emulator/` (`acc2.rs`, `com.rs`,
`acc2_audio.rs`) on `cat-transport-rfc2217`, a new crate in `radio-cat-rs`.

Partially supersedes ADR 0008's "Audio *transport*" exclusion: the wire is
now chosen. The AF panels' *rendering* is unaffected, and the "audio path
configured, no stream yet" state ADR 0008 required remains first-class — it
is what a console shows when nothing is connected to the audio pair.

## Context

The station drives this radio through a homebrew interface box, the
**ACC2-IF** (Rev A, 2026-08-31): USB-C to the PC, and three pigtails to the
radio — DB9 to the COM port, 13-pin DIN to ACC2, SMA to the CN4 IF tap. PTT
is keyed from serial DTR through an opto-isolator onto ACC2 pin 9 (PKS);
receive audio comes off pin 3 (ANO) and transmit audio goes into pin 11
(PKD), both through a USB sound device inside the box.

The emulator already presented two of those three interfaces: a CAT port on
a pseudo-terminal, and the CN4 tap as an RTL-SDR over `rtl_tcp`. `ACC2` was
listed under "Not yet".

Three things forced this decision now.

### 1. The PTY carries no modem control lines, and the documentation said otherwise

`docs/emulator.md` claimed "DTR is already real (it is a PTY modem line)".
Measured on Linux 7.0.0: `TIOCMGET`, `TIOCMBIS` and `TIOCMBIC` all fail with
`ENOTTY` on **both** ends of a `openpty()` pair. Linux ptys implement no
modem-control ioctls at all.

`cat-transport-serial`'s `SerialPort` drives the lines with exactly those
ioctls, and its open path is `let _ = port.set_dtr(true);` — so against the
emulator every line operation failed *silently*. `ts570d-line /dev/pts/N dtr
hold` printed success and keyed nothing. That is service note SN-2 — "this
board once measured perfect while keying nothing" — reproduced in software,
against the one signal the whole interface is built on.

The consequence is not just a missing feature. The `initial_dtr: false` fix
(ADR-less, `planning/ptt-line` Phase 1, shipped in 0.3.0) exists to stop the
radio keying at program start, and **nothing could test it**, because no
virtual radio could observe DTR.

### 2. The audio transport was explicitly undesigned

ADR 0008's 2026-08-28 amendment brought the AF scope and AF FFT into scope
while recording that streaming audio itself — "codec selection, buffering,
TX audio" — was still undesigned, and built the panels against an "audio
path configured, no stream yet" state pending that design. Modelling ANO and
PKD forces the choice.

### 3. The service notes are the interesting part

The datasheet's §9 records six faults this station actually hit, several of
which are software-visible: a cable that keyed the radio the instant it
seated, phantom keying from a back-fed dead adapter, DTR-at-open key-down,
and a board wired one mirror position away. Software that is supposed to
survive these has never been run against them.

## Decision

### The emulator presents the radio side of the ACC2-IF, and the pin map is data

`emulator/src/acc2.rs` models the DIN-13 as all thirteen pins, each with its
Kenwood name, its function, and whether this station wires it, leaves it
unused, or has cut it. A connector modelled as "the four signals we use"
cannot express SN-1 (pin 13 bonded to the braid) or SN-2 (the mirror trap)
at all, because both are about pins that are supposed to do nothing.

`mating_face_number()` maps a solder-side pin number to its number on the
mating face, so SN-2's trap is a function a test can state rather than a
warning prose repeats.

### The COM port is served as an RFC 2217 device server

Rejected: an emulator-local side-channel for the lines, and requiring the
`tty0tty` out-of-tree kernel module.

RFC 2217 (Telnet Com Port Control Option) is the protocol the world already
ships for putting a serial port **with its modem lines** on a network.
`ser2net` speaks it; Moxa, Digi and USR device servers speak it. Choosing it
means the emulator and a real serial device server are interchangeable to
everything upstream, and `ts570d` gains genuine remote-rig-over-a-device-
server support rather than an emulator-only code path.

This is the same reasoning `emulator/src/tap.rs` already records for
choosing `rtl_tcp` on CN4, applied unchanged. A private protocol would have
been less code and would have made the emulator a special case forever.

Per Rule 7 the protocol lives in `radio-cat-rs` — `cat-transport-rfc2217`,
whose `Rfc2217Port` is a `Transport + ModemControlLines`, so
`SerialCatSession<Rfc2217Port>` supplies CAT framing and forwards the lines
with no new framing code. The emulator's device server is built on the same
crate's I/O-free `Rfc2217Peer`, so one implementation of the protocol
exists.

The PTY endpoint stays. It needs no port, most of the suite uses it, and
nothing about it was wrong except the claim made for it.

### The ACC2 audio pair is a paced TCP PCM stream

Rejected: `snd-aloop`, on the ground that already disqualified `tty0tty` —
it cannot run in CI and would stop the emulator being self-contained.

One full-duplex connection carrying 48 kHz mono s16le PCM: ANO out, PKD in.
Paced to real time exactly as the tap paces its IQ, because audio delivered
as fast as the CPU allowed makes an AF scope's timebase a decoration.

**ANO renders the same `Band` the CN4 tap serves**, at the same dial, on the
same envelopes. That is the property worth having: a station visible in the
panorama is audible at the offset the panorama puts it at. A receiver whose
audio was unrelated noise would let a console pass every test while showing
an operator two views that contradict each other.

### Keying goes through the command table

A hardware key on PKS raises TX by feeding `TX;` through
`TS570D_COMMAND_TABLE`, not by writing the `tx` flag. A radio that is off
does not key because a pin went low, and a console must not be able to tell
how the radio was keyed. What the routes do *not* share is the microphone —
PKS mutes it, SS does not — which is the whole reason the radio has two PTT
pins, and is modelled.

### Faults are switches, off by default

`--acc2-fault pin13-bonded` (SN-1) and `--acc2-fault phantom-keying` (SN-3).
Phantom keying *chatters* rather than keying cleanly, because TX/RX chatter
is the symptom the operator actually reported; a fault modelled as a steady
key-down would not reproduce what sent them looking. Clearing a fault
mid-chatter releases the key — a transmitter left up by a cleared fault
would be worse than the fault.

SN-5 needs no switch: a client that opens with DTR asserted keys the radio,
because that is what the port does.

## Consequences

- **`ts570d`'s DTR-at-open fix is now testable**, and is tested
  (`emulator/tests/acc2_if.rs`). So are RTS receive-enable, CTS-follows-power,
  PKS mic-muting, SN-1 and disconnect-while-keyed.
- **`planning/ptt-line` Phase 3** (a TUI PTT-line item) has been blocked
  since 2026-08-23 for want of a radio that answers the line. It is now
  unblocked.
- **`ts570d` gains an RFC 2217 client**, wired up as `--cat-dtr <port>`
  (and `--cat-only <port>` for a station that does not key from DTR) (and `--cat-only` for a station that does not key from DTR) (amended 2026-09-01, after the initial acceptance). It is a
  peer of a local device path, not of `--server`: it owns a bare serial port
  exclusively rather than attaching to a process that already owns a radio,
  which is why it takes the same `--baud`/`--stop-bits` and why the PTT
  line works over it. `ts570d-line` accepts the same endpoint, so the bench
  tool can rehearse a keying problem against the emulator before anyone
  touches the radio.
- **A temporary local `[patch]`** redirects every `radio-cat-rs` crate to the
  sibling checkout, because `cat-transport-rfc2217` is not in a release yet
  and two copies of `cat-transport-core` would not satisfy each other's trait
  bounds. `Cargo.toml` says so at both sites. This must be reverted to a
  tagged git dependency when `radio-cat-rs` cuts the release.
- **`cat_signal::synthetic::Emitter::envelope` became public.** The audio
  path and the IQ path must agree about when a station is transmitting, and
  the only way to guarantee that is for both to ask the same emitter.
- The emulator is still Linux-only, and now for one fewer reason: the COM
  port, the audio pair and the tap are all plain `std::net`. Only the PTY is
  Unix-bound.

## Not done

- **`ts570d server ... --cat-dtr`.** Headless server mode still requires a
  local `--port`. Nothing prevents it — `server::run` is generic over the
  session — but it needs `ServerArgs` to carry a transport choice rather
  than a path, and that was not in this change's scope.
- **A console consuming ACC2 audio.** ADR 0008's AF panels still render the
  "configured, no stream" state; nothing yet connects them to this wire.
  Decoding, buffering and resampling on the consumer side remain undesigned.
- **PSQ and SMET as electrical outputs.** They are modelled and tested as
  values (`acc2::psq_open`, `acc2::smet_volts`) but nothing serves them over
  a wire, because this station wires neither pin.
