//! TS-570D Terminal UI
//!
//! Provides the ratatui-based terminal interface for the radio controller.

pub(crate) mod layout;
mod terminal;

pub use terminal::run;

#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type UiResult<T> = Result<T, UiError>;

/// Live radio state for UI rendering.
/// All fields have defaults matching TS-570D power-on state.
#[derive(Debug, Clone)]
pub struct RadioDisplay {
    /// VFO A frequency in Hz (e.g., 14_000_000 for 14.000 MHz)
    pub vfo_a_hz: u64,
    /// VFO B frequency in Hz
    pub vfo_b_hz: u64,
    /// Operating mode name (e.g., "USB", "LSB", "CW")
    pub mode: String,
    /// S-meter reading (0–30 scale per TS-570D CAT protocol)
    pub smeter: u16,
    /// True when transmitting
    pub tx: bool,
}

impl Default for RadioDisplay {
    fn default() -> Self {
        Self {
            vfo_a_hz: 14_000_000,
            vfo_b_hz: 14_100_000,
            mode: "USB".to_string(),
            smeter: 10,
            tx: false,
        }
    }
}
