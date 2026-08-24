// Copyright 2026 Matt Franklin
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! TS-570D Radio Control Application
//!
//! Main entry point: parse `--port <path>` (local serial) or `--server
//! <host:port>` (remote `ts570d server` instance) from argv, open the
//! chosen transport, create a typed Ts570d client, and run the ratatui UI.
//!
//! The emulator (if needed) runs as a **separate process**:
//!   cargo run --bin emulator
//! It prints the PTY slave path to stdout; pass that path here via --port.
//!
//! # Platforms
//!
//! Linux uses `#[monoio::main]` (io_uring). Windows has no `monoio` at all
//! (io_uring is a Linux kernel interface) and drives the same `run_app()`
//! future via a hand-rolled thread-parking executor instead — see
//! `docs/adr/0006-windows-concurrency-model.md` and `win_runtime.rs`.

#[cfg(target_os = "windows")]
mod win_runtime;

use tracing::info;

use cat_transport_core::{CatSession, ResponseDisposition, TransportError};
use cat_transport_serial::{SerialCatSession, SerialConfig, SerialPort};
use cat_transport_tcp::{TcpCatSession, TcpSessionError};
use radio::Ts570d;

/// Which transport to open for the local TUI (`--port` or `--server`,
/// mutually exclusive — see [`parse_args`]).
enum Transport {
    /// `--port <path>`: open a local serial port directly.
    Serial {
        port: String,
        baud: u32,
        stop_bits: u8,
    },
    /// `--server <host:port>`: connect to a remote `ts570d server`
    /// instance's raw TCP listener instead of opening a local serial port.
    Server { addr: String },
}

/// Parsed command-line arguments for the local TUI.
struct Args {
    transport: Transport,
}

/// Command-line arguments specific to `ts570d server ...`: headless network
/// server mode, one process owning the serial port and exposing it over the
/// network to WSJT-X (via the rigctld-compatible listener) and/or other
/// `radio-cat-rs`-aware clients (via the raw `cat-server` TCP/UDP
/// listeners), instead of running the local TUI.
struct ServerArgs {
    port: String,
    baud: u32,
    stop_bits: u8,
    raw_tcp_port: Option<u16>,
    raw_udp_port: Option<u16>,
    rigctl_port: Option<u16>,
}

/// Print usage and exit with code 1.
fn usage_exit() -> ! {
    eprintln!(
        "Usage: ts570d --port <serial-port-path> [--baud <rate>] [--stop-bits <n>]\n\
         \n\
           --port      Serial port path (required, mutually exclusive with --server)\n\
                       Examples: /dev/pts/5  /dev/ttyUSB0  COM3\n\
           --baud      Baud rate: 1200, 2400, 4800, 9600  (default: 9600)\n\
           --stop-bits Stop bits: 1 or 2                  (default: 1)\n\
         \n\
         Usage: ts570d --server <host:port>\n\
         \n\
           --server    Connect to a remote `ts570d server` instance's raw\n\
                       TCP listener instead of opening a local serial port\n\
                       (mutually exclusive with --port/--baud/--stop-bits).\n\
                       Example: --server 127.0.0.1:7373"
    );
    std::process::exit(1);
}

/// Parse `--port <path>`, `--baud <rate>`, `--stop-bits <n>`, or `--server
/// <host:port>` from `std::env::args()`. Unknown flags are silently ignored.
/// Exits with an error message and code 1 for missing/invalid values, or if
/// `--port`/`--baud`/`--stop-bits` and `--server` are combined.
fn parse_args() -> Args {
    let mut args_iter = std::env::args().skip(1);
    let mut port: Option<String> = None;
    let mut baud: u32 = 9600;
    let mut stop_bits: u8 = 1;
    let mut server: Option<String> = None;

    loop {
        match args_iter.next().as_deref() {
            Some("--port") => match args_iter.next() {
                Some(path) => port = Some(path),
                None => usage_exit(),
            },
            Some("--server") => match args_iter.next() {
                Some(addr) => server = Some(addr),
                None => usage_exit(),
            },
            Some("--baud") => match args_iter.next() {
                Some(val) => {
                    let rate: u32 = val.parse().unwrap_or_else(|_| {
                        eprintln!("error: --baud value must be a number, got {:?}", val);
                        std::process::exit(1);
                    });
                    match rate {
                        1200 | 2400 | 4800 | 9600 => baud = rate,
                        _ => {
                            eprintln!(
                                "error: invalid baud rate {}; valid values: 1200, 2400, 4800, 9600",
                                rate
                            );
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    eprintln!("error: --baud requires a value");
                    std::process::exit(1);
                }
            },
            Some("--stop-bits") => match args_iter.next() {
                Some(val) => {
                    let n: u8 = val.parse().unwrap_or_else(|_| {
                        eprintln!("error: --stop-bits value must be a number, got {:?}", val);
                        std::process::exit(1);
                    });
                    match n {
                        1 | 2 => stop_bits = n,
                        _ => {
                            eprintln!("error: invalid stop bits {}; valid values: 1 or 2", n);
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    eprintln!("error: --stop-bits requires a value");
                    std::process::exit(1);
                }
            },
            Some(_) => {}
            None => break,
        }
    }

    match (port, server) {
        (Some(_), Some(_)) => {
            eprintln!("error: --port and --server are mutually exclusive");
            std::process::exit(1);
        }
        (Some(p), None) => Args {
            transport: Transport::Serial {
                port: p,
                baud,
                stop_bits,
            },
        },
        (None, Some(addr)) => Args {
            transport: Transport::Server { addr },
        },
        (None, None) => usage_exit(),
    }
}

/// Print `ts570d server` usage and exit with code 1.
fn server_usage_exit() -> ! {
    eprintln!(
        "Usage: ts570d server --port <serial-port-path> [--baud <rate>] [--stop-bits <n>]\n\
                     [--raw-tcp-port <port>] [--raw-udp-port <port>] [--rigctl-port <port>]\n\
         \n\
           --port          Serial port path (required)\n\
           --baud          Baud rate: 1200, 2400, 4800, 9600  (default: 9600)\n\
           --stop-bits     Stop bits: 1 or 2                  (default: 1)\n\
           --raw-tcp-port  Bind cat-server's raw length-prefixed TCP protocol\n\
           --raw-udp-port  Bind cat-server's raw enveloped UDP protocol\n\
           --rigctl-port   Bind a Hamlib rigctld-compatible TCP listener\n\
                           (for WSJT-X's \"Hamlib NET rigctl\" rig type)\n\
           At least one of --raw-tcp-port/--raw-udp-port/--rigctl-port is required."
    );
    std::process::exit(1);
}

/// Parse `ts570d server`'s own flags from `std::env::args()`, skipping both
/// the program name and the `server` subcommand word itself (positions 0
/// and 1).
fn parse_server_args() -> ServerArgs {
    let mut args_iter = std::env::args().skip(2);
    let mut port: Option<String> = None;
    let mut baud: u32 = 9600;
    let mut stop_bits: u8 = 1;
    let mut raw_tcp_port: Option<u16> = None;
    let mut raw_udp_port: Option<u16> = None;
    let mut rigctl_port: Option<u16> = None;

    fn parse_port_number(val: Option<String>, flag: &str) -> u16 {
        match val.and_then(|v| v.parse::<u16>().ok()) {
            Some(p) => p,
            None => {
                eprintln!("error: {flag} requires a valid port number (0-65535)");
                std::process::exit(1);
            }
        }
    }

    loop {
        match args_iter.next().as_deref() {
            Some("--port") => match args_iter.next() {
                Some(path) => port = Some(path),
                None => server_usage_exit(),
            },
            Some("--baud") => match args_iter.next() {
                Some(val) => {
                    let rate: u32 = val.parse().unwrap_or_else(|_| {
                        eprintln!("error: --baud value must be a number, got {:?}", val);
                        std::process::exit(1);
                    });
                    match rate {
                        1200 | 2400 | 4800 | 9600 => baud = rate,
                        _ => {
                            eprintln!(
                                "error: invalid baud rate {}; valid values: 1200, 2400, 4800, 9600",
                                rate
                            );
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    eprintln!("error: --baud requires a value");
                    std::process::exit(1);
                }
            },
            Some("--stop-bits") => match args_iter.next() {
                Some(val) => {
                    let n: u8 = val.parse().unwrap_or_else(|_| {
                        eprintln!("error: --stop-bits value must be a number, got {:?}", val);
                        std::process::exit(1);
                    });
                    match n {
                        1 | 2 => stop_bits = n,
                        _ => {
                            eprintln!("error: invalid stop bits {}; valid values: 1 or 2", n);
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    eprintln!("error: --stop-bits requires a value");
                    std::process::exit(1);
                }
            },
            Some("--raw-tcp-port") => {
                raw_tcp_port = Some(parse_port_number(args_iter.next(), "--raw-tcp-port"))
            }
            Some("--raw-udp-port") => {
                raw_udp_port = Some(parse_port_number(args_iter.next(), "--raw-udp-port"))
            }
            Some("--rigctl-port") => {
                rigctl_port = Some(parse_port_number(args_iter.next(), "--rigctl-port"))
            }
            Some(_) => {}
            None => break,
        }
    }

    if raw_tcp_port.is_none() && raw_udp_port.is_none() && rigctl_port.is_none() {
        eprintln!("error: at least one of --raw-tcp-port/--raw-udp-port/--rigctl-port is required");
        std::process::exit(1);
    }

    match port {
        Some(p) => ServerArgs {
            port: p,
            baud,
            stop_bits,
            raw_tcp_port,
            raw_udp_port,
            rigctl_port,
        },
        None => server_usage_exit(),
    }
}

/// Adapts `cat_transport_tcp::TcpCatSession` (`CatSession<Error =
/// TcpSessionError>`) to `CatSession<Error = TransportError>`, the bound
/// `Ts570d<S>` requires. Mirrors `ft991a::main::TcpClientSession`'s shape
/// exactly (see `docs/adr/0006-windows-concurrency-model.md`'s
/// cross-reference note). Unlike `ft991a`, `ts570d` wraps this in nothing
/// further: `ui::run`'s `Radio` bound has no `ModemControlLines`-equivalent
/// requirement (confirmed by reading `radio/src/radio_trait.rs` — `ts570d`
/// has no CW-keying feature, per this repo's own CLAUDE.md), so no
/// `cat_transport_core::NoModemControlLines` wrapper is needed here.
struct TcpClientSession {
    inner: TcpCatSession,
}

impl TcpClientSession {
    fn new(inner: TcpCatSession) -> Self {
        Self { inner }
    }
}

/// `TcpSessionError` -> `TransportError`: the orthogonal error-mapping step
/// every network-transport consumer needs regardless of modem-line concerns
/// (see `cat_transport_core::NoModemControlLines`'s doc comment for why this
/// mapping can only be written here, not in a shared crate).
fn map_tcp_err(err: TcpSessionError) -> TransportError {
    match err {
        TcpSessionError::Io(e) => TransportError::Io(e),
        TcpSessionError::FrameTooLarge { len, max } => TransportError::Other(format!(
            "frame length {len} exceeds max frame size {max} bytes"
        )),
    }
}

#[async_trait::async_trait(?Send)]
impl CatSession for TcpClientSession {
    type Error = TransportError;

    async fn execute(
        &mut self,
        request: &[u8],
        response: &mut Vec<u8>,
    ) -> Result<ResponseDisposition, Self::Error> {
        self.inner
            .execute(request, response)
            .await
            .map_err(map_tcp_err)
    }

    async fn send(&mut self, request: &[u8]) -> Result<(), Self::Error> {
        self.inner.send(request).await.map_err(map_tcp_err)
    }

    fn flush_rx(&mut self) {
        self.inner.flush_rx();
    }
}

/// `ts570d server ...` -- headless network server mode: one process owns
/// the serial port, exposed over the network to WSJT-X (via the new
/// rigctld-compatible listener) and/or other `radio-cat-rs`-aware clients
/// (via the existing raw `cat-server` TCP/UDP listeners), instead of
/// running the local TUI.
async fn run_server_mode() {
    let args = parse_server_args();

    let port = SerialPort::open(
        &args.port,
        SerialConfig {
            baud_rate: args.baud,
            stop_bits: args.stop_bits,
            // Never assert DTR at open — see the TUI-mode open above:
            // DTR may be wired as a PTT key line. (planning/ptt-line)
            initial_dtr: false,
            ..SerialConfig::default()
        },
    )
    .expect("serial open failed");

    info!(
        "Serial port opened (server mode): {} @ {} baud {} stop bit(s)",
        args.port, args.baud, args.stop_bits
    );

    let session = SerialCatSession::new(port);
    let config = server::ServerConfig {
        raw_tcp_port: args.raw_tcp_port,
        raw_udp_port: args.raw_udp_port,
        rigctl_port: args.rigctl_port,
    };

    // `server::run` is `async fn` on Linux and a plain blocking `fn` on
    // Windows (matching `cat_rigctl::run`'s own per-platform split, since
    // `#[monoio::main]` cannot exist there -- see `server::run`'s own doc
    // comment). Both variants share the same name and now support the same
    // full feature set (including `--rigctl-port`), so only this call
    // site's `.await` needs to differ.
    #[cfg(target_os = "linux")]
    let result = server::run(session, config).await;
    #[cfg(target_os = "windows")]
    let result = server::run(session, config);

    if let Err(e) = result {
        eprintln!("Server error: {e}");
        std::process::exit(1);
    }
}

/// Shared application entry point, driven to completion by `#[monoio::main]`
/// on Linux and by `win_runtime::block_on` on Windows (see
/// `docs/adr/0006-windows-concurrency-model.md`). Everything below this
/// point (argument parsing, opening the chosen transport, `ui::run`) is
/// identical on both platforms; only the executor driving this future
/// differs.
async fn run_app() {
    // 1. Initialize logging — use RUST_LOG env var to control verbosity.
    tracing_subscriber::fmt().with_env_filter("info").init();

    info!("Starting TS-570D Radio Control Application");

    // `ts570d server ...` branches off entirely before the direct/TUI
    // argument parsing below -- it has its own flag set and never opens a
    // `ui::run` session.
    if std::env::args().nth(1).as_deref() == Some("server") {
        run_server_mode().await;
        return;
    }

    // 2. Parse CLI arguments.
    let args = parse_args();

    // 3. Open the chosen transport, then wrap it in the typed TS-570D client.
    let result = match args.transport {
        Transport::Serial {
            port,
            baud,
            stop_bits,
        } => {
            // SerialPort::open must be called inside an active monoio
            // runtime on Linux because it registers the fd with io_uring;
            // on Windows it is a plain synchronous call (see
            // radio-cat-rs's docs/adr/0004-windows-serial-backend.md).
            let serial = SerialPort::open(
                &port,
                SerialConfig {
                    baud_rate: baud,
                    stop_bits,
                    // Never assert DTR at open: on stations keying PTT from
                    // the DTR line (e.g. an ACC2 opto interface), the default
                    // `initial_dtr: true` keys the transmitter the moment the
                    // port opens. CAT itself needs no DTR. (planning/ptt-line)
                    initial_dtr: false,
                    ..SerialConfig::default()
                },
            )
            .expect("serial open failed");

            info!(
                "Serial port opened: {} @ {} baud {} stop bit(s)",
                port, baud, stop_bits
            );

            let radio = Ts570d::new(SerialCatSession::new(serial));

            // `ui::run` is `async fn` on Linux (driven by `#[monoio::main]`)
            // but a plain synchronous `fn` on Windows (its own internal
            // two-task scheduler already blocks the calling thread to
            // completion — see docs/adr/0006-windows-concurrency-model.md).
            #[cfg(target_os = "linux")]
            let ui_result = ui::run(radio).await;
            #[cfg(target_os = "windows")]
            let ui_result = ui::run(radio);

            ui_result
        }
        Transport::Server { addr } => {
            let tcp_session = TcpCatSession::connect(&addr).await.unwrap_or_else(|e| {
                eprintln!("error: could not connect to {addr}: {e}");
                std::process::exit(1);
            });

            info!("Connected to remote ts570d server at {}", addr);

            let radio = Ts570d::new(TcpClientSession::new(tcp_session));

            #[cfg(target_os = "linux")]
            let ui_result = ui::run(radio).await;
            #[cfg(target_os = "windows")]
            let ui_result = ui::run(radio);

            ui_result
        }
    };

    // 4. Report any UI-level error.
    if let Err(e) = result {
        eprintln!("UI error: {}", e);
        std::process::exit(1);
    }

    info!("Application stopped");
}

/// Linux entry point. Uses monoio's io_uring runtime (single-threaded, !Send).
#[cfg(target_os = "linux")]
#[monoio::main(timer_enabled = true)]
async fn main() {
    run_app().await;
}

/// Windows entry point. `#[monoio::main]` cannot exist on Windows (`monoio`
/// requires io_uring, a Linux kernel interface) -- `win_runtime::block_on`
/// drives the same `run_app()` future instead. See
/// `docs/adr/0006-windows-concurrency-model.md`.
#[cfg(target_os = "windows")]
fn main() {
    win_runtime::block_on(run_app());
}
