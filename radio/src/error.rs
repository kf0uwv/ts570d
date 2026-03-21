use thiserror::Error;

#[derive(Debug, Error)]
pub enum RadioError {
    #[error("Invalid mode: {0}")]
    InvalidMode(u8),
    #[error("Frequency out of range: {0} Hz (valid: 500000–60000000)")]
    FrequencyOutOfRange(u64),
    #[error("Invalid protocol string: {0}")]
    InvalidProtocolString(String),
}

pub type RadioResult<T> = Result<T, RadioError>;
