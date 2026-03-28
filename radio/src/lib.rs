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

//! Kenwood TS-570D CAT Protocol Implementation
//!
//! This crate provides a complete implementation of the Kenwood TS-570D
//! Computer Aided Transceiver (CAT) protocol. The implementation is data-driven,
//! with a comprehensive command table that defines all supported commands and
//! their metadata.
//!
//! # Architecture
//!
//! - `client`: Generic `RadioClient<T: Transport>` for sending commands and reading responses
//! - `commands`: Command table with metadata for all TS-570D CAT commands
//! - `error`: Error types for protocol operations
//! - `protocol`: Core protocol types (`Mode`, `Frequency`)
//!
//! # Usage
//!
//! ```no_run
//! use radio::commands::CommandMetadata;
//!
//! // Look up command metadata
//! let fa_cmd = CommandMetadata::find("FA").unwrap();
//! assert!(fa_cmd.supports_read);
//! assert!(fa_cmd.supports_write);
//! ```

pub mod client;
pub mod commands;
pub mod protocol;
pub mod ts570d;

pub use client::RadioClient;
pub use protocol::{Response, ResponseFramer, ResponseParser};
pub use ts570d::Ts570d;
