//! TS-570D Framework
//!
//! Shared infrastructure for all workspace crates.
//!
//! This framework provides:
//! - **Message types** - Common message enums for inter-component communication
//! - **Channels** - monoio-based channel helpers for passing messages
//! - **State machine** - Application state management and transitions
//! - **Error types** - Common error handling
//! - **Transport trait** - Byte-level I/O interface decoupling radio from serial
//!
//! ## Architecture
//!
//! The framework uses a simple channel-based communication pattern:
//!
//! 1. Components communicate via monoio channels (see `channels` module)
//! 2. Messages are defined in the `messages` module and can be extended
//! 3. State management is handled by the `state_machine` module
//!
//! ## monoio Considerations
//!
//! This framework is designed for use with monoio, an io_uring-based async runtime.
//! Unlike tokio, monoio uses a thread-per-core model where futures are **!Send**
//! (not thread-safe). This means:
//!
//! - Channel types cannot be sent between threads
//! - No `Send + Sync` bounds on traits or types
//! - Each component runs on a single thread
//!
//! ## Example Usage
//!
//! ```rust,ignore
//! use framework::{
//!     channels::create_message_channel,
//!     Message, MessageTypes, SystemMessage,
//!     state_machine::{ApplicationStateMachine, State},
//! };
//!
//! #[monoio::main]
//! async fn main() {
//!     // Create channels for communication
//!     let (ui_tx, mut ui_rx) = create_message_channel();
//!     let (serial_tx, mut serial_rx) = create_message_channel();
//!
//!     // Initialize state machine
//!     let mut state_machine = ApplicationStateMachine::new();
//!     state_machine.initialize().unwrap();
//!
//!     // Send a message
//!     let msg = Message::new(
//!         "1".to_string(),
//!         "app".to_string(),
//!         "ui".to_string(),
//!         MessageTypes::System(SystemMessage::Startup),
//!     );
//!     ui_tx.send(msg).unwrap();
//!
//!     // Receive messages
//!     while let Some(msg) = ui_rx.recv().await {
//!         // Handle message
//!     }
//! }
//! ```

// Framework modules
pub mod channels;
pub mod errors;
pub mod messages;
pub mod state_machine;
pub mod transport;

// Re-export main framework components
pub use channels::{
    create_bounded_message_channel, create_message_channel, BoundedMessageReceiver,
    BoundedMessageSender, MessageReceiver, MessageSender,
};
pub use errors::{FrameworkError, FrameworkResult, TransportError};
pub use state_machine::{ApplicationStateMachine, State};
pub use messages::{
    ComponentStatus, EmulatorMessage, LogLevel, Message, MessageTypes, RadioMessage,
    SerialMessage, SystemMessage, UIMessage,
};
pub use transport::Transport;
// Re-export commonly used local-sync channel types
pub use local_sync::mpsc::bounded::{Rx as BoundedRx, Tx as BoundedTx};
pub use local_sync::mpsc::unbounded::{Rx as UnboundedRx, Tx as UnboundedTx};

// Re-export monoio runtime
pub use monoio::RuntimeBuilder;

// Re-export commonly used std types for convenience
pub use std::pin::Pin;
pub use std::sync::Arc;

// Re-export monoio async I/O traits
pub use monoio::io::{AsyncReadRent, AsyncWriteRent};
