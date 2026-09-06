# Kenwood TS-570D Radio Control

Terminal-based CAT control for the Kenwood TS-570D/S HF transceiver. Built with Rust, using io_uring for serial I/O on Linux (native Win32 COM-port I/O on Windows) and ratatui for the TUI.

![TS-570D Radio Control](docs/screenshots/control.png)

## Requirements

- **Linux**: kernel 5.1+ (io_uring), serial port access (`dialout` group membership, or root)
- **Windows**: 64-bit Windows with a COM port (native or USB-serial adapter) — see [Windows support](#windows-support) below
- Kenwood TS-570D or TS-570S, or a remote `ts570d server` instance (see [Network modes](#network-modes))
- RS-232C serial connection (or USB-serial adapter)

## Installation

### Debian/Ubuntu package

Download the latest `.deb` from the releases page and install:

```sh
sudo dpkg -i ts570d-radio-control_<version>_amd64.deb
```

Installs three binaries to `/usr/bin/`:

| Binary | Description |
|--------|-------------|
| `ts570d-control` | Main control application |
| `ts570d-emulator` | Virtual radio emulator |
| `rs232c-pintest` | RS-232C wiring/pin diagnostic |
| `ts570d-gui` | GPU console (egui/wgpu), network-only — needs a display |

### Running the whole thing against a virtual radio

```sh
# 1. virtual hardware: a CAT port and a CN4 tap that looks like an RTL-SDR
ts570d-emulator --if-out 127.0.0.1:1234
#    PTY_SLAVE=/dev/pts/5
#    IF_OUT=127.0.0.1:1234

# 2. the control program, owning the radio and serving consoles
ts570d-control server --port /dev/pts/5 \
    --console-port 4532 --if-out 127.0.0.1:1234

# 3. a console
ts570d-gui 127.0.0.1:4532
```

Swap step 1 for a real radio and a real dongle behind `rtl_tcp` and steps
2 and 3 are unchanged — which is the point of the emulator presenting
hardware interfaces rather than serving the console protocol itself.

### Windows

Download the latest `ts570d-radio-control_<version>_windows-x86_64.zip` from
the releases page and extract it. It contains `ts570d.exe`, `pin-test.exe`,
`README.md`, and `LICENSE.txt` — no installer, just run `ts570d.exe` from a
terminal (PowerShell or Command Prompt). See [Windows support](#windows-support).

### Build from source

```sh
cargo build --release        # ts570d and pin-test
cargo build --release -p emulator
```

Binaries are placed in `target/release/` as `ts570d`, `emulator`, and `pin-test`.

## Usage

```sh
ts570d-control --cat-only /dev/ttyS0
```

Full options:

```
Usage: ts570d-control --cat-only <port>  [--baud <rate>] [--stop-bits <n>] [sources]
       ts570d-control --cat-dtr  <port>  [--baud <rate>] [--stop-bits <n>] [sources]
       ts570d-control --server   <host:port>

  --cat-only  CAT only. The station does not key PTT from DTR, so the
              console does not offer the PTT-line control.
  --cat-dtr   CAT, and PTT keyed from the serial DTR line (an ACC2 opto
              interface, say). Offers the [P] PTT-line item.
              Mutually exclusive with --cat-only.

  <port> is a local device or a host:port on an RFC 2217 device server --
  ser2net, a Moxa/Digi box, or this repo's emulator run with --com:
              /dev/ttyUSB0   COM3   127.0.0.1:4001   radio.local:4001

  --baud      Baud rate: 1200, 2400, 4800, 9600  (default: 9600)
  --stop-bits Stop bits: 1 or 2                  (default: 1)

  --server    Attach to a remote `ts570d server` that already owns a radio
              and is sharing it. Example: --server 127.0.0.1:7373

Signal sources (optional, either serial mode):
  --if-out <endpoint>        the radio's IF output: an rtl_tcp server
                             (host:port), or a local dongle (rtl:0, rtl:1)
  --acc2-audio <endpoint>    the ACC2 receive-audio pair: a PCM server
                             (host:port), or a sound device
```

### Naming a device

Every endpoint takes a device *or* a network address, and the shape of the
argument decides which — there is no second flag to say so.

| endpoint | on this machine | over the network |
|---|---|---|
| CAT | `/dev/ttyUSB0`, `COM3` | `radio.local:4001` (RFC 2217) |
| IF output | `rtl:0`, `rtl:1` | `127.0.0.1:1234` (rtl_tcp) |
| ACC2 audio | a sound device | `127.0.0.1:4002` (PCM) |

**An SDR is not a file on any platform.** It is claimed over USB by libusb,
so neither the tty layer nor the sound layer ever sees it — there is no
`/dev` node on Linux and no `COM`-style name on Windows. `librtlsdr`
addresses it by index, which is why `rtl:0` is the syntax and why it is the
same on both.

Sound devices are the opposite: their names are per-host, differ between
machines, and nobody types `plughw:CARD=Codec,DEV=0` from memory. So the
console offers them — see below.

### Choosing a device from the console

The **SOURCE** tab (key `4`) lists what this machine can see, grouped by
kind, with the host's default marked. Arrow keys move, enter attaches, and
each row shows the exact string the flag would have taken — picking is a
shortcut for typing, not a separate mechanism, so what you pick is what you
can write down and use next time.

A group that found nothing and a group that could not be asked read
differently on purpose: `nothing attached` means plug something in, and
`unavailable: ...` means this build cannot see that kind of hardware at all.

`cargo run -p cat-signal-rtlsdr --features device --example enumerate` and
`cargo run -p cat-signal-audio --features device --example enumerate`
answer the same question without a console in the way.

### Building with device support

Both device backends are **off by default**, because each wants a C
toolchain most people compiling this do not need:

```sh
cargo build --features sdr-device            # librtlsdr: --if-out rtl:0
cargo build --features audio-device          # ALSA/WASAPI: --acc2-audio audio:<name>
cargo build --features sdr-device,audio-device
```

Without them the flags still parse and the picker still lists the groups —
each says *why* it is empty rather than implying the hardware is absent.

**The flag names the wiring; the argument names the endpoint.** Whether
this radio's PTT is keyed from DTR is a fact about the shack, not something
software can detect — a port has a DTR pin either way. So you say which,
once. The transport then follows from the shape of the argument, which is
why the same flag reaches a radio on the desk and a radio on the network.

The TS-570D factory default is 9600 baud, 8N1. If your radio has been configured differently, pass `--baud` and `--stop-bits` accordingly.

### Key bindings

| Key | Action |
|-----|--------|
| `F` | Frequency menu |
| `N` | Memory channel menu |
| `M` | Mode / DSP menu |
| `R` | Receive settings |
| `T` | Transmit settings |
| `C` | CW keyer settings |
| `O` | Tones (CTCSS/tone squelch) |
| `S` | System settings |
| `D` | Diagnostics (runs 107 CAT command round-trips × 3 rounds). Shows a TX-safety warning first — requires an antenna/dummy load and explicit acknowledgment — then prompts for a callsign to identify the CW keying test (blank skips that one step; nothing is sent bare). |
| `P` | PTT line — drive the port's DTR or RTS by hand, and watch CTS/DSR. Shown only under **`--cat-dtr`**: `--cat-only` says this station does not key from DTR, and `--server` has no line at the other end of a socket. See below. |
| `Q` | Quit |

### `P` — the PTT line

Most PTT interfaces are not CAT: they hang an opto-isolator off **DTR** and
drop it onto the radio's keying pin (on this station, ACC2 pin 9 through an
LTV4N35). `[P]` drives that line directly, which is what you want when the
question is "is the interface wired right?" rather than "does the radio
answer?".

It goes through the same transmit warning `[D]` does, because it keys the
transmitter just as surely — **connect an antenna or a dummy load first.**

| Key | Action |
|-----|--------|
| `Space` / `Enter` | Key / unkey the selected line |
| `D` / `R` | Choose DTR or RTS (refused while the line is up — unkey first) |
| `Esc` | Release the line and go back |

The status line shows CTS and DSR as the port reports them. On a TS-570D,
CTS follows the radio's COM port being alive, so a line that goes up with
CTS down means the radio is missing, not the cable.

Leaving the screen — by `Esc` or by quitting — restores **DTR deasserted and
RTS asserted**. RTS is not put back low on purpose: the radio treats it as
receive-enable and stops answering CAT while it is down.

For the same thing outside the TUI, with no CAT traffic at all, see the
`ts570d-line` bench tool below.

## Emulator

A built-in emulator lets you run the control program without a physical radio. See [docs/emulator.md](docs/emulator.md) for details. The emulator is Linux/Unix-only (it hosts a pseudo-terminal pair) — it does not build or run on Windows.

## Network modes

### Headless server mode

`ts570d server` runs the control program without a TUI: one process owns the
physical serial port and exposes it to the network, so multiple remote
clients (this application's own `--server` mode, WSJT-X, or any
`radio-cat-rs`-aware client) can share one radio connection.

```
Usage: ts570d server --port <serial-port-path> [--baud <rate>] [--stop-bits <n>]
             [--raw-tcp-port <port>] [--raw-udp-port <port>] [--rigctl-port <port>]

  --raw-tcp-port  Bind cat-server's raw length-prefixed TCP protocol
  --raw-udp-port  Bind cat-server's raw enveloped UDP protocol
  --rigctl-port   Bind a Hamlib rigctld-compatible TCP listener
                  (for WSJT-X's "Hamlib NET rigctl" rig type)
```

At least one of the three listener flags is required.

**Windows note:** all three listener flags, including `--rigctl-port`
(WSJT-X/Hamlib), work on Windows — `radio-cat-rs`'s `cat-rigctl` crate
gained a real Windows backend. See
[docs/adr/0006-windows-concurrency-model.md](docs/adr/0006-windows-concurrency-model.md)'s
amendment.

### Remote client mode

`ts570d-control --server <host:port>` connects the normal TUI to a remote
`ts570d server` instance's raw TCP listener instead of a local serial port —
useful for controlling a radio connected to a different machine:

```sh
# On the machine with the radio attached:
ts570d server --port /dev/ttyUSB0 --raw-tcp-port 7373

# On any machine on the network:
ts570d-control --server radio-host:7373
```

Works on both Linux and Windows.

### Remote *serial port* mode

Passing a `host:port` to `--cat-only`/`--cat-dtr` is a different thing from
`--server`, and the difference is worth being clear about. `--server`
reaches a `ts570d server` process that already owns a radio and is sharing
it. A `host:port` on the serial flags reaches a **bare serial port** and
owns it exclusively — it is a local port over a network.

That matters because RFC 2217 carries the RS-232 **modem control lines**,
not just the bytes. On a station that keys PTT from DTR (an ACC2 opto
interface, say), the whole PTT path works over it; over `--server` it does
not exist at all.

```sh
# Against a real device server:
ts570d-control --cat-dtr radio.local:4001

# Against the emulator, which serves its COM port the same way:
ts570d-emulator --com 127.0.0.1:4001 --if-out 127.0.0.1:1234 --acc2-audio 127.0.0.1:4002
ts570d-control --cat-dtr 127.0.0.1:4001 --if-out 127.0.0.1:1234 --acc2-audio 127.0.0.1:4002
```

Either way it opens with **DTR deasserted** and RTS asserted: DTR because
it may be a PTT key line, RTS because the TS-570D treats it as
receive-enable and withholds CAT responses while it is low.

`ts570d-line` takes the same endpoint, so the bench tool works against a
remote port or the emulator:

```sh
ts570d-line 127.0.0.1:4001 status
ts570d-line 127.0.0.1:4001 dtr hold
```

## Windows support

Windows is a supported target (`x86_64-pc-windows-msvc`, the only one) for
`ts570d.exe`: local serial (native Win32 COM-port I/O), `--server` remote
client mode, and headless server mode (raw TCP/UDP and `--rigctl-port`
alike). The `emulator` and
`pin-test` diagnostic tool have no Windows-specific concerns of their own
(`pin-test` is fully cross-platform; `emulator` is Linux/Unix-only by
design, per above).

This repo has no Windows machine to test against directly — Windows builds
are verified by the CI `windows-check` job, which runs `cargo check` **and
`cargo test`** on a `windows-latest` MSVC host (`radio-cat-rs` ADR 0012),
optionally preceded locally by `make windows-check` (cargo-xwin, best-effort,
cannot run tests); real hardware/runtime validation
happens on the release workflow's actual `windows-latest` build and,
ultimately, by users running the released binary. See
[docs/adr/0006-windows-concurrency-model.md](docs/adr/0006-windows-concurrency-model.md)
for the concurrency-model design and its documented residual risk.

## Architecture

The generic, radio-independent CAT engine and transport layer live in the
sibling library [`radio-cat-rs`](https://github.com/kf0uwv/radio-cat-rs)
(`cat-framework`, `cat-client`, `cat-transport-core`, `cat-transport-serial`
— consumed as git dependencies), with all TS-570D specifics isolated in this
repo's own crates:

| Crate | Responsibility |
|-------|----------------|
| `cat-framework` (external) | Generic CAT engine (command table, parser, dispatch, response builder). No radio-specific types. |
| `cat-transport-serial` (external) | io_uring RS-232 transport on Linux, native Win32 COM-port transport on Windows (implements `Transport`/`CatSession`). |
| `radio` | TS-570D command table, `CatRadio` state machine, controller client (`Ts570d<S: CatSession>`), `Radio` trait + domain types. |
| `ui` | Ratatui terminal interface (depends on `radio` only). |
| `emulator` | Virtual radio; runs `CatFramework<Ts570dRadio>`. |

A single `TS570D_COMMAND_TABLE` backs both the controller and the emulator.
The design — dependency graph, command-processing sequence, extraction
boundary, network-transport readiness, and how a second radio (`ft991a`,
Yaesu FT-991A) implements the generic traits — is recorded as
[ADRs](docs/adr/), both here and in `radio-cat-rs`'s own `docs/adr/`.

## Protocol

CAT command reference: Kenwood TS-570D instruction manual, pages 70–81.
PDF: <https://www.kenwood.com/usa/Support/pdf/TS-570-English.pdf>

## License

Copyright 2026 Matt Franklin. Licensed under the [Apache License, Version 2.0](LICENSE.txt).
