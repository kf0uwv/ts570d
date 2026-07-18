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

//! Integrity regression tests for the single `TS570D_COMMAND_TABLE`.
//!
//! These lock in the invariants required after unifying the controller and
//! emulator onto one command table: unique identifiers and codes, well-formed
//! codes, at least one capability per command, and that every table command is
//! recognised and dispatched by the generic framework (never `UnknownCommand`).

use std::collections::HashSet;

use cat_framework::{CatFramework, ProtocolErrorKind, ResponseDisposition};
use radio::{Ts570dRadio, TS570D_COMMAND_TABLE};

#[test]
fn command_ids_are_unique() {
    let defs = TS570D_COMMAND_TABLE.definitions();
    let ids: HashSet<_> = defs.iter().map(|d| d.id).collect();
    assert_eq!(ids.len(), defs.len(), "duplicate command id in table");
}

#[test]
fn command_codes_are_unique() {
    let defs = TS570D_COMMAND_TABLE.definitions();
    let codes: HashSet<_> = defs.iter().map(|d| d.code).collect();
    assert_eq!(codes.len(), defs.len(), "duplicate command code in table");
}

#[test]
fn command_codes_are_two_ascii_uppercase() {
    for d in TS570D_COMMAND_TABLE.definitions() {
        assert_eq!(d.code.len(), 2, "code {:?} is not 2 chars", d.code);
        assert!(
            d.code
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()),
            "code {:?} has non-uppercase/digit chars",
            d.code
        );
    }
}

#[test]
fn every_command_has_a_capability() {
    for d in TS570D_COMMAND_TABLE.definitions() {
        let has_form =
            !d.query_forms.is_empty() || !d.set_forms.is_empty() || !d.action_forms.is_empty();
        assert!(
            d.is_readable() || d.is_writable() || has_form,
            "command {:?} declares no readable/writable/form capability",
            d.code
        );
    }
}

#[test]
fn every_command_is_recognised_and_dispatched() {
    // Build a minimal legal frame for each command and confirm the framework
    // never reports it as an unknown command — i.e., the single table backs the
    // full dispatch surface.
    let mut framework = CatFramework::new(Ts570dRadio::new());

    for d in TS570D_COMMAND_TABLE.definitions() {
        let has_zero_query = d.query_forms.iter().any(|f| f.min_len == 0);
        let has_zero_action = d.action_forms.iter().any(|f| f.min_len == 0);

        let frame = if has_zero_query || has_zero_action {
            format!("{};", d.code)
        } else if let Some(form) = d.set_forms.first() {
            format!("{}{};", d.code, "0".repeat(form.min_len))
        } else {
            format!("{};", d.code)
        };

        let mut out = Vec::new();
        let outcome = framework
            .process_frame(&frame, &mut out)
            .unwrap_or_else(|e| panic!("process_frame failed for {}: {:?}", d.code, e));

        // Some commands (e.g. TX/RX actions) legitimately produce no response;
        // the invariant is that the table recognises and dispatches the command.
        assert!(
            !matches!(
                outcome.response,
                ResponseDisposition::ProtocolError(ProtocolErrorKind::UnknownCommand)
            ),
            "command {:?} reported UnknownCommand despite being in the table",
            d.code
        );
    }
}
