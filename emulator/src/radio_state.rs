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

    // --- Additional fields for expanded CAT command coverage ---
    /// Scan active
    pub scan: bool,
    /// Microphone gain 0–255
    pub mic_gain: u8,
    /// AGC time constant (GT command): 0=off, 1=fast, 2=mid, 3=mid-slow, 4=slow
    pub agc: u8,
    /// VOX gain 0–255
    pub vox_gain: u8,
    /// VOX delay in milliseconds (0–1000)
    pub vox_delay: u16,
    /// RX VFO selection (FR command): 0=VFO-A, 1=VFO-B, 2=memory
    pub rx_vfo: u8,
    /// TX VFO selection (FT command): 0=VFO-A, 1=VFO-B
    pub tx_vfo: u8,
    /// Power on/off state (PS command)
    pub power_on: bool,
    /// CW keyer speed in WPM (KS command)
    pub keyer_speed: u8,
    /// CW pitch 0–12 (PT command)
    pub cw_pitch: u8,
    /// CW auto zero-beat (CA command)
    pub cw_auto_zerobeat: bool,
    /// Antenna tuner mode (AC command): packed 3-digit code
    pub ac_mode: u8,
    /// Filter high cutoff (SH command): 0–10
    pub sh: u8,
    /// Filter low cutoff (SL command): 0–10
    pub sl: u8,
    /// IF shift direction: '+' or '-'
    pub is_direction: char,
    /// IF shift frequency offset 0–9999 Hz
    pub is_freq: u16,
    /// CTCSS tone number (CN command): 00–39
    pub ctcss_tone: u8,
    /// Tone number (TN command): 00–39
    pub tone_number: u8,
    /// Beat cancel mode (BC command): 0=off, 1=on, 2=enhanced
    pub beat_cancel_mode: u8,
    /// Semi break-in delay in ms (SD command): 0–1000
    pub semi_break_in_delay: u16,
    /// RIT offset in Hz (–9999 to +9999)
    pub rit_offset: i32,
    /// XIT offset in Hz (–9999 to +9999)
    pub xit_offset: i32,
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

            scan: false,
            mic_gain: 50,
            agc: 4,
            vox_gain: 128,
            vox_delay: 250,
            rx_vfo: 0,
            tx_vfo: 0,
            power_on: true,
            keyer_speed: 20,
            cw_pitch: 6,
            cw_auto_zerobeat: false,
            ac_mode: 0,
            sh: 10,
            sl: 0,
            is_direction: '+',
            is_freq: 0,
            ctcss_tone: 0,
            tone_number: 0,
            beat_cancel_mode: 0,
            semi_break_in_delay: 0,
            rit_offset: 0,
            xit_offset: 0,
        }
    }
}
