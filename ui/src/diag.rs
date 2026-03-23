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
    Done { results: Vec<DiagResult> },
}
