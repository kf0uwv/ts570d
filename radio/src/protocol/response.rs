//! Typed response variants for the TS-570D CAT protocol.
//!
//! The [`Response`] enum wraps every distinct response the radio can emit.
//! [`InformationResponse`] represents the composite `IF` response that
//! packs the full radio state into a single message.

use crate::protocol::{Frequency, Mode};

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
    /// SQ — squelch level.  Fields: (main_sub selector 0/1, level 0–255)
    Squelch(u8, u8),
    /// PC — transmit power (5–100 W)
    Power(u8),
    /// TX/RX — transmit/receive status.  `true` = transmitting
    TxRxStatus(bool),
    /// AI — auto-information mode enabled
    AutoInfo(bool),
    /// `?;` error response
    Error,
}

/// Composite response to the `IF` (Information) query.
///
/// The TS-570D packs the full operational state into a single 37-character
/// payload (plus command code and terminator).
///
/// Wire layout (chars after `IF`, before `;`):
/// ```text
/// Pos  Len  Field
///   0   11  frequency (Hz, zero-padded)
///  11    4  step (Hz, 4-digit, not always used)
///  15    5  RIT/XIT offset (-9999 to +9999 Hz, signed, leading sign)
///  20    1  RIT enabled (0/1)
///  21    1  XIT enabled (0/1)
///  22    2  memory bank (00–09, or spaces if in VFO mode)
///  24    2  memory channel (00–99)
///  26    1  TX/RX status (0=RX, 1=TX)
///  27    1  mode (1–9)
///  28    1  VFO/memory (0=VFO, 1=Memory)
///  29    1  scan status (0=off, 1=on)
///  30    1  split (0=off, 1=on)
///  31    2  CTCSS tone number (00–42)
///  33    2  tone number (00–42)
///  35    1  offset (not used on TS-570D, always 0)
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct InformationResponse {
    /// Current VFO frequency in Hz
    pub frequency: Frequency,
    /// Tuning step in Hz
    pub step: u32,
    /// RIT/XIT offset in Hz (signed)
    pub rit_xit_offset: i32,
    /// RIT enabled
    pub rit_enabled: bool,
    /// XIT enabled
    pub xit_enabled: bool,
    /// Memory bank (0–9; 0 when in VFO mode)
    pub memory_bank: u8,
    /// Memory channel (0–99; 0 when in VFO mode)
    pub memory_channel: u8,
    /// `true` = transmitting
    pub tx_rx: bool,
    /// Operating mode
    pub mode: Mode,
    /// `0` = VFO mode, `1` = memory mode
    pub vfo_memory: u8,
    /// Scan status (`0` = off, `1` = scanning)
    pub scan_status: u8,
    /// Split operation enabled
    pub split: bool,
    /// CTCSS tone number (0–42)
    pub ctcss_tone: u8,
    /// Tone number (0–42)
    pub tone_number: u8,
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
        let r = Response::Squelch(0, 50);
        assert_eq!(r, Response::Squelch(0, 50));
    }

    #[test]
    fn test_response_power() {
        let r = Response::Power(100);
        assert_eq!(r, Response::Power(100));
    }

    #[test]
    fn test_response_txrx_status_rx() {
        let r = Response::TxRxStatus(false);
        assert_eq!(r, Response::TxRxStatus(false));
    }

    #[test]
    fn test_response_txrx_status_tx() {
        let r = Response::TxRxStatus(true);
        assert_eq!(r, Response::TxRxStatus(true));
    }

    #[test]
    fn test_response_auto_info() {
        let r = Response::AutoInfo(true);
        assert_eq!(r, Response::AutoInfo(true));
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
