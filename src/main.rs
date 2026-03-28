// Copyright 2024 Matt Franklin
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
//! Main entry point: parse --port <path> from argv, open the io_uring serial
//! driver on that path, create a typed Ts570d client, and run the ratatui UI.
//!
//! The emulator (if needed) runs as a **separate process**:
//!   cargo run --bin emulator
//! It prints the PTY slave path to stdout; pass that path here via --port.

use tracing::info;

use radio::Ts570d;
use serial::{SerialConfig, SerialPort};

/// Parsed command-line arguments.
struct Args {
    port: String,
    baud: u32,
    stop_bits: u8,
}

/// Print usage and exit with code 1.
fn usage_exit() -> ! {
    eprintln!(
        "Usage: ts570d --port <serial-port-path> [--baud <rate>] [--stop-bits <n>]\n\
         \n\
           --port      Serial port path (required)\n\
                       Examples: /dev/pts/5  /dev/ttyUSB0\n\
           --baud      Baud rate: 1200, 2400, 4800, 9600  (default: 9600)\n\
           --stop-bits Stop bits: 1 or 2                  (default: 1)"
    );
    std::process::exit(1);
}

/// Parse `--port <path>`, `--baud <rate>`, and `--stop-bits <n>` from
/// `std::env::args()`.  Unknown flags are silently ignored.  Exits with an
/// error message and code 1 for missing or invalid values.
fn parse_args() -> Args {
    let mut args_iter = std::env::args().skip(1);
    let mut port: Option<String> = None;
    let mut baud: u32 = 9600;
    let mut stop_bits: u8 = 1;

    loop {
        match args_iter.next().as_deref() {
            Some("--port") => match args_iter.next() {
                Some(path) => port = Some(path),
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

    match port {
        Some(p) => Args {
            port: p,
            baud,
            stop_bits,
        },
        None => usage_exit(),
    }
}

/// Entry point.  Uses monoio's io_uring runtime (single-threaded, !Send).
#[monoio::main(timer_enabled = true)]
async fn main() {
    // 1. Initialize logging — use RUST_LOG env var to control verbosity.
    tracing_subscriber::fmt().with_env_filter("info").init();

    info!("Starting TS-570D Radio Control Application");

    // 2. Parse CLI arguments.
    let args = parse_args();

    // 3. Open the port via the io_uring serial driver.
    //    SerialPort::open must be called inside an active monoio runtime
    //    because it registers the fd with io_uring.
    let port = SerialPort::open(
        &args.port,
        SerialConfig {
            baud_rate: args.baud,
            stop_bits: args.stop_bits,
            ..SerialConfig::default()
        },
    )
    .expect("serial open failed");

    info!(
        "Serial port opened: {} @ {} baud {} stop bit(s)",
        args.port, args.baud, args.stop_bits
    );

    // 4. Wrap in the typed TS-570D client.
    let radio = Ts570d::new(port);

    // 5. Run the radio + UI event loop.
    if let Err(e) = ui::run(radio).await {
        eprintln!("UI error: {}", e);
        std::process::exit(1);
    }

    info!("Application stopped");
}
