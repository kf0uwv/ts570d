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
