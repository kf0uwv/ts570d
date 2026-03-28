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

//! TS-570D Framework
//!
//! Shared infrastructure for all workspace crates.
//!
//! This framework provides:
//! - **State machine** - Application state management and transitions
//! - **Error types** - Common error handling
//! - **Transport trait** - Byte-level I/O interface decoupling radio from serial
//! - **Radio trait** - Typed radio command abstraction

// Framework modules
pub mod errors;
pub mod radio;
pub mod state_machine;
pub mod transport;

// Re-export main framework components
pub use errors::{FrameworkError, FrameworkResult, TransportError};
pub use radio::{Frequency, InformationResponse, Mode, NopRadio, Radio, RadioError, RadioResult};
pub use state_machine::{ApplicationStateMachine, State};
pub use transport::Transport;

// Re-export monoio runtime
pub use monoio::RuntimeBuilder;

// Re-export commonly used std types for convenience
pub use std::pin::Pin;
pub use std::sync::Arc;

// Re-export monoio async I/O traits
pub use monoio::io::{AsyncReadRent, AsyncWriteRent};
