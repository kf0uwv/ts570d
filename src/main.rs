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

/// Print usage and exit with code 1.
fn usage_exit() -> ! {
    eprintln!(
        "Usage: ts570d --port <serial-port-path>\n\
         \n\
         Examples:\n\
           ts570d --port /dev/pts/5       # connect to running emulator\n\
           ts570d --port /dev/ttyUSB0     # connect to physical TS-570D"
    );
    std::process::exit(1);
}

/// Parse `--port <path>` from `std::env::args()`.
/// Returns the port path, or calls `usage_exit()` if missing.
fn parse_port_arg() -> String {
    let mut args = std::env::args().skip(1);
    loop {
        match args.next().as_deref() {
            Some("--port") => match args.next() {
                Some(path) => return path,
                None => usage_exit(),
            },
            Some(_) => {}
            None => usage_exit(),
        }
    }
}

/// Entry point.  Uses monoio's io_uring runtime (single-threaded, !Send).
#[monoio::main(timer_enabled = true)]
async fn main() {
    // 1. Initialize logging — use RUST_LOG env var to control verbosity.
    tracing_subscriber::fmt().with_env_filter("info").init();

    info!("Starting TS-570D Radio Control Application");

    // 2. Parse --port argument.
    let port_path = parse_port_arg();

    // 3. Open the port via the io_uring serial driver.
    //    SerialPort::open must be called inside an active monoio runtime
    //    because it registers the fd with io_uring.
    //    TS-570D communicates at 4800 baud (8N2 per radio spec).
    let port = SerialPort::open(
        &port_path,
        SerialConfig {
            baud_rate: 4800,
            ..SerialConfig::default()
        },
    )
    .expect("serial open failed");

    info!("Serial port opened: {}", port_path);

    // 4. Wrap in the typed TS-570D client.
    let mut radio = Ts570d::new(port);

    // 5. Run the radio + UI event loop.
    if let Err(e) = ui::run(&mut radio).await {
        eprintln!("UI error: {}", e);
        std::process::exit(1);
    }

    info!("Application stopped");
}
