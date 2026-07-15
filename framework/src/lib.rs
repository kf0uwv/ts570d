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

//! Generic CAT Framework
//!
//! Radio-independent infrastructure shared across workspace crates.
//!
//! This framework provides:
//! - **Generic CAT engine** (`cat`) - command table, parsing, structural
//!   validation, dispatch lifecycle, and response building, generic over a
//!   radio-defined command identifier
//! - **Transport trait** - byte-level I/O interface decoupling radio from serial
//! - **Error types** - generic framework and transport errors
//! - **State machine** - generic application state management
//!
//! The framework contains **no radio-specific** command definitions, modes,
//! frequencies, state, or handlers. Those live in radio crates such as `radio`
//! (TS-570D), which implement the [`cat::CatRadio`] trait.

// Framework modules
pub mod cat;
pub mod errors;
pub mod state_machine;
pub mod transport;

// Re-export main framework components
pub use cat::{
    CatCommandCatalog, CatFramework, CatFrameworkError, CatRadio, CommandDefinition, CommandForm,
    CommandId, CommandOperation, CommandOutcome, CommandRequest, CommandTable, ParameterValues,
    ParseError, ProtocolErrorKind, ResponseBuildError, ResponseBuilder, ResponseDisposition,
};
pub use errors::{FrameworkError, FrameworkResult, TransportError};
pub use state_machine::{ApplicationStateMachine, State};
pub use transport::Transport;

// Re-export monoio runtime
pub use monoio::RuntimeBuilder;

// Re-export commonly used std types for convenience
pub use std::pin::Pin;
pub use std::sync::Arc;

// Re-export monoio async I/O traits
pub use monoio::io::{AsyncReadRent, AsyncWriteRent};
