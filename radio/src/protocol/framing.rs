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

//! Byte-stream framer for TS-570D CAT protocol responses.
//!
//! The TS-570D uses `';'` as a command/response terminator.  [`ResponseFramer`]
//! accepts arbitrary byte chunks (as they arrive from the serial port) and
//! extracts complete, semicolon-delimited frames one at a time.

/// Accumulates raw bytes from a serial stream and extracts complete
/// `;`-delimited frames.
///
/// # Example
///
/// ```
/// use radio::protocol::ResponseFramer;
///
/// let mut framer = ResponseFramer::new();
/// framer.feed(b"FA000142");
/// assert!(!framer.has_frame());
/// framer.feed(b"30000;");
/// assert!(framer.has_frame());
/// assert_eq!(framer.next_frame(), Some("FA00014230000;".to_string()));
/// ```
pub struct ResponseFramer {
    buffer: Vec<u8>,
    overflow_count: usize,
    semicolon_count: usize,
}

impl ResponseFramer {
    /// Maximum number of bytes the internal buffer will hold.
    ///
    /// The longest TS-570D response (`IF`) is around 40 bytes.  We allow a
    /// generous safety margin; if the buffer exceeds this limit the oldest
    /// data is discarded to prevent unbounded memory growth.
    const MAX_BUFFER: usize = 1024;

    /// Create a new, empty framer.
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            overflow_count: 0,
            semicolon_count: 0,
        }
    }

    /// Append `data` bytes to the internal buffer.
    ///
    /// If the buffer would exceed [`MAX_BUFFER`](Self::MAX_BUFFER) after
    /// appending, the buffer is cleared first so that partial/corrupt data
    /// does not block future frames.
    pub fn feed(&mut self, data: &[u8]) {
        if self.buffer.len() + data.len() > Self::MAX_BUFFER {
            // Discard stale data to prevent OOM.
            self.overflow_count += 1;
            self.buffer.clear();
            self.semicolon_count = 0;
        }
        self.semicolon_count += data.iter().filter(|&&b| b == b';').count();
        self.buffer.extend_from_slice(data);
    }

    /// Return `true` if there is at least one complete frame available.
    pub fn has_frame(&self) -> bool {
        self.semicolon_count > 0
    }

    /// Return the number of buffer overflows that have occurred since the last
    /// call to this method, resetting the counter to zero.
    pub fn take_overflow_count(&mut self) -> usize {
        let count = self.overflow_count;
        self.overflow_count = 0;
        count
    }

    /// Extract and return the next complete frame (including the `';'`
    /// terminator), or `None` if no complete frame is present.
    ///
    /// The returned bytes are consumed from the internal buffer so that
    /// subsequent calls return the following frame.
    pub fn next_frame(&mut self) -> Option<String> {
        let pos = self.buffer.iter().position(|&b| b == b';')?;
        // Include the terminator in the frame.
        let frame_bytes: Vec<u8> = self.buffer.drain(..=pos).collect();
        self.semicolon_count -= 1;
        // Convert to String, replacing invalid UTF-8 with replacement chars.
        Some(String::from_utf8_lossy(&frame_bytes).into_owned())
    }

    /// Discard all buffered bytes.
    ///
    /// Useful after a timeout or detected protocol error.
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.semicolon_count = 0;
    }
}

impl Default for ResponseFramer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Basic frame extraction
    // -----------------------------------------------------------------------

    #[test]
    fn test_complete_frame_in_one_feed() {
        let mut framer = ResponseFramer::new();
        framer.feed(b"FA00014230000;");
        assert!(framer.has_frame());
        assert_eq!(framer.next_frame(), Some("FA00014230000;".to_string()));
        assert!(!framer.has_frame());
    }

    #[test]
    fn test_partial_frame_no_frame_yet() {
        let mut framer = ResponseFramer::new();
        framer.feed(b"FA000142");
        assert!(!framer.has_frame());
        assert_eq!(framer.next_frame(), None);
    }

    #[test]
    fn test_partial_then_complete() {
        let mut framer = ResponseFramer::new();
        framer.feed(b"FA000142");
        assert!(!framer.has_frame());
        framer.feed(b"30000;");
        assert!(framer.has_frame());
        assert_eq!(framer.next_frame(), Some("FA00014230000;".to_string()));
        assert!(!framer.has_frame());
    }

    // -----------------------------------------------------------------------
    // Multiple frames
    // -----------------------------------------------------------------------

    #[test]
    fn test_multiple_frames_in_one_feed() {
        let mut framer = ResponseFramer::new();
        framer.feed(b"FA00014230000;MD2;");
        assert!(framer.has_frame());
        assert_eq!(framer.next_frame(), Some("FA00014230000;".to_string()));
        assert!(framer.has_frame());
        assert_eq!(framer.next_frame(), Some("MD2;".to_string()));
        assert!(!framer.has_frame());
    }

    #[test]
    fn test_three_frames_sequential() {
        let mut framer = ResponseFramer::new();
        framer.feed(b"FA00014230000;FB00007100000;MD2;");
        assert_eq!(framer.next_frame(), Some("FA00014230000;".to_string()));
        assert_eq!(framer.next_frame(), Some("FB00007100000;".to_string()));
        assert_eq!(framer.next_frame(), Some("MD2;".to_string()));
        assert_eq!(framer.next_frame(), None);
    }

    #[test]
    fn test_two_frames_split_across_feeds() {
        let mut framer = ResponseFramer::new();
        framer.feed(b"FA00014230000;FB000");
        assert_eq!(framer.next_frame(), Some("FA00014230000;".to_string()));
        assert!(!framer.has_frame());
        framer.feed(b"07100000;");
        assert_eq!(framer.next_frame(), Some("FB00007100000;".to_string()));
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_error_frame() {
        let mut framer = ResponseFramer::new();
        framer.feed(b"?;");
        assert_eq!(framer.next_frame(), Some("?;".to_string()));
    }

    #[test]
    fn test_empty_feed() {
        let mut framer = ResponseFramer::new();
        framer.feed(b"");
        assert!(!framer.has_frame());
        assert_eq!(framer.next_frame(), None);
    }

    #[test]
    fn test_clear_discards_data() {
        let mut framer = ResponseFramer::new();
        framer.feed(b"FA00014230000;");
        framer.clear();
        assert!(!framer.has_frame());
        assert_eq!(framer.next_frame(), None);
    }

    #[test]
    fn test_clear_discards_partial_data() {
        let mut framer = ResponseFramer::new();
        framer.feed(b"FA000142");
        framer.clear();
        framer.feed(b"MD2;");
        assert_eq!(framer.next_frame(), Some("MD2;".to_string()));
    }

    #[test]
    fn test_default_is_empty() {
        let mut framer = ResponseFramer::default();
        assert!(!framer.has_frame());
        assert_eq!(framer.next_frame(), None);
    }

    #[test]
    fn test_buffer_overflow_clears_and_accepts_new_data() {
        let mut framer = ResponseFramer::new();
        // Fill the buffer to the limit with data that has no terminator.
        let big_chunk = vec![b'X'; ResponseFramer::MAX_BUFFER];
        framer.feed(&big_chunk);
        // Now feed a valid frame — the framer should have cleared the stale data.
        framer.feed(b"MD2;");
        assert!(framer.has_frame());
        assert_eq!(framer.next_frame(), Some("MD2;".to_string()));
    }

    #[test]
    fn test_semicolon_only_frame() {
        // An edge case: bare ';' is a valid (if empty) frame.
        let mut framer = ResponseFramer::new();
        framer.feed(b";");
        assert!(framer.has_frame());
        assert_eq!(framer.next_frame(), Some(";".to_string()));
    }

    #[test]
    fn test_remainder_after_frame_extraction() {
        let mut framer = ResponseFramer::new();
        framer.feed(b"MD2;PARTIAL");
        assert_eq!(framer.next_frame(), Some("MD2;".to_string()));
        // Remaining "PARTIAL" is still in the buffer but has no terminator.
        assert!(!framer.has_frame());
        framer.feed(b";");
        assert_eq!(framer.next_frame(), Some("PARTIAL;".to_string()));
    }
}
