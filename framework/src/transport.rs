//! Transport trait for byte-level serial communication.
//!
//! This module defines the `Transport` trait that decouples radio protocol
//! handling from the concrete serial port implementation.

use async_trait::async_trait;

use crate::errors::TransportError;

/// Byte-level transport interface for serial communication.
///
/// Implemented by `serial::SerialPort` (production) and test doubles.
/// Used by `radio::Ts570d<T: Transport>` without the radio crate
/// depending on the serial crate directly.
///
/// # monoio compatibility
/// Uses `#[async_trait(?Send)]` — no Send bounds, compatible with
/// monoio's thread-per-core model where futures are !Send.
#[async_trait(?Send)]
pub trait Transport {
    /// Write bytes to the transport. Returns number of bytes written.
    async fn write(&mut self, data: &[u8]) -> Result<usize, TransportError>;

    /// Read bytes from the transport into buf. Returns number of bytes read.
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, TransportError>;

    /// Flush any buffered writes.
    async fn flush(&mut self) -> Result<(), TransportError>;
}
