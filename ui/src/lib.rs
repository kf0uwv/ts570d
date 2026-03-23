//! TS-570D Terminal UI
//!
//! Provides the ratatui-based terminal interface for the radio controller.

pub(crate) mod control;
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
    // --- Primary (from IF / get_information) ---
    pub vfo_a_hz: u64,
    pub vfo_b_hz: u64,
    pub mode: String,
    pub tx: bool,
    pub rit: bool,
    pub xit: bool,
    pub rit_xit_offset_hz: i32,
    pub split: bool,
    pub scan: bool,
    pub memory_channel: u8,
    pub memory_mode: bool,

    // --- Meters ---
    pub smeter: u16,

    // --- Gains / levels ---
    pub af_gain: u8,
    pub rf_gain: u8,
    pub squelch: u8,
    pub mic_gain: u8,
    pub power_pct: u8,
    pub agc: u8,

    // --- Receiver features ---
    pub noise_blanker: bool,
    pub noise_reduction: u8,
    pub preamp: bool,
    pub attenuator: bool,
    pub speech_processor: bool,
    pub beat_cancel: u8,

    // --- Transmit ---
    pub vox: bool,
    pub antenna: u8,

    // --- Tone ---
    pub ctcss: bool,
    pub freq_lock: bool,
    pub fine_step: bool,

    // --- Poll errors (from most recent poll cycle) ---
    pub poll_errors: Vec<String>,
}

impl Default for RadioDisplay {
    fn default() -> Self {
        Self {
            vfo_a_hz: 14_000_000,
            vfo_b_hz: 14_100_000,
            mode: "USB".to_string(),
            tx: false,
            rit: false,
            xit: false,
            rit_xit_offset_hz: 0,
            split: false,
            scan: false,
            memory_channel: 0,
            memory_mode: false,
            smeter: 0,
            af_gain: 200,
            rf_gain: 255,
            squelch: 0,
            mic_gain: 50,
            power_pct: 100,
            agc: 2,
            noise_blanker: false,
            noise_reduction: 0,
            preamp: false,
            attenuator: false,
            speech_processor: false,
            beat_cancel: 0,
            vox: false,
            antenna: 1,
            ctcss: false,
            freq_lock: false,
            fine_step: false,
            poll_errors: Vec::new(),
        }
    }
}
