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
//! [`RadioClient`] wraps any [`CatSession`] implementation and provides
//! high-level `query` and `set` methods that validate command codes against
//! the [`TS570D_COMMAND_TABLE`](crate::TS570D_COMMAND_TABLE) before placing bytes on the wire.
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
//! // construct with any CatSession implementation
//! // let mut client = RadioClient::new(my_session);
//! // let response = client.query("FA").await.unwrap();
//! ```

use framework::errors::TransportError;
use framework::session::CatSession;
use framework::{CommandDefinition, ResponseDisposition};

use crate::ts570d_radio::{Ts570dCommandId, TS570D_COMMAND_TABLE};
use crate::{RadioError, RadioResult};

/// Sends CAT commands over a [`CatSession`] and reads back responses.
///
/// All methods validate the command code against [`TS570D_COMMAND_TABLE`](crate::TS570D_COMMAND_TABLE)
/// before touching the session.
pub struct RadioClient<S: CatSession> {
    pub(crate) session: S,
}

impl<S> RadioClient<S>
where
    S: CatSession<Error = TransportError>,
{
    /// Create a new `RadioClient` wrapping the given session.
    pub fn new(session: S) -> Self {
        Self { session }
    }

    /// Send a query command and return the radio's response string.
    ///
    /// Formats the wire bytes as `"<code>;"` and returns the response the
    /// session reports back for that exchange.
    ///
    /// # Errors
    ///
    /// - [`RadioError::UnknownCommand`] — `code` is not in the command table
    /// - [`RadioError::CommandNotReadable`] — command does not support read
    /// - [`RadioError::Transport`] — I/O error on the underlying session
    pub async fn query(&mut self, code: &str) -> RadioResult<String> {
        let meta = Self::validate_code(code)?;
        if !meta.is_readable() {
            return Err(RadioError::CommandNotReadable(code.to_string()));
        }

        let wire = format!("{};", code);
        self.execute_query(wire.as_bytes()).await
    }

    /// Send a query command with a parameter prefix and return the radio's response string.
    ///
    /// Formats the wire bytes as `"<code><params>;"`.  Use this when the command
    /// requires a selector or sub-address appended directly to the command code
    /// before the semicolon, e.g. `"SM0;"` or `"RM1;"`.
    ///
    /// # Errors
    ///
    /// - [`RadioError::UnknownCommand`] — `code` is not in the command table
    /// - [`RadioError::CommandNotReadable`] — command does not support read
    /// - [`RadioError::Transport`] — I/O error on the underlying session
    pub async fn query_with_param(&mut self, code: &str, params: &str) -> RadioResult<String> {
        let meta = Self::validate_code(code)?;
        if !meta.is_readable() {
            return Err(RadioError::CommandNotReadable(code.to_string()));
        }

        let wire = format!("{}{};", code, params);
        self.execute_query(wire.as_bytes()).await
    }

    /// Send a set command with parameters.
    ///
    /// Formats the wire bytes as `"<code><params>;"` and does not wait for a
    /// response (set commands on the TS-570D are fire-and-forget unless AI
    /// mode is active) — delegates to [`CatSession::send`], which real
    /// sessions implement without blocking on a read.
    ///
    /// # Errors
    ///
    /// - [`RadioError::UnknownCommand`] — `code` is not in the command table
    /// - [`RadioError::CommandNotWritable`] — command does not support write
    /// - [`RadioError::Transport`] — I/O error on the underlying session
    pub async fn set(&mut self, code: &str, params: &str) -> RadioResult<()> {
        let meta = Self::validate_code(code)?;
        if !meta.is_writable() {
            return Err(RadioError::CommandNotWritable(code.to_string()));
        }

        let wire = format!("{}{};", code, params);
        self.session.send(wire.as_bytes()).await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Look up `code` in the command table; return an error if not found.
    fn validate_code(code: &str) -> RadioResult<&'static CommandDefinition<Ts570dCommandId>> {
        TS570D_COMMAND_TABLE
            .find(code)
            .ok_or_else(|| RadioError::UnknownCommand(code.to_string()))
    }

    /// Execute one query-shaped exchange through the session and decode the
    /// response bytes into a `String` matching the wire text the radio sent.
    async fn execute_query(&mut self, wire: &[u8]) -> RadioResult<String> {
        let mut response = Vec::new();
        let disposition = self.session.execute(wire, &mut response).await?;
        match disposition {
            ResponseDisposition::ProtocolError(kind) => Err(RadioError::InvalidProtocolString(
                format!("session reported a protocol error: {:?}", kind),
            )),
            ResponseDisposition::ResponseWritten | ResponseDisposition::NoResponse => {
                Ok(String::from_utf8_lossy(&response).into_owned())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use framework::test_support::{Exchange, ScriptedCatSession};

    // -----------------------------------------------------------------------
    // Helper
    // -----------------------------------------------------------------------

    /// Build a `RadioClient` around a `ScriptedCatSession` pre-loaded with
    /// `script`. Supersedes the old ad hoc `MockTransport` — see
    /// `framework::test_support` for the shared implementation.
    fn client_with_script<I: IntoIterator<Item = Exchange>>(
        script: I,
    ) -> RadioClient<ScriptedCatSession> {
        RadioClient::new(ScriptedCatSession::with_script(script))
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[monoio::test(driver = "legacy")]
    async fn test_query_fa_formats_correctly() {
        let mut client = client_with_script([Exchange::new("FA;", "FA00014250000;")]);

        let response = client.query("FA").await.unwrap();

        assert_eq!(client.session.written(), b"FA;");
        assert_eq!(response, "FA00014250000;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_fa_formats_correctly() {
        let mut client = client_with_script([Exchange::new("FA00014250000;", "")]);

        client.set("FA", "00014250000").await.unwrap();

        assert_eq!(client.session.written(), b"FA00014250000;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_query_unknown_command_returns_error() {
        let mut client = client_with_script([]);

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
        let mut client = client_with_script([]);

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
        let mut client = client_with_script([]);

        let result = client.query("TX").await;

        assert!(
            matches!(result, Err(RadioError::CommandNotReadable(ref c)) if c == "TX"),
            "expected CommandNotReadable(TX), got {:?}",
            result
        );
    }

    #[monoio::test(driver = "legacy")]
    async fn test_set_does_not_read_response() {
        // set() must call session.send(), never session.execute() with a
        // read-shaped expectation. The dedicated regression test for "send()
        // must not block on a read" lives in
        // `framework::session::tests::send_writes_without_reading`, which
        // uses a Transport whose read() panics — a real behavioral proof
        // that in-memory sessions like this one cannot provide. Here we only
        // confirm set() succeeds against a single fire-and-forget exchange.
        let mut client = client_with_script([Exchange::new("FA00014250000;", "")]);

        client.set("FA", "00014250000").await.unwrap();
    }

    #[monoio::test(driver = "legacy")]
    async fn test_query_set_unknown_command_does_not_write() {
        let mut client = client_with_script([]);

        let _ = client.query("ZZ").await;

        assert!(
            client.session.written().is_empty(),
            "nothing should be written for unknown command"
        );
    }

    #[monoio::test(driver = "legacy")]
    async fn test_query_with_param_sm0_formats_correctly() {
        // Tests the client's query_with_param mechanism (sends "SM0;").
        // Note: per manual p.80 the canonical SM query is "SM;" not "SM0;",
        // and the canonical answer is "SM<4digits>;" (no selector).
        let mut client = client_with_script([Exchange::new("SM0;", "SM0015;")]);

        let response = client.query_with_param("SM", "0").await.unwrap();

        assert_eq!(client.session.written(), b"SM0;");
        assert_eq!(response, "SM0015;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_query_with_param_rm1_formats_correctly() {
        let mut client = client_with_script([Exchange::new("RM1;", "RM10023;")]);

        let response = client.query_with_param("RM", "1").await.unwrap();

        assert_eq!(client.session.written(), b"RM1;");
        assert_eq!(response, "RM10023;");
    }

    #[monoio::test(driver = "legacy")]
    async fn test_query_with_param_unknown_command_returns_error() {
        let mut client = client_with_script([]);

        let result = client.query_with_param("ZZ", "0").await;

        assert!(
            matches!(result, Err(RadioError::UnknownCommand(ref c)) if c == "ZZ"),
            "expected UnknownCommand(ZZ), got {:?}",
            result
        );
    }
}
