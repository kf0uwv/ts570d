use std::io::stdout;

use crossterm::{execute, terminal};
use ratatui::{backend::CrosstermBackend, Terminal};
use serialport::SerialPort;

use crate::commands;
use crate::io::EmulatorIo;
use crate::pty::PtyPair;
use crate::radio_state::RadioState;
use crate::tui;
use crate::EmulatorError;

/// The TS-570D emulator.
///
/// Creates a PTY pair, serves as the radio on the master side, and exposes
/// the slave path so clients (tests, the main application) can connect.
pub struct Emulator {
    /// Held to keep the slave PTY alive (prevents EIO on the master).
    /// `None` when the emulator was created with a physical serial port.
    _pty: Option<PtyPair>,
    /// Cached slave device path.
    slave_path: String,
    io: EmulatorIo,
    state: RadioState,
}

impl Emulator {
    /// Create a new emulator.  The slave PTY path is printed to stdout so
    /// that external processes can discover where to connect.
    pub fn new() -> Result<Self, EmulatorError> {
        let mut pty = PtyPair::new()?;
        let slave_path = pty.slave_path().to_string();
        println!("Emulator listening on: {}", slave_path);
        // Take the master port out of PtyPair for use by EmulatorIo.
        let master = pty.take_master();
        let io = EmulatorIo::from_port(master);
        let state = RadioState::default();
        Ok(Emulator {
            _pty: Some(pty),
            slave_path,
            io,
            state,
        })
    }

    /// Create an emulator from an already-opened port (virtual PTY master or physical serial).
    ///
    /// `slave_path` should be `Some(path)` for virtual PTY mode and `None` for physical mode.
    /// The caller is responsible for printing status before calling this.
    pub fn from_port(port: Box<dyn SerialPort>, slave_path: String) -> Self {
        let io = EmulatorIo::from_port(port);
        let state = RadioState::default();
        Emulator {
            _pty: None,
            slave_path,
            io,
            state,
        }
    }

    /// Return the slave PTY path (e.g. `/dev/pts/5`).
    pub fn slave_path(&self) -> &str {
        &self.slave_path
    }

    /// Run the blocking event loop.
    ///
    /// Reads commands from the PTY master, dispatches them through
    /// `CommandHandler`, and writes responses back.  Runs until an I/O error
    /// (e.g. last slave fd is closed) or the process receives SIGINT.
    pub fn run(&mut self) -> Result<(), EmulatorError> {
        loop {
            match self.io.read_commands() {
                Ok(cmds) => {
                    for cmd in cmds {
                        let response = commands::handle(&cmd, &mut self.state);
                        self.io.write_response(&response)?;
                    }
                }
                Err(EmulatorError::Io(ref e))
                    if e.kind() == std::io::ErrorKind::TimedOut
                        || e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    // No data yet — retry.
                    continue;
                }
                Err(EmulatorError::Io(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof
                        || e.kind() == std::io::ErrorKind::BrokenPipe =>
                {
                    // Client disconnected — normal shutdown.
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Run the event loop with a Ratatui LCD-style TUI overlay.
    ///
    /// Enters crossterm alternate screen and raw mode, then runs the same
    /// read/handle/write loop as `run()`, redrawing the display after each
    /// batch of commands.  Terminal state is restored on exit.
    pub fn run_with_tui(&mut self) -> Result<(), EmulatorError> {
        // Enter raw mode and alternate screen.
        terminal::enable_raw_mode()?;
        let mut out = stdout();
        execute!(out, terminal::EnterAlternateScreen)?;

        let backend = CrosstermBackend::new(stdout());
        let mut terminal = Terminal::new(backend)?;

        let result = self.tui_loop(&mut terminal);

        // Always restore terminal even if an error occurred.
        let _ = terminal::disable_raw_mode();
        let _ = execute!(stdout(), terminal::LeaveAlternateScreen);

        result
    }

    fn tui_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<(), EmulatorError> {
        loop {
            // 1. Draw the current state.
            terminal.draw(|f| tui::draw(f, &self.state))?;

            // 2. Read incoming CAT commands (blocking with timeout).
            match self.io.read_commands() {
                Ok(cmds) => {
                    // 3. Handle each command, updating state.
                    for cmd in cmds {
                        let response = commands::handle(&cmd, &mut self.state);
                        // 4. Write response.
                        self.io.write_response(&response)?;
                    }
                }
                Err(EmulatorError::Io(ref e))
                    if e.kind() == std::io::ErrorKind::TimedOut
                        || e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    // No data — just redraw.
                    continue;
                }
                Err(EmulatorError::Io(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof
                        || e.kind() == std::io::ErrorKind::BrokenPipe =>
                {
                    // Client disconnected — clean exit.
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}
