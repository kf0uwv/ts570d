use std::collections::VecDeque;
use std::io::stdout;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute, terminal,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use serialport::SerialPort;

use crate::commands;
use crate::io::EmulatorIo;
use crate::logger::{now_ms, BackgroundLogger, LogEvent};
use crate::pty::PtyPair;
use crate::radio_state::RadioState;
use crate::tui;
use crate::EmulatorError;

/// Maximum number of log entries kept for the TUI command panel.
const LOG_LIMIT: usize = 40;

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
    /// Rolling command/response log for the TUI command panel.
    log: VecDeque<String>,
}

impl Emulator {
    /// Create a new emulator.  The slave PTY path is printed to stdout so
    /// that external processes can discover where to connect.
    pub fn new() -> Result<Self, EmulatorError> {
        let mut pty = PtyPair::new()?;
        let slave_path = pty.slave_path().to_string();
        println!("PTY_SLAVE={}", slave_path);
        // Take the master port out of PtyPair for use by EmulatorIo.
        let master = pty.take_master();
        let io = EmulatorIo::from_port(master);
        let state = RadioState::default();
        Ok(Emulator {
            _pty: Some(pty),
            slave_path,
            io,
            state,
            log: VecDeque::new(),
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
            log: VecDeque::new(),
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
                        let (response, _changes) = commands::handle(&cmd, &mut self.state);
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

    /// Run the background event loop, emitting NDJSON events to `logger`.
    ///
    /// Emits a `startup` event immediately, then a `command` event and
    /// zero or more `state_change` events for every CAT command received.
    pub fn run_background(&mut self, mut logger: BackgroundLogger) -> Result<(), EmulatorError> {
        // Emit startup event.
        let startup = LogEvent::Startup {
            ts: now_ms(),
            port: &self.slave_path.clone(),
            mode: "background",
        };
        logger.log_event(&startup);

        loop {
            match self.io.read_commands() {
                Ok(cmds) => {
                    for cmd in cmds {
                        let (response, changes) = commands::handle(&cmd, &mut self.state);
                        self.io.write_response(&response)?;

                        // Log the raw command + response.
                        let cmd_event = LogEvent::Command {
                            ts: now_ms(),
                            raw: &format!("{};", cmd),
                            response: &response,
                        };
                        logger.log_event(&cmd_event);

                        // Log each state change.
                        for change in &changes {
                            let sc_event = LogEvent::StateChange {
                                ts: now_ms(),
                                field: change.field,
                                value: &change.value,
                            };
                            logger.log_event(&sc_event);
                        }
                    }
                }
                Err(EmulatorError::Io(ref e))
                    if e.kind() == std::io::ErrorKind::TimedOut
                        || e.kind() == std::io::ErrorKind::WouldBlock =>
                {
                    continue;
                }
                Err(EmulatorError::Io(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof
                        || e.kind() == std::io::ErrorKind::BrokenPipe =>
                {
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

    /// Append a command/response pair to the rolling TUI log.
    fn log_entry(&mut self, cmd: &str, response: &str) {
        self.log.push_back(format!("→ {};", cmd));
        self.log.push_back(format!("← {}", response));
        // Keep the log bounded at LOG_LIMIT entries (pairs of 2).
        while self.log.len() > LOG_LIMIT {
            self.log.pop_front();
        }
    }

    fn tui_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<(), EmulatorError> {
        loop {
            // 1. Draw the current state.
            let slave_path = self.slave_path.clone();
            let log_slice: Vec<String> = self.log.iter().cloned().collect();
            terminal.draw(|f| tui::draw(f, &self.state, &slave_path, &log_slice))?;

            // 2. Poll for keyboard events (non-blocking, 10 ms window).
            if event::poll(Duration::from_millis(10))? {
                if let Event::Key(key) = event::read()? {
                    let quit = match key.code {
                        // q or Q → clean shutdown.
                        KeyCode::Char('q') | KeyCode::Char('Q') => true,
                        // Ctrl+C → clean shutdown.
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => true,
                        _ => false,
                    };
                    if quit {
                        let _ = terminal::disable_raw_mode();
                        let _ = execute!(stdout(), terminal::LeaveAlternateScreen);
                        std::process::exit(0);
                    }
                }
            }

            // 3. Read incoming CAT commands (non-blocking poll).
            match self.io.read_commands() {
                Ok(cmds) => {
                    // 4. Handle each command, updating state.
                    for cmd in cmds {
                        let (response, _changes) = commands::handle(&cmd, &mut self.state);
                        // 5. Append to command log.
                        self.log_entry(&cmd, &response);
                        // 6. Write response.
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
