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

//! The TS-570D's `KY` (CW KEYING) set, built and checked without I/O.
//!
//! From the CAT reference (parameters 43 and 44): the set is `KY`, a space
//! (ASCII 20h) in byte 3, then the message **space-padded to 24
//! characters** -- a 28-byte fixed-length command. "Up to 24 characters
//! can be converted as one sentence at one time. The subsequent sentence
//! must be keyed after the current conversion is completed." The read
//! `KY;` answers `KY0;` (buffer space available) or `KY1;` (not available)
//! -- buffer *space*, not "finished keying".
//!
//! One module so the controller (`Ts570d::send_cw`) and the emulator's
//! `KY` handler cannot disagree about the form or the character set.

use crate::{RadioError, RadioResult};

/// The most characters one `KY` sentence carries.
pub const KY_MAX_CHARS: usize = 24;

/// The characters the keyer converts (upper case), from the manual:
/// "Letters, numbers, `" ' ( ) + , - . / : = ?`". A space is also allowed
/// between words; the manual's padding blanks "will not send any
/// character".
pub const KY_CHARSET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789\"'()+,-./:=?";

/// Whether the keyer can send `c` (upper case only; a space counts).
pub fn ky_accepts(c: char) -> bool {
    c == ' ' || KY_CHARSET.contains(c)
}

/// The `KY` set parameter for `message`: a space then the upper-cased
/// message padded to [`KY_MAX_CHARS`] -- 25 characters, so the whole
/// command is 28 bytes.
///
/// Refused, before anything is written: an empty or all-blank message (it
/// would send nothing), more than 24 characters, or any character the
/// keyer cannot send -- including `;`, which would end the frame early.
pub fn ky_parameter(message: &str) -> RadioResult<String> {
    let text = message.to_ascii_uppercase();
    if text.trim().is_empty() {
        return Err(RadioError::InvalidCwText("empty message".to_string()));
    }
    if let Some(bad) = text.chars().find(|&c| !ky_accepts(c)) {
        return Err(RadioError::InvalidCwText(format!(
            "the keyer cannot send {bad:?}"
        )));
    }
    // Every accepted character is ASCII, so bytes == characters here.
    if text.len() > KY_MAX_CHARS {
        return Err(RadioError::InvalidCwText(format!(
            "{} characters; one KY sentence holds at most {KY_MAX_CHARS}",
            text.len()
        )));
    }
    Ok(format!(" {text:<KY_MAX_CHARS$}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_is_a_space_and_24_padded_characters() {
        let p = ky_parameter("cq").unwrap();
        assert_eq!(p.len(), 1 + KY_MAX_CHARS);
        assert_eq!(p, format!(" CQ{}", " ".repeat(22)));
        assert_eq!(format!("KY{p};").len(), 28);
    }

    #[test]
    fn every_manual_character_is_accepted() {
        assert!(ky_parameter("\"'()+,-./:=?").is_ok());
        assert!(ky_parameter("ABCDEFGHIJKLMNOPQRSTUVWX").is_ok());
        assert!(ky_parameter("YZ0123456789").is_ok());
    }

    #[test]
    fn refusals() {
        let long = "E".repeat(25);
        for bad in ["", " ", "A;", "SK!", "\u{e9}", long.as_str()] {
            assert!(
                matches!(ky_parameter(bad), Err(RadioError::InvalidCwText(_))),
                "{bad:?}"
            );
        }
    }
}
