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

    /// Discard any unread bytes in the receive buffer.
    /// Default implementation is a no-op (e.g. for in-memory fakes).
    fn flush_rx(&mut self) {}
}
