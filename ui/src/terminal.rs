use std::io::{self, Stdout};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use framework::radio::Radio;
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{layout::draw_ui, RadioDisplay, UiError, UiResult};

/// Initialize the terminal: enable raw mode and enter the alternate screen.
/// The caller is responsible for calling `cleanup_terminal()` when done.
pub(crate) fn init_terminal() -> UiResult<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Restore the terminal to its normal state (disable raw mode, leave alternate screen).
pub(crate) fn cleanup_terminal() -> UiResult<()> {
    disable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, LeaveAlternateScreen)?;
    Ok(())
}

/// Draw a single frame using the given radio state.
pub(crate) fn draw_frame(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &RadioDisplay,
) -> UiResult<()> {
    terminal.draw(|f| draw_ui(f, state))?;
    Ok(())
}

/// Run the radio UI — polls radio state and renders until 'q' is pressed.
///
/// Handles terminal setup/teardown. Polls `radio` every 200 ms for
/// VFO A frequency, mode, and S-meter reading.
pub async fn run<R: Radio>(radio: &mut R) -> UiResult<()> {
    let mut terminal = init_terminal()?;
    let mut state = RadioDisplay::default();
    let result = run_radio_loop(radio, &mut terminal, &mut state).await;
    cleanup_terminal()?;
    result
}

async fn run_radio_loop<R: Radio>(
    radio: &mut R,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut RadioDisplay,
) -> UiResult<()> {
    loop {
        // Poll radio state — soft errors keep previous values
        if let Ok(freq) = radio.get_vfo_a().await {
            state.vfo_a_hz = freq.hz();
        }
        if let Ok(mode) = radio.get_mode().await {
            state.mode = mode.name().to_string();
        }
        if let Ok(smeter) = radio.get_smeter().await {
            state.smeter = smeter;
        }

        draw_frame(terminal, state)?;

        // Wait 200 ms, checking for 'q' every ~10 ms
        let mut elapsed = std::time::Duration::ZERO;
        let poll_interval = std::time::Duration::from_millis(200);
        let check_step = std::time::Duration::from_millis(10);

        while elapsed < poll_interval {
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
