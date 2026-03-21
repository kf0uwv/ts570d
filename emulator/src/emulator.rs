use crate::commands;
use crate::io::EmulatorIo;
use crate::pty::PtyPair;
use crate::radio_state::RadioState;
use crate::EmulatorError;

/// The TS-570D emulator.
///
/// Creates a PTY pair, serves as the radio on the master side, and exposes
/// the slave path so clients (tests, the main application) can connect.
pub struct Emulator {
    /// Held to keep the slave PTY alive (prevents EIO on the master).
    _pty: PtyPair,
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
            _pty: pty,
            slave_path,
            io,
            state,
        })
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
                    if e.kind() == std::io::ErrorKind::UnexpectedEof
                        || e.kind() == std::io::ErrorKind::BrokenPipe =>
                {
                    // Client disconnected — normal shutdown path.
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}
