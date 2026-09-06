# TS-570D Emulator

The emulator provides a virtual Kenwood TS-570D that speaks the full CAT protocol over a pseudo-terminal (PTY) pair. It lets you develop, test, and demo the control program without a physical radio.

![TS-570D Emulator](screenshots/emulator.png)

## Running

```sh
ts570d-emulator
```

The emulator creates a PTY pair and prints the slave device path (e.g. `/dev/pts/4`) to the header bar. Point `ts570d-control` at that path:

```sh
ts570d-control --cat-only /dev/pts/4
```

## Virtual devices

The emulator is a **radio**, not a server. It presents the interfaces a
TS-570D presents, and the control program connects to them exactly as it
would to the real thing:

| device | how |
|---|---|
| CAT serial | a PTY, path printed as `PTY_SLAVE=…` |
| radio COM port | `--com <addr>`, an RFC 2217 device server, printed as `COM_PORT=…` |
| ACC2 audio pair | `--acc2-audio <addr>`, 48 kHz mono s16le PCM, printed as `ACC2_AUDIO=…` |
| CN4 IF tap | `--if-out <addr>`, an RTL-SDR over `rtl_tcp`, printed as `IF_OUT=…` |

```sh
ts570d-emulator --com 127.0.0.1:4001 \
                --acc2-audio 127.0.0.1:4002 \
                --if-out 127.0.0.1:1234
```

The COM port and the ACC2 audio pair together are the radio side of the
**ACC2-IF** — the station's sound-card and CAT interface box. See
[ACC2-IF virtual hardware](#acc2-if-virtual-hardware) below.

An earlier version served the console's own network protocol directly.
That was the wrong layer: it put the radio and the server in one box and
made the control program unnecessary to test anything. The control program
is the thing that owns a radio and serves consoles.

### The CN4 tap is an RTL-SDR

It speaks `rtl_tcp` — librtlsdr's own protocol — so a virtual tap, a real
dongle and an actual `rtl_tcp` server are interchangeable to everything
downstream. The FFT, the windowing and the inversion correction all run for
real against it.

**The IQ is mirrored**, because that is what comes off CN4: a TS-570D's LO1
is high-side, so its tapped spectrum arrives reversed. A tap that served
un-mirrored IQ would make the control program's correction cancel a
distortion that was never applied — right on the bench and wrong the moment
real hardware appeared.

**The window follows the dial.** The SDR is parked on the 73.05 MHz first
IF and the radio's local oscillator does the tuning, so retuning over CAT
moves the window. That is the property a console's click-to-tune depends
on.

### Synthetic signals

Five kinds, with genuinely different shapes:

| | width | behaviour |
|---|---|---|
| CW | ~100 Hz | keys on and off at sending speed |
| Digital | ~50 Hz | 15-second slots, hard edges, FT8-shaped |
| SSB | ~2.6 kHz | leans to its sideband, restless |
| AM | ~6 kHz | carrier plus two symmetric sidebands |
| Noise | ~20 kHz | flat and wide — deliberately *not* a signal |

They live at absolute frequencies and are crowded into the HF amateur
bands, with the space between them close to empty: a console that only
ever sees busy spectrum is never tested against a quiet band. Density is
about one signal per 8 kHz of window, which leaves each one its own shape.

`--seed <n>` chooses the band. The same seed gives the same signals in the
same places every run, so "the carrier that was here yesterday" is a useful
thing to say while debugging a console.

## ACC2-IF virtual hardware

The station drives this radio through a homebrew interface box (the
**ACC2-IF**, Rev A): a USB-C cable to the PC, and three pigtails to the
radio — DB9 to the COM port, 13-pin DIN to ACC2, SMA to the CN4 IF tap. The
emulator presents the radio side of all three, so the box's behaviour can be
exercised with no TS-570D on the bench.

### The PTY cannot carry a PTT line — which is why `--com` exists

This document used to say, under "Not yet":

> **ACC2** — the audio path and the DTR PTT line. DTR is already real (it is
> a PTY modem line); the audio is not emulated yet.

**The parenthetical was false.** Linux pseudo-terminals implement no
modem-control ioctls at all: `TIOCMGET`, `TIOCMBIS` and `TIOCMBIC` fail with
`ENOTTY` on *both* ends. There was no DTR on the PTY to observe and no CTS
to raise, and because `cat-transport-serial` opens with
`let _ = port.set_dtr(…)`, the failure was silent. The emulator was
reproducing service note SN-2 in software: measuring perfect while keying
nothing.

Since the ACC2-IF keys PTT from DTR and nothing else, a transport that
carries the wires is a precondition for modelling it. `--com` serves the
radio's COM port as an **RFC 2217** device server — the Telnet Com Port
Control Option, which carries DTR/RTS/CTS/DSR/DCD alongside the bytes. The
argument is the one the CN4 tap already makes for `rtl_tcp`: `ser2net` and
every Moxa/Digi device server speak RFC 2217, so this virtual radio and a
real serial device server are interchangeable to whatever is upstream.

```sh
ts570d-emulator --com 127.0.0.1:4001
# COM_PORT=127.0.0.1:4001
```

The PTY has not gone anywhere. It needs no port, most of the test suite
uses it, and nothing about it was wrong except the claim quoted above.

### What the DB9 does (datasheet §7)

| Pin | Signal | Behaviour |
|---|---|---|
| 2 | RXD | CAT responses out |
| 3 | TXD | CAT commands in |
| 4 | DTR | in — keys ACC2 pin 9 (PKS) through the opto |
| 7 | RTS | in — receive-enable; **low inhibits CAT responses** |
| 8 | CTS | out — asserted while the radio is on; `PS0;` drops it |

DSR and DCD are never asserted. That is not an omission: the datasheet's
DB9 table wires neither pin, because the radio's COM connector does not
drive them. `ts570d-line status` should show them low.

### What ACC2 does (datasheet §6)

All thirteen pins are modelled, including the ones nothing is wired to and
the one that is cut — a connector reduced to "the four signals we use"
could not express the service notes below at all.

| Pin | Name | Behaviour |
|---|---|---|
| 3 | **ANO** | RX AF out, **fixed** level from Menu 34 — does *not* follow AF gain |
| 5 | PSQ | squelch status out |
| 6 | SMET | S-meter out, same raw 0–30 scale `SM;` reports |
| 9 | **PKS** | PTT in — ground to transmit, **mic muted** |
| 11 | **PKD** | mic/data audio in |
| 13 | SS | PTT in, **mic live** — physically cut (SN-1), inert |

Keying through PKS raises TX by feeding `TX;` through the same command
table a CAT client reaches it by, so a console cannot tell how the radio was
keyed — and a radio that is off does not key just because a pin went low.
What the two routes do not share is the microphone, which is the entire
reason the radio has two PTT pins.

### The ACC2 audio pair

`--acc2-audio <addr>` serves one full-duplex TCP connection carrying
**48 kHz mono signed 16-bit little-endian** PCM: server to client is ANO
(receive audio), client to server is PKD (transmit audio). Paced to real
time, exactly as the CN4 tap paces its IQ.

ANO carries **the same band the waterfall is showing**. Both are rendered
from one `cat_signal::synthetic::Band` at the same dial, so a station
visible as a trace in the panorama is audible as a tone at the offset the
panorama puts it at, keying and fading on the same envelope. A receiver
whose audio was unrelated noise would let a console pass every test while
showing an operator two views that contradict each other.

The sideband is real: USB hears above the dial, LSB below, CW listens in a
narrow window at the sidetone pitch, and a transmitting radio produces no
receive audio at all. An empty band still carries a noise floor, so
"connected but quiet" and "dead" do not look the same.

Audio arriving on PKD is measured and shown in the TUI, and **never keys the
radio** — SN-6: this station keys via DTR only, VOX stays off.

### Reproducible faults

Off by default; a virtual radio that misbehaved out of the box would be a
worse radio, not a more honest one.

```sh
ts570d-emulator --com 127.0.0.1:4001 --acc2-fault pin13-bonded
ts570d-emulator --com 127.0.0.1:4001 --acc2-fault phantom-keying
```

| fault | service note | what it does |
|---|---|---|
| `pin13-bonded` | SN-1 | a DIN lead with pin 13 tied to the braid: seating the plug keys the radio, **mic live** |
| `phantom-keying` | SN-3 | CTS back-feeds a dead adapter, DTR floats positive and keys PKS marginally — TX/RX chatter, not a clean key-down |

Two more service notes are modelled as behaviour rather than as switches.
SN-5 (Linux raises DTR at open, so a client that does not force it low keys
the radio at startup) is simply what the port does — which is what finally
makes `ts570d`'s `initial_dtr: false` fix testable. SN-2's mirror trap is
modelled as data: `acc2::mating_face_number` maps a solder-side pin to its
number on the mating face, so the trap can be stated in a test rather than
only warned about in prose.

## Interface

The emulator TUI has two panels:

**Left — Radio display**
- S-meter bar with calibrated tick marks (S1–S9, +20 dB)
- Large LED-style frequency readout (MHz, 10 Hz resolution)
- Mode indicator (USB, LSB, CW, FM, AM, FSK)
- Status flags: RX/TX, ANT1/ANT2, CTRL

**Right — Command log**
- `PORT:` the PTY slave path
- `ACC2:` the connector — whether a plug is seated, which pin has the radio
  keyed (`PKS keyed`, or `SS keyed (pin 13!)` in red if the cut pin is
  somehow live), whether the microphone is muted, and the level arriving on
  PKD. The real radio's front panel says none of this, which is exactly why
  the emulator shows it.
- Live feed of every CAT command received (`→`) and every response sent (`←`)
- Command annotations showing the operation name and parameter meaning

## Supported commands

The emulator implements the following CAT commands from the TS-570D manual (pages 70–81):

| Category | Commands |
|----------|----------|
| Frequency | FA, FB, IF |
| Mode | MD |
| VFO/Memory | FR, FT, MC, MR, MW |
| Tuning steps | DN, UP |
| Meters | SM, RM |
| Gain | AG, RG, MG |
| Squelch | SQ |
| Power | PC, PS |
| TX/RX | TX, RX |
| Noise | NB, NR |
| Filters | SH, SL, BC, IS, FW |
| Antenna | AN, AC |
| RIT/XIT | RT, XT, RC, RD, RU |
| Scan | SC |
| Lock / Step | LK, FS |
| VOX | VX, VG, VD |
| Tones | CN, CT, TN, TO |
| CW | KS, PT, SD, CA, KY |
| Speech | PR, VR, LM, PB |
| Preamp/Att | PA, RA |
| AGC | GT |
| Auto info | AI |
| Menu | EX |
| Misc | BY, ID, FV, SR |

SET commands update emulator state immediately; subsequent query commands reflect the new state.

## Notes

- The emulator is intended for development and testing only — it does not model RF behaviour, propagation, or audio
- PTY is torn down when the emulator exits; the control program will disconnect cleanly
- Press `q` in the emulator window to quit
