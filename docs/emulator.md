# TS-570D Emulator

The emulator provides a virtual Kenwood TS-570D that speaks the full CAT protocol over a pseudo-terminal (PTY) pair. It lets you develop, test, and demo the control program without a physical radio.

![TS-570D Emulator](screenshots/emulator.png)

## Running

```sh
ts570d-emulator
```

The emulator creates a PTY pair and prints the slave device path (e.g. `/dev/pts/4`) to the header bar. Point `ts570d-control` at that path:

```sh
ts570d-control --port /dev/pts/4
```

## Network interface (a dummy radio for a console)

```sh
ts570d-emulator --native 127.0.0.1:4532
```

Serves the native typed protocol alongside the PTY, and prints the bound
address as `NATIVE_LISTEN=…` so a script can find it. Point the GUI at it:

```sh
ts570d-gui 127.0.0.1:4532
```

**It is the same radio.** A native command is translated into the CAT frame
a real client would have sent and fed to the same emulated radio the PTY
serves, so tuning over the network moves this emulator's own display. A
second state machine would be a dummy radio that disagrees with the dummy
radio, and the first time the two drifted it would look exactly like a
console bug.

### Synthetic signals

The emulator generates a spectrum with signals in it, so a waterfall has
something to look like and click-to-tune has somewhere to tune to. Five
kinds, with genuinely different shapes:

| | width | behaviour |
|---|---|---|
| CW | ~100 Hz | keys on and off at sending speed |
| Digital | ~50 Hz | 15-second slots, hard edges, FT8-shaped |
| SSB | ~2.6 kHz | leans to its sideband, restless |
| AM | ~6 kHz | carrier plus two symmetric sidebands |
| Noise | ~20 kHz | flat and wide — deliberately *not* a signal |

Signals live at **absolute frequencies**, and the window follows the dial
the way an IF tap does. So retuning moves the window over a fixed
landscape: tune to a carrier and it comes to the centre. They are crowded
into the HF amateur bands, with the space between them close to empty,
because a console that only ever sees busy spectrum is never tested
against a quiet band.

`--seed <n>` chooses the band. The same seed gives the same signals in the
same places every run, which is what lets "the carrier that was here
yesterday" be a useful thing to say while debugging a console. The default
is fixed for that reason; pass a different one for a different band.

## Interface

The emulator TUI has two panels:

**Left — Radio display**
- S-meter bar with calibrated tick marks (S1–S9, +20 dB)
- Large LED-style frequency readout (MHz, 10 Hz resolution)
- Mode indicator (USB, LSB, CW, FM, AM, FSK)
- Status flags: RX/TX, ANT1/ANT2, CTRL

**Right — Command log**
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
