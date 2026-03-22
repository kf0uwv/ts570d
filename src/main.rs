//! TS-570D Radio Control Application
//!
//! Main entry point: starts the emulator in a background OS thread, then
//! opens the PTY slave via the io_uring serial driver, creates a typed
//! Ts570d client, and runs a ratatui UI that polls radio state every 200 ms.

use crossterm::event::{self, Event, KeyCode};
use tracing::info;

use radio::Ts570d;
use serial::SerialPort;
use ui::{RadioDisplay, UiError};

/// Entry point.  Uses monoio's io_uring runtime (single-threaded, !Send).
#[monoio::main(timer_enabled = true)]
async fn main() {
    // Initialize logging — use RUST_LOG env var to control verbosity.
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    info!("Starting TS-570D Radio Control Application");

    // -----------------------------------------------------------------------
    // 1. Start emulator in a dedicated OS thread (blocking I/O loop).
    // -----------------------------------------------------------------------
    let mut emulator = emulator::emulator::Emulator::new()
        .expect("emulator init failed");
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
    let port = SerialPort::open(&slave_path, 9600)
        .expect("serial open failed");

    info!("Serial port opened: {}", slave_path);

    // -----------------------------------------------------------------------
    // 4. Wrap in the typed TS-570D client.
    // -----------------------------------------------------------------------
    let mut radio = Ts570d::new(port);

    // -----------------------------------------------------------------------
    // 5. Run the radio + UI event loop.
    // -----------------------------------------------------------------------
    if let Err(e) = run_radio_ui(&mut radio).await {
        eprintln!("UI error: {}", e);
        std::process::exit(1);
    }

    info!("Application stopped");
}

/// Combined radio polling + UI render loop.
///
/// - Polls the radio every 200 ms for VFO A frequency, mode, and S-meter.
/// - Draws the ratatui UI after each poll.
/// - Exits immediately when 'q' is pressed.
///
/// Terminal setup / teardown is handled here so that the cleanup path runs
/// even if any radio query returns an error.
async fn run_radio_ui<T: framework::transport::Transport>(
    radio: &mut Ts570d<T>,
) -> Result<(), UiError> {
    let mut terminal = ui::init_terminal()?;
    let mut state = RadioDisplay::default();

    let result = radio_ui_loop(radio, &mut terminal, &mut state).await;

    // Always restore the terminal, regardless of how the loop ended.
    ui::cleanup_terminal()?;
    result
}

/// Inner loop — separated so cleanup runs on all exit paths.
async fn radio_ui_loop<T: framework::transport::Transport>(
    radio: &mut Ts570d<T>,
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    state: &mut RadioDisplay,
) -> Result<(), UiError> {
    loop {
        // -- Poll radio state ------------------------------------------------
        // Each query sends a CAT command and reads back the response.
        // Errors are soft — we keep whatever values we had on failure.
        if let Ok(freq) = radio.get_vfo_a().await {
            state.vfo_a_hz = freq.hz();
        }
        if let Ok(mode) = radio.get_mode().await {
            state.mode = mode.name().to_string();
        }
        if let Ok(smeter) = radio.get_smeter().await {
            state.smeter = smeter;
        }

        // -- Render the UI ---------------------------------------------------
        ui::draw_frame(terminal, state)?;

        // -- Wait 200 ms, checking for 'q' every ~10 ms ---------------------
        //
        // crossterm::event::poll(Duration::ZERO) is synchronous and
        // non-blocking, safe to call from inside a monoio task.
        let mut elapsed = std::time::Duration::ZERO;
        let poll_interval = std::time::Duration::from_millis(200);
        let check_step = std::time::Duration::from_millis(10);

        while elapsed < poll_interval {
            // Non-blocking check for keyboard input.
            if event::poll(std::time::Duration::ZERO).map_err(UiError::Io)? {
                if let Event::Key(key) = event::read().map_err(UiError::Io)? {
                    if key.code == KeyCode::Char('q') {
                        return Ok(());
                    }
                }
            }
            monoio::time::sleep(check_step).await;
            elapsed += check_step;
        }
    }
}
