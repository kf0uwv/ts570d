//! TS-570D Radio Control Application
//!
//! Main entry point: starts the emulator in a background OS thread, then
//! opens the PTY slave via the io_uring serial driver, creates a typed
//! Ts570d client, and runs the ratatui UI that polls radio state every 200 ms.

use tracing::info;

use radio::Ts570d;
use serial::{SerialConfig, SerialPort};

/// Entry point.  Uses monoio's io_uring runtime (single-threaded, !Send).
#[monoio::main(timer_enabled = true)]
async fn main() {
    // Initialize logging — use RUST_LOG env var to control verbosity.
    tracing_subscriber::fmt().with_env_filter("info").init();

    info!("Starting TS-570D Radio Control Application");

    // -----------------------------------------------------------------------
    // 1. Start emulator in a dedicated OS thread (blocking I/O loop).
    // -----------------------------------------------------------------------
    let mut emulator = emulator::emulator::Emulator::new().expect("emulator init failed");
    let slave_path = emulator.slave_path().to_string();

    std::thread::spawn(move || {
        emulator.run().expect("emulator run failed");
    });

    // -----------------------------------------------------------------------
    // 2. Brief pause so the emulator's read loop is ready before we connect.
    // -----------------------------------------------------------------------
    monoio::time::sleep(std::time::Duration::from_millis(100)).await;

    // -----------------------------------------------------------------------
    // 3. Open the PTY slave via the io_uring serial driver.
    //
    //    SerialPort::open must be called inside an active monoio runtime
    //    because UnixStream::from_std registers the fd with io_uring.
    // -----------------------------------------------------------------------
    let port = SerialPort::open(
        &slave_path,
        SerialConfig {
            baud_rate: 9600,
            ..SerialConfig::default()
        },
    )
    .expect("serial open failed");

    info!("Serial port opened: {}", slave_path);

    // -----------------------------------------------------------------------
    // 4. Wrap in the typed TS-570D client.
    // -----------------------------------------------------------------------
    let mut radio = Ts570d::new(port);

    // -----------------------------------------------------------------------
    // 5. Run the radio + UI event loop.
    // -----------------------------------------------------------------------
    if let Err(e) = ui::run(&mut radio).await {
        eprintln!("UI error: {}", e);
        std::process::exit(1);
    }

    info!("Application stopped");
}
