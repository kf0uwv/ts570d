//! Typed response variants for the TS-570D CAT protocol.
//!
//! The [`Response`] enum wraps every distinct response the radio can emit.
//! [`InformationResponse`] is defined in `framework::radio` and re-exported
//! here for convenience.

pub use framework::radio::InformationResponse;

use framework::radio::{Frequency, Mode};

/// A parsed response from the TS-570D radio.
///
/// Each variant corresponds to the 2-letter command code whose reply it
/// represents.  The `Error` variant corresponds to the `?;` error reply.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    /// FA — VFO A frequency
    VfoAFrequency(Frequency),
    /// FB — VFO B frequency
    VfoBFrequency(Frequency),
    /// MD — operating mode
    Mode(Mode),
    /// ID — radio model identifier (018 = TS-570D, 019 = TS-570S)
    RadioId(u16),
    /// IF — composite information response
    Information(InformationResponse),
    /// SM — S-meter reading.  Fields: (main_sub selector, reading 0–30)
    SMeter(u8, u16),
    /// AG — AF gain level.  Fields: (main_sub selector 0/1, level 0–255)
    AfGain(u8, u8),
    /// RG — RF gain level (0–255)
    RfGain(u8),
    /// SQ — squelch level (0–255)
    Squelch(u8),
    /// PC — transmit power (5–100 W)
    Power(u8),
    /// NB — noise blanker on/off
    NoiseBlanker(bool),
    /// NR — noise reduction level (0=off, 1=NR1, 2=NR2)
    NoiseReduction(u8),
    /// PA — pre-amplifier on/off
    Preamp(bool),
    /// RA — attenuator on/off (00=off, 01=on)
    Attenuator(bool),
    /// MG — microphone gain (0–100)
    MicGain(u8),
    /// GT — AGC time constant (002=fast, 004=slow)
    Agc(u8),
    /// RT — RIT on/off
    Rit(bool),
    /// XT — XIT on/off
    Xit(bool),
    /// SC — scan on/off
    Scan(bool),
    /// VX — VOX on/off
    Vox(bool),
    /// VG — VOX gain (001–009)
    VoxGain(u8),
    /// VD — VOX delay in ms (0–3000)
    VoxDelay(u16),
    /// FR — receiver VFO/memory selection (0=VFO A, 1=VFO B, 2=Memory)
    RxVfo(u8),
    /// FT — transmitter VFO/memory selection (0=VFO A, 1=VFO B, 2=Memory)
    TxVfo(u8),
    /// LK — frequency lock on/off
    FrequencyLock(bool),
    /// PS — power on/off
    PowerOn(bool),
    /// BY — busy indicator (0=not busy, 1=busy)
    Busy(bool),
    /// PR — speech processor on/off
    SpeechProcessor(bool),
    /// MC — memory channel (00–99)
    MemoryChannel(u8),
    /// AN — antenna selection (1=ANT1, 2=ANT2)
    Antenna(u8),
    /// CN — CTCSS tone number (01–39)
    CtcssTone(u8),
    /// CT — CTCSS on/off
    Ctcss(bool),
    /// TN — tone number (01–39)
    ToneNumber(u8),
    /// TO — tone on/off
    Tone(bool),
    /// BC — beat cancel (0=off, 1=on, 2=enhanced)
    BeatCancel(u8),
    /// IS — IF shift (direction char, frequency)
    IfShift(char, u16),
    /// KS — keyer speed in WPM (10–60)
    KeyerSpeed(u8),
    /// PT — CW pitch (00–12)
    CwPitch(u8),
    /// RM — meter reading (meter_type, value)
    Meter(u8, u16),
    /// SD — semi break-in delay in ms (0–1000)
    SemiBreakInDelay(u16),
    /// CA — CW auto zero-beat on/off
    CwAutoZerobeat(bool),
    /// FS — fine step on/off
    FineStep(bool),
    /// SH — DSP high cutoff filter index (00–20)
    HighCutoff(u8),
    /// SL — DSP low cutoff filter index (00–20)
    LowCutoff(u8),
    /// `?;` error response
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_error_variant() {
        let r = Response::Error;
        assert_eq!(r, Response::Error);
    }

    #[test]
    fn test_response_vfoa_frequency() {
        let freq = Frequency::new(14_230_000).unwrap();
        let r = Response::VfoAFrequency(freq);
        assert_eq!(
            r,
            Response::VfoAFrequency(Frequency::new(14_230_000).unwrap())
        );
    }

    #[test]
    fn test_response_mode() {
        let r = Response::Mode(Mode::Usb);
        assert_eq!(r, Response::Mode(Mode::Usb));
    }

    #[test]
    fn test_response_radio_id() {
        let r = Response::RadioId(18);
        assert_eq!(r, Response::RadioId(18));
    }

    #[test]
    fn test_response_smeter() {
        let r = Response::SMeter(0, 15);
        assert_eq!(r, Response::SMeter(0, 15));
    }

    #[test]
    fn test_response_af_gain() {
        let r = Response::AfGain(0, 128);
        assert_eq!(r, Response::AfGain(0, 128));
    }

    #[test]
    fn test_response_rf_gain() {
        let r = Response::RfGain(200);
        assert_eq!(r, Response::RfGain(200));
    }

    #[test]
    fn test_response_squelch() {
        let r = Response::Squelch(50);
        assert_eq!(r, Response::Squelch(50));
    }

    #[test]
    fn test_response_power() {
        let r = Response::Power(100);
        assert_eq!(r, Response::Power(100));
    }

    #[test]
    fn test_response_clone() {
        let r = Response::RadioId(18);
        let r2 = r.clone();
        assert_eq!(r, r2);
    }

    #[test]
    fn test_information_response_fields() {
        let info = InformationResponse {
            frequency: Frequency::new(14_230_000).unwrap(),
            step: 1000,
            rit_xit_offset: -500,
            rit_enabled: true,
            xit_enabled: false,
            memory_bank: 0,
            memory_channel: 0,
            tx_rx: false,
            mode: Mode::Usb,
            vfo_memory: 0,
            scan_status: 0,
            split: false,
            ctcss_tone: 0,
            tone_number: 0,
        };
        assert_eq!(info.frequency, Frequency::new(14_230_000).unwrap());
        assert_eq!(info.mode, Mode::Usb);
        assert!(info.rit_enabled);
        assert!(!info.xit_enabled);
        assert_eq!(info.rit_xit_offset, -500);
    }
}
