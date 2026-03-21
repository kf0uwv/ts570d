use std::os::fd::{AsRawFd, OwnedFd};

use nix::pty::{openpty, OpenptyResult};
use nix::unistd::ttyname;

use crate::EmulatorError;

/// A Linux pseudo-terminal pair.
/// The master fd is used by the emulator for I/O.
/// The slave path (/dev/pts/N) is exposed to connecting applications.
pub struct PtyPair {
    pub master: OwnedFd,
    pub slave_path: String,
}

impl PtyPair {
    /// Create a new PTY pair.
    pub fn new() -> Result<Self, EmulatorError> {
        let OpenptyResult { master, slave } =
            openpty(None, None).map_err(|e| EmulatorError::Pty(e.to_string()))?;

        let slave_path = ttyname(slave.as_raw_fd())
            .map_err(|e| EmulatorError::Pty(e.to_string()))?
            .to_string_lossy()
            .into_owned();

        drop(slave);

        Ok(PtyPair { master, slave_path })
    }

    /// Get the slave device path (e.g., "/dev/pts/5")
    pub fn slave_path(&self) -> &str {
        &self.slave_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_creation() {
        let pair = PtyPair::new().expect("PTY creation failed");
        assert!(
            pair.slave_path().starts_with("/dev/pts/"),
            "Expected /dev/pts/N path, got: {}",
            pair.slave_path()
        );
        assert!(
            std::path::Path::new(pair.slave_path()).exists(),
            "Slave path does not exist: {}",
            pair.slave_path()
        );
    }
}
