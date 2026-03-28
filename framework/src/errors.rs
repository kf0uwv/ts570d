// Copyright 2024 Matt Franklin
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

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
