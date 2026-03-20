//! Channel-based Communication Patterns
//!
//! Provides type aliases and helper functions for creating monoio channels
//! used in inter-component communication.
//!
//! ## Overview
//!
//! This module defines channel types based on `local-sync`'s `mpsc` (multi-producer,
//! single-consumer) channels. These are thread-local channels designed specifically
//! for monoio. Unlike standard library channels, these channels are **!Send**
//! (not thread-safe) because monoio uses a thread-per-core model where futures
//! are not moved between threads.
//!
//! ## Usage
//!
//! Components should use channels to communicate with each other:
//!
//! ```rust,ignore
//! use framework::channels::create_message_channel;
//! use framework::Message;
//!
//! // Create a channel for messages
//! let (tx, rx) = create_message_channel();
//!
//! // Send messages
//! tx.send(message).await.unwrap();
//!
//! // Receive messages
//! while let Some(msg) = rx.recv().await {
//!     // Process message
//! }
//! ```
//!
//! ## Architecture Pattern
//!
//! Each component should:
//! 1. Own a receiver for incoming messages
//! 2. Hold senders to other components it needs to communicate with
//! 3. Run a receive loop processing messages asynchronously
//!
//! Example:
//! ```rust,ignore
//! struct MyComponent {
//!     rx: MessageReceiver,
//!     ui_tx: MessageSender,
//!     serial_tx: MessageSender,
//! }
//!
//! impl MyComponent {
//!     async fn run(&mut self) {
//!         while let Some(msg) = self.rx.recv().await {
//!             match msg.payload {
//!                 MessageTypes::Serial(cmd) => {
//!                     // Handle serial message
//!                     self.serial_tx.send(response).await.ok();
//!                 }
//!                 _ => {}
//!             }
//!         }
//!     }
//! }
//! ```

use crate::Message;

/// Sender side of an unbounded message channel
///
/// This is a type alias for local-sync's unbounded sender. It can be cloned
/// to create multiple senders to the same receiver.
///
/// Note: This type is **!Send** and cannot be moved between threads.
pub type MessageSender = local_sync::mpsc::unbounded::Tx<Message>;

/// Receiver side of an unbounded message channel
///
/// This is a type alias for local-sync's unbounded receiver. There can only
/// be one receiver per channel.
///
/// Note: This type is **!Send** and cannot be moved between threads.
pub type MessageReceiver = local_sync::mpsc::unbounded::Rx<Message>;

/// Sender side of a bounded message channel
///
/// Unlike unbounded channels, bounded channels have a fixed capacity
/// and will apply backpressure when full.
///
/// Note: This type is **!Send** and cannot be moved between threads.
pub type BoundedMessageSender = local_sync::mpsc::bounded::Tx<Message>;

/// Receiver side of a bounded message channel
///
/// Note: This type is **!Send** and cannot be moved between threads.
pub type BoundedMessageReceiver = local_sync::mpsc::bounded::Rx<Message>;

/// Create an unbounded message channel
///
/// Returns a tuple of (sender, receiver). The sender can be cloned to create
/// multiple producers, but there can only be one consumer.
///
/// Unbounded channels have no capacity limit and will never block on send,
/// but can consume unbounded memory if messages are produced faster than
/// they are consumed.
///
/// # Example
///
/// ```rust,ignore
/// let (tx, rx) = create_message_channel();
///
/// // Clone sender for multiple producers
/// let tx2 = tx.clone();
///
/// // Send from multiple producers
/// tx.send(msg1).await.unwrap();
/// tx2.send(msg2).await.unwrap();
///
/// // Receive from single consumer
/// while let Some(msg) = rx.recv().await {
///     println!("Received: {:?}", msg);
/// }
/// ```
pub fn create_message_channel() -> (MessageSender, MessageReceiver) {
    local_sync::mpsc::unbounded::channel()
}

/// Create a bounded message channel with specified capacity
///
/// Returns a tuple of (sender, receiver). When the channel is full,
/// send operations will wait until space is available.
///
/// Bounded channels apply backpressure to prevent unbounded memory growth
/// and can help maintain system stability under high load.
///
/// # Arguments
///
/// * `capacity` - Maximum number of messages the channel can hold
///
/// # Example
///
/// ```rust,ignore
/// // Create channel with capacity for 100 messages
/// let (tx, rx) = create_bounded_message_channel(100);
///
/// // Send will block if channel is full
/// tx.send(msg).await.unwrap();
/// ```
pub fn create_bounded_message_channel(capacity: usize) -> (BoundedMessageSender, BoundedMessageReceiver) {
    local_sync::mpsc::bounded::channel(capacity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MessageTypes, SystemMessage};

    #[monoio::test]
    async fn test_unbounded_channel_creation() {
        let (tx, mut rx) = create_message_channel();

        // Test sending and receiving a message
        let msg = Message::new(
            "test-1".to_string(),
            "test".to_string(),
            "dest".to_string(),
            MessageTypes::System(SystemMessage::Startup),
        );

        tx.send(msg.clone()).unwrap();

        if let Some(received) = rx.recv().await {
            assert_eq!(received.id, msg.id);
            assert_eq!(received.payload, msg.payload);
        } else {
            panic!("Failed to receive message");
        }
    }

    #[monoio::test]
    async fn test_bounded_channel_creation() {
        let (tx, mut rx) = create_bounded_message_channel(10);

        let msg = Message::new(
            "test-1".to_string(),
            "test".to_string(),
            "dest".to_string(),
            MessageTypes::System(SystemMessage::Startup),
        );

        tx.send(msg.clone()).await.unwrap();

        if let Some(received) = rx.recv().await {
            assert_eq!(received.id, msg.id);
        } else {
            panic!("Failed to receive message");
        }
    }

    #[monoio::test]
    async fn test_multiple_senders() {
        let (tx, mut rx) = create_message_channel();
        let tx2 = tx.clone();

        let msg1 = Message::new(
            "1".to_string(),
            "sender1".to_string(),
            "dest".to_string(),
            MessageTypes::System(SystemMessage::Startup),
        );

        let msg2 = Message::new(
            "2".to_string(),
            "sender2".to_string(),
            "dest".to_string(),
            MessageTypes::System(SystemMessage::Shutdown),
        );

        tx.send(msg1.clone()).unwrap();
        tx2.send(msg2.clone()).unwrap();

        // Receive both messages
        let received1 = rx.recv().await.unwrap();
        let received2 = rx.recv().await.unwrap();

        assert_eq!(received1.id, "1");
        assert_eq!(received2.id, "2");
    }
}
