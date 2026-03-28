# Kenwood TS-570D Radio Control

Terminal-based CAT control for the Kenwood TS-570D/S HF transceiver. Built with Rust, using io_uring for serial I/O and ratatui for the TUI.

![TS-570D Radio Control](docs/screenshots/control.png)

## Requirements

- Linux kernel 5.1+ (io_uring)
- Kenwood TS-570D or TS-570S
- RS-232C serial connection (or USB-serial adapter)
- Serial port access (`dialout` group membership, or root)

## Installation

### Debian/Ubuntu package

Download the latest `.deb` from the releases page and install:

```sh
sudo dpkg -i ts570d-radio-control_0.1.0_amd64.deb
```

### Build from source

```sh
cargo build --release
```

Binaries are placed in `target/release/`:

| Binary | Description |
|--------|-------------|
| `ts570d` | Main control application |
| `emulator` | Virtual radio emulator |
| `pin-test` | RS-232C pin/wiring diagnostic |

## Usage

```sh
ts570d-control /dev/ttyS0
```

The serial port defaults to 4800 baud, 8N2, no flow control — matching the TS-570D factory default. Pass `--help` for all options.

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
| `D` | Diagnostics (runs all 104 CAT commands) |
| `Q` | Quit |

## Emulator

A built-in emulator lets you run the control program without a physical radio. See [docs/emulator.md](docs/emulator.md) for details.

## Protocol

CAT command reference: Kenwood TS-570D instruction manual, pages 70–81.
PDF: <https://www.kenwood.com/usa/Support/pdf/TS-570-English.pdf>

## License

Copyright 2026 Matt Franklin. Licensed under the [Apache License, Version 2.0](LICENSE.txt).
