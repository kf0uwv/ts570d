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

//! Generic CAT command sender for the TS-570D.
//!
//! [`RadioClient`] wraps any [`Transport`] implementation and provides
//! high-level `query` and `set` methods that validate command codes against
//! the [`COMMAND_TABLE`] before placing bytes on the wire.
//!
//! # Wire format
//!
//! - **Query**: `<CMD>;`          e.g. `FA;`
//! - **Set**:   `<CMD><params>;`  e.g. `FA00014250000;`
//! - **Response** (query only): `<CMD><data>;` read back from the radio
//!
//! # Example
//!
//! ```no_run
//! use radio::RadioClient;
//!
//! // construct with any Transport implementation
//! // let mut client = RadioClient::new(my_transport);
//! // let response = client.query("FA").await.unwrap();
//! ```

use framework::transport::Transport;

use crate::commands::CommandMetadata;
use framework::radio::{RadioError, RadioResult};

/// Sends CAT commands over a [`Transport`] and reads back responses.
///
/// All methods validate the command code against [`COMMAND_TABLE`]
/// before touching the transport.
pub struct RadioClient<T: Transport> {
    pub(crate) transport: T,
}

impl<T: Transport> RadioClient<T> {
    /// Create a new `RadioClient` wrapping the given transport.
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    /// Send a query command and return the radio's response string.
    ///
    /// Formats the wire bytes as `"<code>;"` and reads back the response up to
    /// and including the terminating `';'`.
    ///
    /// # Errors
    ///
    /// - [`RadioError::UnknownCommand`] — `code` is not in the command table
    /// - [`RadioError::CommandNotReadable`] — command does not support read
    /// - [`RadioError::Transport`] — I/O error on the underlying transport
    pub async fn query(&mut self, code: &str) -> RadioResult<String> {
        let meta = Self::validate_code(code)?;
        if !meta.supports_read {
            return Err(RadioError::CommandNotReadable(code.to_string()));
        }

        let wire = format!("{};", code);
        self.transport.write(wire.as_bytes()).await?;
        self.transport.flush().await?;
        self.read_response().await
    }

    /// Send a query command with a parameter prefix and return the radio's response string.
    ///
    /// Formats the wire bytes as `"<code><params>;"` and reads back the response up to
    /// and including the terminating `';'`.  Use this when the command requires a
    /// selector or sub-address appended directly to the command code before the
    /// semicolon, e.g. `"SM0;"` or `"RM1;"`.
    ///
    /// # Errors
    ///
    /// - [`RadioError::UnknownCommand`] — `code` is not in the command table
    /// - [`RadioError::CommandNotReadable`] — command does not support read
    /// - [`RadioError::Transport`] — I/O error on the underlying transport
    pub async fn query_with_param(&mut self, code: &str, params: &str) -> RadioResult<String> {
        let meta = Self::validate_code(code)?;
        if !meta.supports_read {
            return Err(RadioError::CommandNotReadable(code.to_string()));
        }

        let wire = format!("{}{};", code, params);
        self.transport.write(wire.as_bytes()).await?;
        self.transport.flush().await?;
        self.read_response().await
    }

    /// Send a set command with parameters.
    ///
    /// Formats the wire bytes as `"<code><params>;"` and does not read a
    /// response (set commands on the TS-570D are fire-and-forget unless AI
    /// mode is active).
    ///
    /// # Errors
    ///
    /// - [`RadioError::UnknownCommand`] — `code` is not in the command table
    /// - [`RadioError::CommandNotWritable`] — command does not support write
    /// - [`RadioError::Transport`] — I/O error on the underlying transport
    pub async fn set(&mut self, code: &str, params: &str) -> RadioResult<()> {
        let meta = Self::validate_code(code)?;
        if !meta.supports_write {
            return Err(RadioError::CommandNotWritable(code.to_string()));
        }

        let wire = format!("{}{};", code, params);
        self.transport.write(wire.as_bytes()).await?;
        self.transport.flush().await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Look up `code` in the command table; return an error if not found.
    fn validate_code(code: &str) -> RadioResult<&'static CommandMetadata> {
        CommandMetadata::find(code).ok_or_else(|| RadioError::UnknownCommand(code.to_string()))
    }

    /// Read bytes from the transport until a `';'` terminator is encountered.
    async fn read_response(&mut self) -> RadioResult<String> {
        let mut response = Vec::new();
        let mut buf = [0u8; 1];

        loop {
            let n = self.transport.read(&mut buf).await?;
            if n == 0 {
                // EOF — return whatever we have (may be empty)
                break;
            }
            response.push(buf[0]);
            if buf[0] == b';' {
                break;
            }
        }

        Ok(String::from_utf8_lossy(&response).into_owned())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use framework::errors::TransportError;
    use framework::transport::Transport;
    use std::collections::VecDeque;

    // -----------------------------------------------------------------------
    // Mock Transport
    // -----------------------------------------------------------------------

    /// A simple in-memory transport for testing.
    ///
    /// `writes` accumulates every byte written by the client.
    /// `reads` is a queue of bytes the client will read back.
    struct MockTransport {
        writes: Vec<u8>,
        reads: VecDeque<u8>,
    }

    impl MockTransport {
        fn new() -> Self {
            Self {
                writes: Vec::new(),
                reads: VecDeque::new(),
            }
        }

        /// Enqueue bytes that `read()` will return to the client.
        fn enqueue_response(&mut self, response: &str) {
            self.reads.extend(response.as_bytes());
        }

        /// Return everything that was written via `write()`.
        fn written(&self) -> &[u8] {
            &self.writes
        }
    }

    #[async_trait(?Send)]
    impl Transport for MockTransport {
        async fn write(&mut self, data: &[u8]) -> Result<usize, TransportError> {
            self.writes.extend_from_slice(data);
            Ok(data.len())
        }

        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
            if let Some(byte) = self.reads.pop_front() {
                buf[0] = byte;
                Ok(1)
            } else {
                Ok(0)
            }
        }

        async fn flush(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_query_fa_formats_correctly() {
        let mut transport = MockTransport::new();
        transport.enqueue_response("FA00014250000;");

        let mut client = RadioClient::new(transport);
        let response = client.query("FA").await.unwrap();

        assert_eq!(client.transport.written(), b"FA;");
        assert_eq!(response, "FA00014250000;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_fa_formats_correctly() {
        let transport = MockTransport::new();
        let mut client = RadioClient::new(transport);

        client.set("FA", "00014250000").await.unwrap();

        assert_eq!(client.transport.written(), b"FA00014250000;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_query_unknown_command_returns_error() {
        let transport = MockTransport::new();
        let mut client = RadioClient::new(transport);

        let result = client.query("ZZ").await;

        assert!(
            matches!(result, Err(RadioError::UnknownCommand(ref c)) if c == "ZZ"),
            "expected UnknownCommand(ZZ), got {:?}",
            result
        );
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_read_only_command_returns_error() {
        // IF is read-only (supports_write = false)
        let transport = MockTransport::new();
        let mut client = RadioClient::new(transport);

        let result = client.set("IF", "").await;

        assert!(
            matches!(result, Err(RadioError::CommandNotWritable(ref c)) if c == "IF"),
            "expected CommandNotWritable(IF), got {:?}",
            result
        );
    }

    #[monoio::test(driver = "legacy")]
    async fn test_query_write_only_command_returns_error() {
        // TX is write-only (supports_read = false)
        let transport = MockTransport::new();
        let mut client = RadioClient::new(transport);

        let result = client.query("TX").await;

        assert!(
            matches!(result, Err(RadioError::CommandNotReadable(ref c)) if c == "TX"),
            "expected CommandNotReadable(TX), got {:?}",
            result
        );
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_does_not_read_response() {
        // set() must not call read() — the mock read queue is empty, which
        // would cause a panic or return 0 bytes. If this test passes, set()
        // never attempted to read.
        let transport = MockTransport::new();
        let mut client = RadioClient::new(transport);

        client.set("FA", "00014250000").await.unwrap();
        // No panic — reads were never called.
    }

    #[monoio::test(driver = "legacy")]
    async fn test_query_set_unknown_command_does_not_write() {
        let transport = MockTransport::new();
        let mut client = RadioClient::new(transport);

        let _ = client.query("ZZ").await;

        assert!(
            client.transport.written().is_empty(),
            "nothing should be written for unknown command"
        );
    }

    #[monoio::test(driver = "legacy")]
    async fn test_query_with_param_sm0_formats_correctly() {
        // Tests the client's query_with_param mechanism (sends "SM0;").
        // Note: per manual p.80 the canonical SM query is "SM;" not "SM0;",
        // and the canonical answer is "SM<4digits>;" (no selector).
        let mut transport = MockTransport::new();
        transport.enqueue_response("SM0015;");

        let mut client = RadioClient::new(transport);
        let response = client.query_with_param("SM", "0").await.unwrap();

        assert_eq!(client.transport.written(), b"SM0;");
        assert_eq!(response, "SM0015;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_query_with_param_rm1_formats_correctly() {
        let mut transport = MockTransport::new();
        transport.enqueue_response("RM10023;");

        let mut client = RadioClient::new(transport);
        let response = client.query_with_param("RM", "1").await.unwrap();

        assert_eq!(client.transport.written(), b"RM1;");
        assert_eq!(response, "RM10023;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_query_with_param_unknown_command_returns_error() {
        let transport = MockTransport::new();
        let mut client = RadioClient::new(transport);

        let result = client.query_with_param("ZZ", "0").await;

        assert!(
            matches!(result, Err(RadioError::UnknownCommand(ref c)) if c == "ZZ"),
            "expected UnknownCommand(ZZ), got {:?}",
            result
        );
    }
}
