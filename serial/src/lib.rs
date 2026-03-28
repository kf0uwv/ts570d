// Copyright 2026 Matt Franklin
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

//! Serial communication crate for TS-570D
//!
//! Provides custom io_uring-based RS-232 implementation for Linux systems.
//! This crate handles low-level serial communication using io_uring for
//! high-performance asynchronous operations.

pub mod io_uring;

pub use io_uring::{FlowControl, Parity, SerialConfig, SerialPort};

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
