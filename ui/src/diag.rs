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

//! Diagnostic mode data model.
//!
//! `DiagState` tracks the lifecycle of a diagnostic run. The UI transitions
//! from `Idle` → `Running` when [D] is pressed, then to `Done` once all
//! commands have completed.

/// Number of times each diagnostic command is repeated.
pub(crate) const DIAG_ROUNDS: usize = 3;

/// Result of a single diagnostic step (one command, one round).
#[derive(Debug, Clone)]
pub(crate) struct DiagResult {
    /// Short label, e.g. `"set_vfo_a/get_vfo_a"`.
    pub(crate) label: &'static str,
    /// Round index, 1..=DIAG_ROUNDS.
    pub(crate) round: usize,
    /// Whether the check passed.
    pub(crate) passed: bool,
    /// `"ok"` or a brief error/mismatch description.
    pub(crate) detail: String,
}

/// Diagnostic run lifecycle.
pub(crate) enum DiagState {
    /// Not yet started (or reset). Shows "Press [D] to run diagnostics".
    #[allow(dead_code)]
    Idle,
    /// Actively running.
    Running {
        /// Label of the command currently being tested.
        current_label: &'static str,
        /// Round currently in progress (1..=DIAG_ROUNDS).
        current_round: usize,
        /// Results accumulated so far.
        results: Vec<DiagResult>,
    },
    /// All steps finished (or aborted). `results` contains the full log.
    /// `scroll` is the number of lines scrolled from the top.
    Done {
        results: Vec<DiagResult>,
        scroll: usize,
    },
}
