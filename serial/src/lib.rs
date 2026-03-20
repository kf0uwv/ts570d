//! Serial communication crate for TS-570D
//! 
//! Provides custom io_uring-based RS-232 implementation for Linux systems.
//! This crate handles low-level serial communication using io_uring for
//! high-performance asynchronous operations.

pub mod io_uring;

// Re-export will be added when proper implementation is complete
// pub use io_uring::*;

/// Serial communication errors
#[derive(thiserror::Error, Debug)]
pub enum SerialError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    
    #[error("Device not found: {0}")]
    DeviceNotFound(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

/// Result type for serial operations
pub type SerialResult<T> = Result<T, SerialError>;