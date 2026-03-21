use crate::radio_state::RadioState;

/// Parse and handle a single CAT command string (without trailing `;`).
///
/// Returns the complete response string including the trailing `;`, ready to
/// be written back to the serial port.  Unknown or malformed commands return
/// `"?;"` as the TS-570D does.
pub fn handle(cmd: &str, state: &mut RadioState) -> String {
    // Commands are at least 2 characters (the 2-letter code).
    if cmd.len() < 2 {
        return "?;".to_string();
    }

    let code = &cmd[..2].to_ascii_uppercase();
    let params = &cmd[2..];

    match code.as_str() {
        // ------------------------------------------------------------------
        // FA — VFO A Frequency
        // ------------------------------------------------------------------
        "FA" => {
            if params.is_empty() {
                // Query
                format!("FA{:011};", state.vfo_a_hz)
            } else if params.len() == 11 {
                // Set
                if let Ok(hz) = params.parse::<u64>() {
                    state.vfo_a_hz = hz;
                    format!("FA{:011};", state.vfo_a_hz)
                } else {
                    "?;".to_string()
                }
            } else {
                "?;".to_string()
            }
        }

        // ------------------------------------------------------------------
        // FB — VFO B Frequency
        // ------------------------------------------------------------------
        "FB" => {
            if params.is_empty() {
                format!("FB{:011};", state.vfo_b_hz)
            } else if params.len() == 11 {
                if let Ok(hz) = params.parse::<u64>() {
                    state.vfo_b_hz = hz;
                    format!("FB{:011};", state.vfo_b_hz)
                } else {
                    "?;".to_string()
                }
            } else {
                "?;".to_string()
            }
        }

        // ------------------------------------------------------------------
        // MD — Operating Mode
        // ------------------------------------------------------------------
        "MD" => {
            if params.is_empty() {
                format!("MD{};", state.mode)
            } else if params.len() == 1 {
                if let Ok(m) = params.parse::<u8>() {
                    // Valid modes: 1–7 and 9
                    if matches!(m, 1..=7 | 9) {
                        state.mode = m;
                        format!("MD{};", state.mode)
                    } else {
                        "?;".to_string()
                    }
                } else {
                    "?;".to_string()
                }
            } else {
                "?;".to_string()
            }
        }

        // ------------------------------------------------------------------
        // AG — AF Gain (0–255, 3 digits)
        // ------------------------------------------------------------------
        "AG" => {
            if params.is_empty() {
                format!("AG{:03};", state.af_gain)
            } else if params.len() == 3 {
                if let Ok(v) = params.parse::<u8>() {
                    state.af_gain = v;
                    format!("AG{:03};", state.af_gain)
                } else {
                    "?;".to_string()
                }
            } else {
                "?;".to_string()
            }
        }

        // ------------------------------------------------------------------
        // RG — RF Gain (0–255, 3 digits)
        // ------------------------------------------------------------------
        "RG" => {
            if params.is_empty() {
                format!("RG{:03};", state.rf_gain)
            } else if params.len() == 3 {
                if let Ok(v) = params.parse::<u8>() {
                    state.rf_gain = v;
                    format!("RG{:03};", state.rf_gain)
                } else {
                    "?;".to_string()
                }
            } else {
                "?;".to_string()
            }
        }

        // ------------------------------------------------------------------
        // SQ — Squelch Level (0–255, 3 digits)
        // ------------------------------------------------------------------
        "SQ" => {
            if params.is_empty() {
                format!("SQ{:03};", state.squelch)
            } else if params.len() == 3 {
                if let Ok(v) = params.parse::<u8>() {
                    state.squelch = v;
                    format!("SQ{:03};", state.squelch)
                } else {
                    "?;".to_string()
                }
            } else {
                "?;".to_string()
            }
        }

        // ------------------------------------------------------------------
        // PC — Power Control (0–100, 3 digits)
        // ------------------------------------------------------------------
        "PC" => {
            if params.is_empty() {
                format!("PC{:03};", state.power_control)
            } else if params.len() == 3 {
                if let Ok(v) = params.parse::<u8>() {
                    if v <= 100 {
                        state.power_control = v;
                        format!("PC{:03};", state.power_control)
                    } else {
                        "?;".to_string()
                    }
                } else {
                    "?;".to_string()
                }
            } else {
                "?;".to_string()
            }
        }

        // ------------------------------------------------------------------
        // TX — Transmit (PTT on), write-only, no params
        // ------------------------------------------------------------------
        "TX" => {
            state.tx = true;
            "TX;".to_string()
        }

        // ------------------------------------------------------------------
        // RX — Receive (PTT off), write-only, no params
        // ------------------------------------------------------------------
        "RX" => {
            state.tx = false;
            "RX;".to_string()
        }

        // ------------------------------------------------------------------
        // IF — Transceiver Information (read-only, composite response)
        //
        // TS-570D IF format:
        //   IF<11-digit-freq><5-spaces><5-digit-RIT><RIT-on><XIT-on><mem-ch(2)><tx><mode><vfo><scan>0;
        // ------------------------------------------------------------------
        "IF" => {
            let tx_flag = if state.tx { 1u8 } else { 0u8 };
            format!(
                "IF{:011}     00000000{:02}{}{}{}{};",
                state.vfo_a_hz, // 11-digit freq
                // "     " 5 spaces for sub-info
                // "00000" RIT offset (5 digits)
                // "0" RIT off, "0" XIT off
                0u8,        // mem_ch (2 digits via {:02})
                tx_flag,    // TX/RX status (0=rx, 1=tx)
                state.mode, // mode digit
                0u8,        // VFO/memory selection
                0u8,        // scan
            )
        }

        // ------------------------------------------------------------------
        // ID — Transceiver ID, returns "019" for TS-570D
        // ------------------------------------------------------------------
        "ID" => "ID019;".to_string(),

        // ------------------------------------------------------------------
        // AI — Auto Information (0=off, 1=on)
        // ------------------------------------------------------------------
        "AI" => {
            if params.is_empty() {
                format!("AI{};", state.auto_info)
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    if v <= 1 {
                        state.auto_info = v;
                        format!("AI{};", state.auto_info)
                    } else {
                        "?;".to_string()
                    }
                } else {
                    "?;".to_string()
                }
            } else {
                "?;".to_string()
            }
        }

        // ------------------------------------------------------------------
        // SM — S-Meter reading (read-only, with 1-digit selector prefix)
        //
        // Real response: SM0XXXX; where XXXX is 0000–0030.
        // ------------------------------------------------------------------
        "SM" => {
            // params may be "0" (selector) — we respond with SM0<smeter>;
            format!("SM0{:04};", state.smeter)
        }

        // ------------------------------------------------------------------
        // Unknown command
        // ------------------------------------------------------------------
        _ => "?;".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_state() -> RadioState {
        RadioState::default()
    }

    #[test]
    fn test_fa_query() {
        let mut s = default_state();
        let resp = handle("FA", &mut s);
        assert!(
            resp.starts_with("FA") && resp.ends_with(';'),
            "FA query: {}",
            resp
        );
        // "FA" (2) + 11 digits + ";" (1) = 14 chars total
        assert_eq!(resp.len(), 14, "FA response length: {}", resp);
        let digits = &resp[2..13];
        assert!(digits.chars().all(|c| c.is_ascii_digit()), "{}", resp);
    }

    #[test]
    fn test_fa_set_and_query() {
        let mut s = default_state();
        let resp = handle("FA00014250000", &mut s);
        assert_eq!(resp, "FA00014250000;");
        assert_eq!(s.vfo_a_hz, 14_250_000);
    }

    #[test]
    fn test_fb_query() {
        let mut s = default_state();
        let resp = handle("FB", &mut s);
        assert!(resp.starts_with("FB") && resp.ends_with(';'));
        // "FB" (2) + 11 digits + ";" (1) = 14 chars total
        assert_eq!(resp.len(), 14);
    }

    #[test]
    fn test_md_query_default_usb() {
        let mut s = default_state();
        let resp = handle("MD", &mut s);
        assert_eq!(resp, "MD2;"); // USB is default
    }

    #[test]
    fn test_md_set() {
        let mut s = default_state();
        let resp = handle("MD1", &mut s);
        assert_eq!(resp, "MD1;");
        assert_eq!(s.mode, 1);
    }

    #[test]
    fn test_tx_rx() {
        let mut s = default_state();
        assert!(!s.tx);
        let r = handle("TX", &mut s);
        assert_eq!(r, "TX;");
        assert!(s.tx);
        let r = handle("RX", &mut s);
        assert_eq!(r, "RX;");
        assert!(!s.tx);
    }

    #[test]
    fn test_id() {
        let mut s = default_state();
        assert_eq!(handle("ID", &mut s), "ID019;");
    }

    #[test]
    fn test_if_response_format() {
        let mut s = default_state();
        let resp = handle("IF", &mut s);
        assert!(resp.starts_with("IF"), "{}", resp);
        assert!(resp.ends_with(';'), "{}", resp);
    }

    #[test]
    fn test_sm_response() {
        let mut s = default_state();
        let resp = handle("SM", &mut s);
        assert!(resp.starts_with("SM0"), "{}", resp);
        assert!(resp.ends_with(';'));
    }

    #[test]
    fn test_unknown_command() {
        let mut s = default_state();
        assert_eq!(handle("ZZ", &mut s), "?;");
    }

    #[test]
    fn test_ag_query_and_set() {
        let mut s = default_state();
        let resp = handle("AG", &mut s);
        assert!(resp.starts_with("AG") && resp.ends_with(';'));
        let r2 = handle("AG200", &mut s);
        assert_eq!(r2, "AG200;");
        assert_eq!(s.af_gain, 200);
    }

    #[test]
    fn test_fa_response_matches_regex() {
        let mut s = default_state();
        let resp = handle("FA", &mut s);
        // Must match FA\d{11};
        assert_eq!(&resp[..2], "FA");
        let digits = &resp[2..13];
        assert!(
            digits.chars().all(|c| c.is_ascii_digit()),
            "expected 11 digits, got: {}",
            digits
        );
        assert_eq!(&resp[13..], ";");
    }
}
