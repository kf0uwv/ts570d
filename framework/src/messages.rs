//! Message Types and Communication Patterns
//!
//! Defines message types used for inter-crate communication.
//!
//! ## Extending Message Types
//!
//! To add new message types for your component:
//! 1. Edit this file and add a new variant to the `MessageTypes` enum
//! 2. Define your component's message enum (e.g., `MyComponentMessage`)
//! 3. Handle messages in your component's receive loop
//!
//! Example:
//! ```rust,ignore
//! // 1. Add variant to MessageTypes enum
//! pub enum MessageTypes {
//!     // ... existing variants ...
//!     MyComponent(MyComponentMessage),
//! }
//!
//! // 2. Define your message enum
//! #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
//! pub enum MyComponentMessage {
//!     DoSomething { data: String },
//!     StatusUpdate { status: String },
//! }
//!
//! // 3. Handle in your component
//! async fn handle_messages(mut rx: MessageReceiver) {
//!     while let Some(msg) = rx.recv().await {
//!         match msg.payload {
//!             MessageTypes::MyComponent(cmd) => {
//!                 // Handle your messages
//!             }
//!             _ => {}
//!         }
//!     }
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Core message types used throughout the application
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MessageTypes {
    /// Serial communication messages
    Serial(SerialMessage),

    /// Radio control messages  
    Radio(RadioMessage),

    /// User interface messages
    UI(UIMessage),

    /// System coordination messages
    System(SystemMessage),

    /// Emulator messages
    Emulator(EmulatorMessage),
}

/// Serial communication messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SerialMessage {
    /// Connect to serial port
    Connect { port: String, baud_rate: u32 },

    /// Disconnect from serial port
    Disconnect,

    /// Send raw data to serial port
    SendData { data: Vec<u8> },

    /// Data received from serial port
    DataReceived { data: Vec<u8> },

    /// Serial port status
    Status { connected: bool, port: String },
}

/// Radio control messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RadioMessage {
    /// Set frequency
    SetFrequency { frequency: f64 },

    /// Get current frequency
    GetFrequency,

    /// Frequency response
    FrequencyResponse { frequency: f64 },

    /// Set mode (USB, LSB, CW, etc.)
    SetMode { mode: String },

    /// Get current mode
    GetMode,

    /// Mode response
    ModeResponse { mode: String },

    /// Send raw command to radio
    SendCommand { command: String },

    /// Raw response from radio
    CommandResponse { response: String },
}

/// User interface messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum UIMessage {
    /// Initialize user interface
    Initialize,

    /// Update display with new data
    UpdateDisplay { data: HashMap<String, String> },

    /// User input event
    UserInput { input: String },

    /// Display status message
    StatusMessage { message: String, level: LogLevel },

    /// Request user input
    RequestInput { prompt: String },
}

/// System coordination messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SystemMessage {
    /// Application startup
    Startup,

    /// Application shutdown
    Shutdown,

    /// Component status update
    ComponentStatus {
        component: String,
        status: ComponentStatus,
    },

    /// Error notification
    Error { source: String, error: String },

    /// Configuration update
    ConfigUpdate { key: String, value: String },
}

/// Emulator messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EmulatorMessage {
    /// Start emulator
    Start,

    /// Stop emulator
    Stop,

    /// Emulator status
    Status { running: bool },

    /// Virtual TTY created
    VirtualTTYCreated { path: String },
}

/// Log levels for UI messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
    Debug,
}

/// Component status for system messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ComponentStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Error(String),
}

/// Generic message wrapper with metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    /// Unique message identifier
    pub id: String,

    /// Message timestamp
    pub timestamp: u64,

    /// Source component
    pub source: String,

    /// Destination component (empty for broadcast)
    pub destination: String,

    /// Actual message payload
    pub payload: MessageTypes,

    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl Message {
    /// Create a new message
    pub fn new(id: String, source: String, destination: String, payload: MessageTypes) -> Self {
        Self {
            id,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            source,
            destination,
            payload,
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to message
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Serialize message to bytes
    ///
    /// Returns the serialized message or an error if serialization fails.
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Deserialize message from bytes
    ///
    /// Returns the deserialized message or an error if deserialization fails.
    pub fn from_bytes(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }
}
