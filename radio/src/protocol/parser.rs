//! Response parser for TS-570D CAT protocol responses.
//!
//! [`ResponseParser`] converts raw semicolon-terminated strings (as returned
//! by the radio) into typed [`Response`] values.

use framework::radio::{Frequency, Mode, RadioError, RadioResult};
use crate::protocol::response::{InformationResponse, Response};

/// Parses raw TS-570D response strings into typed [`Response`] values.
///
/// The parser is stateless; every call to [`parse`](ResponseParser::parse)
/// is independent.
pub struct ResponseParser;

impl ResponseParser {
    /// Parse a single raw response string into a [`Response`].
    ///
    /// The input may or may not carry a trailing `';'` — both are accepted.
    ///
    /// # Errors
    ///
    /// Returns [`RadioError::InvalidProtocolString`] when the string is too
    /// short, has an unrecognised command code, or contains malformed field
    /// data.
    pub fn parse(raw: &str) -> RadioResult<Response> {
        // Strip optional trailing semicolon.
        let raw = raw.trim_end_matches(';');

        // `?` is the TS-570D error reply.
        if raw == "?" {
            return Ok(Response::Error);
        }

        if raw.len() < 2 {
            return Err(RadioError::InvalidProtocolString(raw.to_string()));
        }

        let (code, params) = raw.split_at(2);

        match code {
            "FA" => Self::parse_frequency(params).map(Response::VfoAFrequency),
            "FB" => Self::parse_frequency(params).map(Response::VfoBFrequency),
            "MD" => Self::parse_mode(params),
            "ID" => Self::parse_radio_id(params),
            "IF" => Self::parse_information(params),
            "SM" => Self::parse_smeter(params),
            "AG" => Self::parse_af_gain(params),
            "RG" => Self::parse_rf_gain(params),
            "SQ" => Self::parse_squelch(params),
            "PC" => Self::parse_power(params),
            "NB" => Self::parse_one_digit_bool(params).map(Response::NoiseBlanker),
            "NR" => Self::parse_one_digit_u8(params).map(Response::NoiseReduction),
            "PA" => Self::parse_one_digit_bool(params).map(Response::Preamp),
            "RA" => Self::parse_two_digit_bool(params).map(Response::Attenuator),
            "MG" => Self::parse_three_digit_u8(params).map(Response::MicGain),
            "GT" => Self::parse_three_digit_u8(params).map(Response::Agc),
            "RT" => Self::parse_one_digit_bool(params).map(Response::Rit),
            "XT" => Self::parse_one_digit_bool(params).map(Response::Xit),
            "SC" => Self::parse_one_digit_bool(params).map(Response::Scan),
            "VX" => Self::parse_one_digit_bool(params).map(Response::Vox),
            "VG" => Self::parse_three_digit_u8(params).map(Response::VoxGain),
            "VD" => Self::parse_four_digit_u16(params).map(Response::VoxDelay),
            "FR" => Self::parse_one_digit_u8(params).map(Response::RxVfo),
            "FT" => Self::parse_one_digit_u8(params).map(Response::TxVfo),
            "LK" => Self::parse_one_digit_bool(params).map(Response::FrequencyLock),
            "PS" => Self::parse_one_digit_bool(params).map(Response::PowerOn),
            "BY" => Self::parse_one_digit_bool(params).map(Response::Busy),
            "PR" => Self::parse_one_digit_bool(params).map(Response::SpeechProcessor),
            "MC" => Self::parse_memory_channel(params),
            "AN" => Self::parse_one_digit_u8(params).map(Response::Antenna),
            "CN" => Self::parse_two_digit_u8(params).map(Response::CtcssTone),
            "CT" => Self::parse_one_digit_bool(params).map(Response::Ctcss),
            "TN" => Self::parse_two_digit_u8(params).map(Response::ToneNumber),
            "TO" => Self::parse_one_digit_bool(params).map(Response::Tone),
            "BC" => Self::parse_one_digit_u8(params).map(Response::BeatCancel),
            "IS" => Self::parse_if_shift(params),
            "KS" => Self::parse_three_digit_u8(params).map(Response::KeyerSpeed),
            "PT" => Self::parse_two_digit_u8(params).map(Response::CwPitch),
            "RM" => Self::parse_meter(params),
            "SD" => Self::parse_four_digit_u16(params).map(Response::SemiBreakInDelay),
            "CA" => Self::parse_one_digit_bool(params).map(Response::CwAutoZerobeat),
            "FS" => Self::parse_one_digit_bool(params).map(Response::FineStep),
            _ => Err(RadioError::InvalidProtocolString(raw.to_string())),
        }
    }

    // -----------------------------------------------------------------------
    // Per-command parsers
    // -----------------------------------------------------------------------

    fn parse_frequency(params: &str) -> RadioResult<Frequency> {
        if params.len() != 11 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        Frequency::from_protocol_str(params)
    }

    fn parse_mode(params: &str) -> RadioResult<Response> {
        if params.len() != 1 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        let byte = params
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        let mode = Mode::try_from(byte)?;
        Ok(Response::Mode(mode))
    }

    fn parse_radio_id(params: &str) -> RadioResult<Response> {
        if params.len() != 3 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        let id = params
            .parse::<u16>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        Ok(Response::RadioId(id))
    }

    /// Parse the `SM` (S-meter) response.
    ///
    /// Wire format: `SM<sel><reading>` where `<sel>` is 1 digit (0/1) and
    /// `<reading>` is 4 digits (0000–0030).
    fn parse_smeter(params: &str) -> RadioResult<Response> {
        if params.len() != 5 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        let sel = params[..1]
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        let reading = params[1..]
            .parse::<u16>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        Ok(Response::SMeter(sel, reading))
    }

    /// Parse the `AG` (AF gain) response.
    ///
    /// Wire format: `AG<sel><level>` where `<sel>` is 1 digit (0/1) and
    /// `<level>` is 3 digits (000–255).
    fn parse_af_gain(params: &str) -> RadioResult<Response> {
        if params.len() != 4 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        let sel = params[..1]
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        let level = params[1..]
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        Ok(Response::AfGain(sel, level))
    }

    /// Parse the `RG` (RF gain) response.
    ///
    /// Wire format: `RG<level>` where `<level>` is 3 digits (000–255).
    fn parse_rf_gain(params: &str) -> RadioResult<Response> {
        if params.len() != 3 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        let level = params
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        Ok(Response::RfGain(level))
    }

    /// Parse the `SQ` (squelch) response.
    ///
    /// Wire format: `SQ<level>` where `<level>` is 3 digits (000–255).
    fn parse_squelch(params: &str) -> RadioResult<Response> {
        if params.len() != 3 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        let level = params
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        Ok(Response::Squelch(level))
    }

    /// Parse the `PC` (transmit power) response.
    ///
    /// Wire format: `PC<level>` where `<level>` is 3 digits (005–100).
    fn parse_power(params: &str) -> RadioResult<Response> {
        if params.len() != 3 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        let level = params
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        Ok(Response::Power(level))
    }

    /// Parse the composite `IF` (Information) response.
    ///
    /// The payload after `IF` is exactly 37 characters long.
    ///
    /// ```text
    /// Pos  Len  Field
    ///   0   11  frequency (Hz, zero-padded)
    ///  11    4  step
    ///  15    5  RIT/XIT offset (signed, e.g. "+0500" or "-0500")
    ///  20    1  RIT enabled (0/1)
    ///  21    1  XIT enabled (0/1)
    ///  22    2  memory bank
    ///  24    2  memory channel
    ///  26    1  TX/RX (0=RX, 1=TX)
    ///  27    1  mode (1–9)
    ///  28    1  VFO/memory (0=VFO, 1=Memory)
    ///  29    1  scan status
    ///  30    1  split
    ///  31    2  CTCSS tone number
    ///  33    2  tone number
    ///  35    1  offset indicator (always 0 on TS-570D)
    ///  36    1  (last position — not used)
    /// ```
    fn parse_information(params: &str) -> RadioResult<Response> {
        // The TS-570D IF payload is 37 characters.
        if params.len() < 37 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }

        let frequency = Frequency::from_protocol_str(&params[0..11])?;

        let step = params[11..15]
            .parse::<u32>()
            .map_err(|_| RadioError::InvalidProtocolString(params[11..15].to_string()))?;

        // RIT/XIT offset: 5 chars, format is sign + 4 digits, e.g. "+0500" or "-0500"
        let rit_xit_offset = Self::parse_signed_offset(&params[15..20])?;

        let rit_enabled = &params[20..21] != "0";
        let xit_enabled = &params[21..22] != "0";

        let memory_bank = params[22..24].trim().parse::<u8>().unwrap_or(0);

        let memory_channel = params[24..26]
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params[24..26].to_string()))?;

        let tx_rx = &params[26..27] != "0";

        let mode_byte = params[27..28]
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params[27..28].to_string()))?;
        let mode = Mode::try_from(mode_byte)?;

        let vfo_memory = params[28..29]
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params[28..29].to_string()))?;

        let scan_status = params[29..30]
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params[29..30].to_string()))?;

        let split = &params[30..31] != "0";

        let ctcss_tone = params[31..33]
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params[31..33].to_string()))?;

        let tone_number = params[33..35]
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params[33..35].to_string()))?;

        Ok(Response::Information(InformationResponse {
            frequency,
            step,
            rit_xit_offset,
            rit_enabled,
            xit_enabled,
            memory_bank,
            memory_channel,
            tx_rx,
            mode,
            vfo_memory,
            scan_status,
            split,
            ctcss_tone,
            tone_number,
        }))
    }

    // -----------------------------------------------------------------------
    // Generic helper parsers
    // -----------------------------------------------------------------------

    /// Parse a single-digit (0/1) as bool.
    fn parse_one_digit_bool(params: &str) -> RadioResult<bool> {
        if params.len() != 1 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        let v = params
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        Ok(v != 0)
    }

    /// Parse a single-digit as u8.
    fn parse_one_digit_u8(params: &str) -> RadioResult<u8> {
        if params.len() != 1 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        params
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))
    }

    /// Parse two digits as u8.
    fn parse_two_digit_u8(params: &str) -> RadioResult<u8> {
        if params.len() != 2 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        params
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))
    }

    /// Parse two digits as bool (00=false, 01=true).
    fn parse_two_digit_bool(params: &str) -> RadioResult<bool> {
        if params.len() != 2 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        let v = params
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        Ok(v != 0)
    }

    /// Parse three digits as u8.
    fn parse_three_digit_u8(params: &str) -> RadioResult<u8> {
        if params.len() != 3 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        params
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))
    }

    /// Parse four digits as u16.
    fn parse_four_digit_u16(params: &str) -> RadioResult<u16> {
        if params.len() != 4 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        params
            .parse::<u16>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))
    }

    /// Parse the `MC` (memory channel) response.
    ///
    /// Wire format: `MC <NN>` where a space may or may not be present.
    /// We accept 2 or 3 chars (with or without leading space).
    fn parse_memory_channel(params: &str) -> RadioResult<Response> {
        let trimmed = params.trim();
        if trimmed.is_empty() || trimmed.len() > 3 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        let ch = trimmed
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        Ok(Response::MemoryChannel(ch))
    }

    /// Parse the `IS` (IF shift) response.
    ///
    /// Wire format: `IS<direction><freq:04>` where direction is `+` or `-` and
    /// freq is 4 digits.
    fn parse_if_shift(params: &str) -> RadioResult<Response> {
        if params.len() != 5 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        let direction = params
            .chars()
            .next()
            .ok_or_else(|| RadioError::InvalidProtocolString(params.to_string()))?;
        if direction != '+' && direction != '-' {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        let freq = params[1..]
            .parse::<u16>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        Ok(Response::IfShift(direction, freq))
    }

    /// Parse the `RM` (meter) response.
    ///
    /// Wire format: `RM<type><value:04>` where type is 1 digit and value is 4 digits.
    fn parse_meter(params: &str) -> RadioResult<Response> {
        if params.len() != 5 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        let meter_type = params[..1]
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        let value = params[1..]
            .parse::<u16>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        Ok(Response::Meter(meter_type, value))
    }

    /// Parse a signed 5-character offset string, e.g. `"+0500"` or `"-0500"`.
    fn parse_signed_offset(s: &str) -> RadioResult<i32> {
        if s.len() != 5 {
            return Err(RadioError::InvalidProtocolString(s.to_string()));
        }
        let (sign, digits) = s.split_at(1);
        let magnitude = digits
            .parse::<i32>()
            .map_err(|_| RadioError::InvalidProtocolString(s.to_string()))?;
        match sign {
            "+" => Ok(magnitude),
            "-" => Ok(-magnitude),
            _ => Err(RadioError::InvalidProtocolString(s.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::response::{InformationResponse, Response};

    // -----------------------------------------------------------------------
    // Error response
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_error_response_with_semicolon() {
        assert_eq!(ResponseParser::parse("?;").unwrap(), Response::Error);
    }

    #[test]
    fn test_parse_error_response_without_semicolon() {
        assert_eq!(ResponseParser::parse("?").unwrap(), Response::Error);
    }

    // -----------------------------------------------------------------------
    // FA — VFO A frequency
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_fa_frequency() {
        let resp = ResponseParser::parse("FA00014230000;").unwrap();
        assert_eq!(
            resp,
            Response::VfoAFrequency(Frequency::new(14_230_000).unwrap())
        );
    }

    #[test]
    fn test_parse_fa_frequency_min() {
        let resp = ResponseParser::parse("FA00000500000;").unwrap();
        assert_eq!(
            resp,
            Response::VfoAFrequency(Frequency::new(500_000).unwrap())
        );
    }

    #[test]
    fn test_parse_fa_wrong_length() {
        let result = ResponseParser::parse("FA001;");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // FB — VFO B frequency
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_fb_frequency() {
        let resp = ResponseParser::parse("FB00007100000;").unwrap();
        assert_eq!(
            resp,
            Response::VfoBFrequency(Frequency::new(7_100_000).unwrap())
        );
    }

    // -----------------------------------------------------------------------
    // MD — mode
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_md_usb() {
        let resp = ResponseParser::parse("MD2;").unwrap();
        assert_eq!(resp, Response::Mode(Mode::Usb));
    }

    #[test]
    fn test_parse_md_lsb() {
        let resp = ResponseParser::parse("MD1;").unwrap();
        assert_eq!(resp, Response::Mode(Mode::Lsb));
    }

    #[test]
    fn test_parse_md_cw_reverse() {
        let resp = ResponseParser::parse("MD7;").unwrap();
        assert_eq!(resp, Response::Mode(Mode::CwReverse));
    }

    #[test]
    fn test_parse_md_invalid_mode() {
        let result = ResponseParser::parse("MD8;");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // ID — radio identifier
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_id_ts570d() {
        let resp = ResponseParser::parse("ID018;").unwrap();
        assert_eq!(resp, Response::RadioId(18));
    }

    #[test]
    fn test_parse_id_ts570s() {
        let resp = ResponseParser::parse("ID019;").unwrap();
        assert_eq!(resp, Response::RadioId(19));
    }

    #[test]
    fn test_parse_id_wrong_length() {
        let result = ResponseParser::parse("ID18;");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // SM — S-meter
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_sm_main() {
        let resp = ResponseParser::parse("SM00015;").unwrap();
        assert_eq!(resp, Response::SMeter(0, 15));
    }

    #[test]
    fn test_parse_sm_sub() {
        let resp = ResponseParser::parse("SM10030;").unwrap();
        assert_eq!(resp, Response::SMeter(1, 30));
    }

    #[test]
    fn test_parse_sm_zero() {
        let resp = ResponseParser::parse("SM00000;").unwrap();
        assert_eq!(resp, Response::SMeter(0, 0));
    }

    // -----------------------------------------------------------------------
    // AG — AF gain
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_ag_main() {
        let resp = ResponseParser::parse("AG0128;").unwrap();
        assert_eq!(resp, Response::AfGain(0, 128));
    }

    #[test]
    fn test_parse_ag_sub() {
        let resp = ResponseParser::parse("AG1200;").unwrap();
        assert_eq!(resp, Response::AfGain(1, 200));
    }

    // -----------------------------------------------------------------------
    // RG — RF gain
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_rg() {
        let resp = ResponseParser::parse("RG255;").unwrap();
        assert_eq!(resp, Response::RfGain(255));
    }

    #[test]
    fn test_parse_rg_zero() {
        let resp = ResponseParser::parse("RG000;").unwrap();
        assert_eq!(resp, Response::RfGain(0));
    }

    // -----------------------------------------------------------------------
    // SQ — squelch
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_sq_level() {
        let resp = ResponseParser::parse("SQ050;").unwrap();
        assert_eq!(resp, Response::Squelch(50));
    }

    #[test]
    fn test_parse_sq_max() {
        let resp = ResponseParser::parse("SQ255;").unwrap();
        assert_eq!(resp, Response::Squelch(255));
    }

    #[test]
    fn test_parse_sq_zero() {
        let resp = ResponseParser::parse("SQ000;").unwrap();
        assert_eq!(resp, Response::Squelch(0));
    }

    // -----------------------------------------------------------------------
    // PC — power
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_pc_max() {
        let resp = ResponseParser::parse("PC100;").unwrap();
        assert_eq!(resp, Response::Power(100));
    }

    #[test]
    fn test_parse_pc_min() {
        let resp = ResponseParser::parse("PC005;").unwrap();
        assert_eq!(resp, Response::Power(5));
    }

    // -----------------------------------------------------------------------
    // IF — composite information
    // -----------------------------------------------------------------------

    /// Build a valid IF payload string for test use.
    ///
    /// Layout (37 chars):
    ///   [0..11]  frequency
    ///   [11..15] step (4 chars)
    ///   [15..20] rit/xit offset (+NNNN or -NNNN)
    ///   [20]     rit enabled
    ///   [21]     xit enabled
    ///   [22..24] memory bank (2 chars)
    ///   [24..26] memory channel (2 chars)
    ///   [26]     tx/rx
    ///   [27]     mode
    ///   [28]     vfo/memory
    ///   [29]     scan status
    ///   [30]     split
    ///   [31..33] ctcss tone (2 chars)
    ///   [33..35] tone number (2 chars)
    ///   [35]     offset indicator
    ///   [36]     (reserved / unused)
    fn make_if_string(
        freq_hz: u64,
        step: u32,
        rit_xit_offset: i32,
        rit_enabled: bool,
        xit_enabled: bool,
        memory_bank: u8,
        memory_channel: u8,
        tx: bool,
        mode: u8,
        vfo_memory: u8,
        scan: u8,
        split: bool,
        ctcss_tone: u8,
        tone_number: u8,
    ) -> String {
        let sign = if rit_xit_offset >= 0 { '+' } else { '-' };
        let offset_abs = rit_xit_offset.unsigned_abs();
        format!(
            "IF{:011}{:04}{}{:04}{}{}{:02}{:02}{}{}{}{}{}{:02}{:02}00;",
            freq_hz,
            step,
            sign,
            offset_abs,
            if rit_enabled { '1' } else { '0' },
            if xit_enabled { '1' } else { '0' },
            memory_bank,
            memory_channel,
            if tx { '1' } else { '0' },
            mode,
            vfo_memory,
            scan,
            if split { '1' } else { '0' },
            ctcss_tone,
            tone_number,
        )
    }

    #[test]
    fn test_parse_if_basic() {
        let raw = make_if_string(
            14_230_000, 1000, 0, false, false, 0, 0, false, 2, 0, 0, false, 0, 0,
        );
        let resp = ResponseParser::parse(&raw).unwrap();
        match resp {
            Response::Information(info) => {
                assert_eq!(info.frequency, Frequency::new(14_230_000).unwrap());
                assert_eq!(info.step, 1000);
                assert_eq!(info.rit_xit_offset, 0);
                assert!(!info.rit_enabled);
                assert!(!info.xit_enabled);
                assert_eq!(info.mode, Mode::Usb);
                assert!(!info.tx_rx);
                assert!(!info.split);
            }
            other => panic!("expected Information, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_if_with_rit_offset() {
        let raw = make_if_string(
            7_100_000, 100, 500, true, false, 0, 0, false, 1, 0, 0, false, 0, 0,
        );
        let resp = ResponseParser::parse(&raw).unwrap();
        match resp {
            Response::Information(info) => {
                assert_eq!(info.frequency, Frequency::new(7_100_000).unwrap());
                assert_eq!(info.rit_xit_offset, 500);
                assert!(info.rit_enabled);
                assert!(!info.xit_enabled);
                assert_eq!(info.mode, Mode::Lsb);
            }
            other => panic!("expected Information, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_if_negative_rit_offset() {
        let raw = make_if_string(
            14_100_000, 0, -200, true, false, 0, 0, false, 3, 0, 0, false, 0, 0,
        );
        let resp = ResponseParser::parse(&raw).unwrap();
        match resp {
            Response::Information(info) => {
                assert_eq!(info.rit_xit_offset, -200);
                assert_eq!(info.mode, Mode::Cw);
            }
            other => panic!("expected Information, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_if_transmitting() {
        let raw = make_if_string(
            14_230_000, 0, 0, false, false, 0, 0, true, 2, 0, 0, false, 0, 0,
        );
        let resp = ResponseParser::parse(&raw).unwrap();
        match resp {
            Response::Information(info) => {
                assert!(info.tx_rx);
            }
            other => panic!("expected Information, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_if_split_and_ctcss() {
        let raw = make_if_string(
            14_230_000, 0, 0, false, false, 0, 0, false, 2, 0, 0, true, 7, 3,
        );
        let resp = ResponseParser::parse(&raw).unwrap();
        match resp {
            Response::Information(info) => {
                assert!(info.split);
                assert_eq!(info.ctcss_tone, 7);
                assert_eq!(info.tone_number, 3);
            }
            other => panic!("expected Information, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_if_too_short() {
        let result = ResponseParser::parse("IF0001423000;");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // NB — noise blanker
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_nb_on() {
        let resp = ResponseParser::parse("NB1;").unwrap();
        assert_eq!(resp, Response::NoiseBlanker(true));
    }

    #[test]
    fn test_parse_nb_off() {
        let resp = ResponseParser::parse("NB0;").unwrap();
        assert_eq!(resp, Response::NoiseBlanker(false));
    }

    // -----------------------------------------------------------------------
    // NR — noise reduction
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_nr_off() {
        let resp = ResponseParser::parse("NR0;").unwrap();
        assert_eq!(resp, Response::NoiseReduction(0));
    }

    #[test]
    fn test_parse_nr1() {
        let resp = ResponseParser::parse("NR1;").unwrap();
        assert_eq!(resp, Response::NoiseReduction(1));
    }

    #[test]
    fn test_parse_nr2() {
        let resp = ResponseParser::parse("NR2;").unwrap();
        assert_eq!(resp, Response::NoiseReduction(2));
    }

    // -----------------------------------------------------------------------
    // PA — pre-amplifier
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_pa_on() {
        let resp = ResponseParser::parse("PA1;").unwrap();
        assert_eq!(resp, Response::Preamp(true));
    }

    #[test]
    fn test_parse_pa_off() {
        let resp = ResponseParser::parse("PA0;").unwrap();
        assert_eq!(resp, Response::Preamp(false));
    }

    // -----------------------------------------------------------------------
    // RA — attenuator
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_ra_off() {
        let resp = ResponseParser::parse("RA00;").unwrap();
        assert_eq!(resp, Response::Attenuator(false));
    }

    #[test]
    fn test_parse_ra_on() {
        let resp = ResponseParser::parse("RA01;").unwrap();
        assert_eq!(resp, Response::Attenuator(true));
    }

    // -----------------------------------------------------------------------
    // MG — mic gain
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_mg() {
        let resp = ResponseParser::parse("MG050;").unwrap();
        assert_eq!(resp, Response::MicGain(50));
    }

    // -----------------------------------------------------------------------
    // GT — AGC
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_gt_fast() {
        let resp = ResponseParser::parse("GT002;").unwrap();
        assert_eq!(resp, Response::Agc(2));
    }

    #[test]
    fn test_parse_gt_slow() {
        let resp = ResponseParser::parse("GT004;").unwrap();
        assert_eq!(resp, Response::Agc(4));
    }

    // -----------------------------------------------------------------------
    // RT — RIT
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_rt_on() {
        let resp = ResponseParser::parse("RT1;").unwrap();
        assert_eq!(resp, Response::Rit(true));
    }

    #[test]
    fn test_parse_rt_off() {
        let resp = ResponseParser::parse("RT0;").unwrap();
        assert_eq!(resp, Response::Rit(false));
    }

    // -----------------------------------------------------------------------
    // XT — XIT
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_xt_on() {
        let resp = ResponseParser::parse("XT1;").unwrap();
        assert_eq!(resp, Response::Xit(true));
    }

    // -----------------------------------------------------------------------
    // SC — scan
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_sc_on() {
        let resp = ResponseParser::parse("SC1;").unwrap();
        assert_eq!(resp, Response::Scan(true));
    }

    #[test]
    fn test_parse_sc_off() {
        let resp = ResponseParser::parse("SC0;").unwrap();
        assert_eq!(resp, Response::Scan(false));
    }

    // -----------------------------------------------------------------------
    // VX — VOX
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_vx_on() {
        let resp = ResponseParser::parse("VX1;").unwrap();
        assert_eq!(resp, Response::Vox(true));
    }

    // -----------------------------------------------------------------------
    // VG — VOX gain
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_vg() {
        let resp = ResponseParser::parse("VG005;").unwrap();
        assert_eq!(resp, Response::VoxGain(5));
    }

    // -----------------------------------------------------------------------
    // VD — VOX delay
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_vd() {
        let resp = ResponseParser::parse("VD1500;").unwrap();
        assert_eq!(resp, Response::VoxDelay(1500));
    }

    #[test]
    fn test_parse_vd_zero() {
        let resp = ResponseParser::parse("VD0000;").unwrap();
        assert_eq!(resp, Response::VoxDelay(0));
    }

    // -----------------------------------------------------------------------
    // FR — RX VFO
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_fr_vfo_a() {
        let resp = ResponseParser::parse("FR0;").unwrap();
        assert_eq!(resp, Response::RxVfo(0));
    }

    #[test]
    fn test_parse_fr_vfo_b() {
        let resp = ResponseParser::parse("FR1;").unwrap();
        assert_eq!(resp, Response::RxVfo(1));
    }

    // -----------------------------------------------------------------------
    // FT — TX VFO
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_ft_vfo_a() {
        let resp = ResponseParser::parse("FT0;").unwrap();
        assert_eq!(resp, Response::TxVfo(0));
    }

    // -----------------------------------------------------------------------
    // LK — frequency lock
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_lk_on() {
        let resp = ResponseParser::parse("LK1;").unwrap();
        assert_eq!(resp, Response::FrequencyLock(true));
    }

    #[test]
    fn test_parse_lk_off() {
        let resp = ResponseParser::parse("LK0;").unwrap();
        assert_eq!(resp, Response::FrequencyLock(false));
    }

    // -----------------------------------------------------------------------
    // PS — power on/off
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_ps_on() {
        let resp = ResponseParser::parse("PS1;").unwrap();
        assert_eq!(resp, Response::PowerOn(true));
    }

    // -----------------------------------------------------------------------
    // BY — busy
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_by_busy() {
        let resp = ResponseParser::parse("BY1;").unwrap();
        assert_eq!(resp, Response::Busy(true));
    }

    #[test]
    fn test_parse_by_not_busy() {
        let resp = ResponseParser::parse("BY0;").unwrap();
        assert_eq!(resp, Response::Busy(false));
    }

    // -----------------------------------------------------------------------
    // PR — speech processor
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_pr_on() {
        let resp = ResponseParser::parse("PR1;").unwrap();
        assert_eq!(resp, Response::SpeechProcessor(true));
    }

    // -----------------------------------------------------------------------
    // MC — memory channel
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_mc_channel_5() {
        let resp = ResponseParser::parse("MC 05;").unwrap();
        assert_eq!(resp, Response::MemoryChannel(5));
    }

    #[test]
    fn test_parse_mc_channel_99() {
        let resp = ResponseParser::parse("MC99;").unwrap();
        assert_eq!(resp, Response::MemoryChannel(99));
    }

    // -----------------------------------------------------------------------
    // AN — antenna
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_an_ant1() {
        let resp = ResponseParser::parse("AN1;").unwrap();
        assert_eq!(resp, Response::Antenna(1));
    }

    #[test]
    fn test_parse_an_ant2() {
        let resp = ResponseParser::parse("AN2;").unwrap();
        assert_eq!(resp, Response::Antenna(2));
    }

    // -----------------------------------------------------------------------
    // CN — CTCSS tone number
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_cn() {
        let resp = ResponseParser::parse("CN07;").unwrap();
        assert_eq!(resp, Response::CtcssTone(7));
    }

    // -----------------------------------------------------------------------
    // CT — CTCSS on/off
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_ct_on() {
        let resp = ResponseParser::parse("CT1;").unwrap();
        assert_eq!(resp, Response::Ctcss(true));
    }

    // -----------------------------------------------------------------------
    // TN — tone number
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_tn() {
        let resp = ResponseParser::parse("TN03;").unwrap();
        assert_eq!(resp, Response::ToneNumber(3));
    }

    // -----------------------------------------------------------------------
    // TO — tone on/off
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_to_on() {
        let resp = ResponseParser::parse("TO1;").unwrap();
        assert_eq!(resp, Response::Tone(true));
    }

    // -----------------------------------------------------------------------
    // BC — beat cancel
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_bc_off() {
        let resp = ResponseParser::parse("BC0;").unwrap();
        assert_eq!(resp, Response::BeatCancel(0));
    }

    #[test]
    fn test_parse_bc_enhanced() {
        let resp = ResponseParser::parse("BC2;").unwrap();
        assert_eq!(resp, Response::BeatCancel(2));
    }

    // -----------------------------------------------------------------------
    // IS — IF shift
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_is_positive() {
        let resp = ResponseParser::parse("IS+0500;").unwrap();
        assert_eq!(resp, Response::IfShift('+', 500));
    }

    #[test]
    fn test_parse_is_negative() {
        let resp = ResponseParser::parse("IS-0200;").unwrap();
        assert_eq!(resp, Response::IfShift('-', 200));
    }

    // -----------------------------------------------------------------------
    // KS — keyer speed
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_ks() {
        let resp = ResponseParser::parse("KS025;").unwrap();
        assert_eq!(resp, Response::KeyerSpeed(25));
    }

    // -----------------------------------------------------------------------
    // PT — CW pitch
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_pt() {
        let resp = ResponseParser::parse("PT06;").unwrap();
        assert_eq!(resp, Response::CwPitch(6));
    }

    // -----------------------------------------------------------------------
    // RM — meter
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_rm_swr() {
        let resp = ResponseParser::parse("RM10050;").unwrap();
        assert_eq!(resp, Response::Meter(1, 50));
    }

    // -----------------------------------------------------------------------
    // SD — semi break-in delay
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_sd() {
        let resp = ResponseParser::parse("SD0200;").unwrap();
        assert_eq!(resp, Response::SemiBreakInDelay(200));
    }

    // -----------------------------------------------------------------------
    // CA — CW auto zero-beat
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_ca_on() {
        let resp = ResponseParser::parse("CA1;").unwrap();
        assert_eq!(resp, Response::CwAutoZerobeat(true));
    }

    // -----------------------------------------------------------------------
    // FS — fine step
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_fs_on() {
        let resp = ResponseParser::parse("FS1;").unwrap();
        assert_eq!(resp, Response::FineStep(true));
    }

    // -----------------------------------------------------------------------
    // Unknown command code
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_unknown_code() {
        let result = ResponseParser::parse("ZZ0;");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_too_short() {
        let result = ResponseParser::parse("X;");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty() {
        let result = ResponseParser::parse("");
        assert!(result.is_err());
    }
}
