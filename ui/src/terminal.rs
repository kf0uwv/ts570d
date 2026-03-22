use std::io::{self, Stdout};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{layout::draw_ui, RadioDisplay, UiError, UiResult};

/// Initialize the terminal: enable raw mode and enter the alternate screen.
/// The caller is responsible for calling `cleanup_terminal()` when done.
pub fn init_terminal() -> UiResult<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Restore the terminal to its normal state (disable raw mode, leave alternate screen).
pub fn cleanup_terminal() -> UiResult<()> {
    disable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, LeaveAlternateScreen)?;
    Ok(())
}

/// Draw a single frame using the given radio state.
pub fn draw_frame(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &RadioDisplay,
) -> UiResult<()> {
    terminal.draw(|f| draw_ui(f, state))?;
    Ok(())
}

/// Run the UI event loop using a static default radio state.
/// Useful for standalone demos and development.
pub fn run_ui() -> Result<(), UiError> {
    let mut terminal = init_terminal()?;
    let state = RadioDisplay::default();

    let result = run_loop(&mut terminal, &state);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &RadioDisplay,
) -> Result<(), UiError> {
    loop {
        draw_frame(terminal, state)?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }
    Ok(())
}
