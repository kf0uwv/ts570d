//! Response parser for TS-570D CAT protocol responses.
//!
//! [`ResponseParser`] converts raw semicolon-terminated strings (as returned
//! by the radio) into typed [`Response`] values.

use crate::error::{RadioError, RadioResult};
use crate::protocol::response::{InformationResponse, Response};
use crate::protocol::{Frequency, Mode};

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
            "TX" => Ok(Response::TxRxStatus(true)),
            "RX" => Ok(Response::TxRxStatus(false)),
            "AI" => Self::parse_auto_info(params),
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
    /// Wire format: `SQ<sel><level>` where `<sel>` is 1 digit (0/1) and
    /// `<level>` is 3 digits (000–255).
    fn parse_squelch(params: &str) -> RadioResult<Response> {
        if params.len() != 4 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        let sel = params[..1]
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        let level = params[1..]
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        Ok(Response::Squelch(sel, level))
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

    /// Parse the `AI` (auto-information) response.
    ///
    /// Wire format: `AI<state>` where `<state>` is a single digit (0=off, 1=on).
    fn parse_auto_info(params: &str) -> RadioResult<Response> {
        if params.len() != 1 {
            return Err(RadioError::InvalidProtocolString(params.to_string()));
        }
        let state = params
            .parse::<u8>()
            .map_err(|_| RadioError::InvalidProtocolString(params.to_string()))?;
        Ok(Response::AutoInfo(state != 0))
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
    use crate::protocol::{Frequency, Mode};

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
    fn test_parse_sq_main() {
        let resp = ResponseParser::parse("SQ0050;").unwrap();
        assert_eq!(resp, Response::Squelch(0, 50));
    }

    #[test]
    fn test_parse_sq_sub() {
        let resp = ResponseParser::parse("SQ1100;").unwrap();
        assert_eq!(resp, Response::Squelch(1, 100));
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
    // TX / RX — TX/RX status
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_tx() {
        let resp = ResponseParser::parse("TX;").unwrap();
        assert_eq!(resp, Response::TxRxStatus(true));
    }

    #[test]
    fn test_parse_rx() {
        let resp = ResponseParser::parse("RX;").unwrap();
        assert_eq!(resp, Response::TxRxStatus(false));
    }

    // -----------------------------------------------------------------------
    // AI — auto-information
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_ai_on() {
        let resp = ResponseParser::parse("AI1;").unwrap();
        assert_eq!(resp, Response::AutoInfo(true));
    }

    #[test]
    fn test_parse_ai_off() {
        let resp = ResponseParser::parse("AI0;").unwrap();
        assert_eq!(resp, Response::AutoInfo(false));
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
