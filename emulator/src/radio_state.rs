/// VFO / memory selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VfoSel {
    A,
    B,
    Memory,
}

/// Simulated TS-570D radio state.
///
/// All values use the same units and encoding as the CAT protocol so that
/// `CommandHandler` can format responses directly from this struct without
/// extra conversion.
#[derive(Debug, Clone)]
pub struct RadioState {
    /// VFO A frequency in Hz (11-digit CAT format, e.g. 14_000_000)
    pub vfo_a_hz: u64,
    /// VFO B frequency in Hz
    pub vfo_b_hz: u64,
    /// Operating mode digit: 1=LSB 2=USB 3=CW 4=FM 5=AM 6=FSK 7=CW-R 9=FSK-R
    pub mode: u8,
    /// True when the radio is in transmit state (PTT on)
    pub tx: bool,
    /// AF (audio) gain 0–255
    pub af_gain: u8,
    /// RF gain 0–255
    pub rf_gain: u8,
    /// Squelch level 0–255
    pub squelch: u8,
    /// Transmit power control 0–100 (watts equivalent for PC command)
    pub power_control: u8,
    /// Simulated S-meter reading 0000–0030 (0–30 = S0–S9+60)
    pub smeter: u16,
    /// Auto-Information mode (AI command): 0=off, 1=on
    pub auto_info: u8,

    // --- TUI / front-panel LCD annunciator fields ---

    /// Built-in antenna tuner active
    pub antenna_tuner: bool,
    /// Active antenna port: 1 or 2
    pub antenna: u8,
    /// RF attenuator active
    pub attenuator: bool,
    /// Pre-amplifier active (maps to PRE-AMP annunciator)
    pub preamp: bool,
    /// VOX (voice-operated transmission) active
    pub vox: bool,
    /// Speech processor active
    pub proc: bool,
    /// Noise blanker active
    pub noise_blanker: bool,
    /// Split operation active (TX on alternate VFO)
    pub split: bool,
    /// Fast AGC mode active
    pub fast_agc: bool,
    /// RIT (receive incremental tuning) active
    pub rit: bool,
    /// XIT (transmit incremental tuning) active
    pub xit: bool,
    /// TX equaliser active
    pub tx_eq: bool,
    /// Noise reduction level: 0 = off, 1 = NR1, 2 = NR2
    pub noise_reduction: u8,
    /// Beat cancel (interference notch) active
    pub beat_cancel: bool,
    /// Menu mode active
    pub menu_mode: bool,
    /// Memory scroll mode active
    pub memory_scroll: bool,
    /// Currently active VFO / memory selection
    pub active_vfo: VfoSel,
    /// Frequency lock active
    pub freq_lock: bool,
    /// Fine tuning step active
    pub fine_step: bool,
    /// 1 MHz step active
    pub mhz_step: bool,
    /// Sub-tone (CTCSS/DCS tone) active
    pub subtone: bool,
    /// CTCSS decode active
    pub ctcss: bool,
    /// CTRL (control) mode active (sub-receiver on TS-570DG)
    pub ctrl: bool,
    /// Currently selected memory channel (0–99)
    pub mem_channel: u8,
    /// Currently displayed menu item number (0–99)
    pub menu_number: u8,
}

impl Default for RadioState {
    fn default() -> Self {
        RadioState {
            // 14.000 MHz — general-coverage HF start frequency
            vfo_a_hz: 14_000_000,
            // 14.100 MHz on B
            vfo_b_hz: 14_100_000,
            // USB (2)
            mode: 2,
            tx: false,
            // Mid-range gains
            af_gain: 128,
            rf_gain: 200,
            squelch: 0,
            // 50 W
            power_control: 50,
            // S5 equivalent
            smeter: 10,
            auto_info: 0,

            // TUI fields
            antenna_tuner: false,
            antenna: 1,
            attenuator: false,
            preamp: false,
            vox: false,
            proc: false,
            noise_blanker: false,
            split: false,
            fast_agc: false,
            rit: false,
            xit: false,
            tx_eq: false,
            noise_reduction: 0,
            beat_cancel: false,
            menu_mode: false,
            memory_scroll: false,
            active_vfo: VfoSel::A,
            freq_lock: false,
            fine_step: false,
            mhz_step: false,
            subtone: false,
            ctcss: false,
            ctrl: false,
            mem_channel: 0,
            menu_number: 0,
        }
    }
}
