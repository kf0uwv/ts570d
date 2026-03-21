pub mod pty;

#[derive(Debug, thiserror::Error)]
pub enum EmulatorError {
    #[error("PTY error: {0}")]
    Pty(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
