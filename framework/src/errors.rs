//! Framework Error Types
//!
//! Defines minimal error types for the framework crate.

use thiserror::Error;

/// Errors that can occur in framework operations
#[derive(Error, Debug)]
pub enum FrameworkError {

    /// Invalid message format
    #[error("Invalid message format: {0}")]
    InvalidMessage(String),

    /// Message serialization error
    #[error("Message serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Channel send error
    #[error("Channel send failed")]
    ChannelSend,

    /// Channel receive error
    #[error("Channel receive failed")]
    ChannelReceive,
}

/// Result type for framework operations
pub type FrameworkResult<T> = Result<T, FrameworkError>;
