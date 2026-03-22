//! Framework Error Types
//!
//! Defines minimal error types for the framework crate.

use thiserror::Error;

/// Errors that can occur in framework operations
#[derive(Error, Debug)]
pub enum FrameworkError {
    /// Invalid message format or state transition
    #[error("Invalid message: {0}")]
    InvalidMessage(String),
}

/// Result type for framework operations
pub type FrameworkResult<T> = Result<T, FrameworkError>;

/// Errors from Transport implementations (serial port I/O).
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Port not open")]
    NotOpen,
    #[error("Write timeout")]
    WriteTimeout,
    #[error("Read timeout")]
    ReadTimeout,
    #[error("Transport error: {0}")]
    Other(String),
}
