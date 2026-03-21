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
        }
    }
}
