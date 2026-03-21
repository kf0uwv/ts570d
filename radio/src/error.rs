use framework::errors::TransportError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RadioError {
    #[error("Invalid mode: {0}")]
    InvalidMode(u8),
    #[error("Frequency out of range: {0} Hz (valid: 500000–60000000)")]
    FrequencyOutOfRange(u64),
    #[error("Invalid protocol string: {0}")]
    InvalidProtocolString(String),
    #[error("Unknown command code: {0}")]
    UnknownCommand(String),
    #[error("Command {0} does not support read (query)")]
    CommandNotReadable(String),
    #[error("Command {0} does not support write (set)")]
    CommandNotWritable(String),
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),
}

pub type RadioResult<T> = Result<T, RadioError>;
