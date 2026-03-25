use crate::logger::StateChange;
use crate::radio_state::RadioState;

/// Parse and handle a single CAT command string (without trailing `;`).
///
/// Returns `(response, changes)` where `response` is the complete response
/// string including the trailing `;` (ready to write back to the serial port),
/// and `changes` is a list of `RadioState` field mutations that occurred.
/// Unknown or malformed commands return `("?;", vec![])`.
pub fn handle(cmd: &str, state: &mut RadioState) -> (String, Vec<StateChange>) {
    handle_inner(cmd, state)
}

fn handle_inner(cmd: &str, state: &mut RadioState) -> (String, Vec<StateChange>) {
    // Helper macro: SET command — silent (no response), one state change.
    // Kenwood CAT protocol: SET commands produce no response from the radio.
    macro_rules! set_ok {
        ($field:expr, $value:expr) => {
            (
                String::new(), // no response — SET is silent per Kenwood protocol
                vec![StateChange {
                    field: $field,
                    value: $value.to_string(),
                }],
            )
        };
    }

    // Helper macro: SET command — silent, two state changes.
    macro_rules! set_ok2 {
        ($f1:expr, $v1:expr, $f2:expr, $v2:expr) => {
            (
                String::new(),
                vec![
                    StateChange {
                        field: $f1,
                        value: $v1.to_string(),
                    },
                    StateChange {
                        field: $f2,
                        value: $v2.to_string(),
                    },
                ],
            )
        };
    }

    // Helper: query-only result (no mutation).
    macro_rules! query {
        ($resp:expr) => {
            ($resp, vec![])
        };
    }

    // Commands are at least 2 characters (the 2-letter code).
    if cmd.len() < 2 {
        return ("?;".to_string(), vec![]);
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
                query!(format!("FA{:011};", state.vfo_a_hz))
            } else if params.len() == 11 {
                // Set — silent per Kenwood protocol
                if let Ok(hz) = params.parse::<u64>() {
                    state.vfo_a_hz = hz;
                    set_ok!("vfo_a_hz", state.vfo_a_hz)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // FB — VFO B Frequency
        // ------------------------------------------------------------------
        "FB" => {
            if params.is_empty() {
                query!(format!("FB{:011};", state.vfo_b_hz))
            } else if params.len() == 11 {
                if let Ok(hz) = params.parse::<u64>() {
                    state.vfo_b_hz = hz;
                    set_ok!("vfo_b_hz", state.vfo_b_hz)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // MD — Operating Mode
        // ------------------------------------------------------------------
        "MD" => {
            if params.is_empty() {
                query!(format!("MD{};", state.mode))
            } else if params.len() == 1 {
                if let Ok(m) = params.parse::<u8>() {
                    // Valid modes: 1–7 and 9
                    if matches!(m, 1..=7 | 9) {
                        state.mode = m;
                        set_ok!("mode", state.mode)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // AG — AF Gain
        //
        // Manual p.75: Read=AG;, Answer=AG<P1:3>; (format 31, 3 digits, no selector).
        // Set=AG<P1:3>; but real radio also accepts AG0<P1:3> (4 chars with selector).
        // ------------------------------------------------------------------
        "AG" => {
            if params.is_empty() {
                // Query: respond with 3-digit level only (no selector per manual)
                query!(format!("AG{:03};", state.af_gain))
            } else if params.len() == 4 {
                // Set with selector prefix (e.g. "0200") — silent
                if let Ok(v) = params[1..].parse::<u8>() {
                    state.af_gain = v;
                    set_ok!("af_gain", state.af_gain)
                } else {
                    query!("?;".to_string())
                }
            } else if params.len() == 3 {
                // Set without selector prefix — silent
                if let Ok(v) = params.parse::<u8>() {
                    state.af_gain = v;
                    set_ok!("af_gain", state.af_gain)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // RG — RF Gain (0–255, 3 digits)
        // ------------------------------------------------------------------
        "RG" => {
            if params.is_empty() {
                query!(format!("RG{:03};", state.rf_gain))
            } else if params.len() == 3 {
                if let Ok(v) = params.parse::<u8>() {
                    state.rf_gain = v;
                    set_ok!("rf_gain", state.rf_gain)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // SQ — Squelch Level (0–255, 3 digits)
        // ------------------------------------------------------------------
        "SQ" => {
            if params.is_empty() {
                query!(format!("SQ{:03};", state.squelch))
            } else if params.len() == 3 {
                if let Ok(v) = params.parse::<u8>() {
                    state.squelch = v;
                    set_ok!("squelch", state.squelch)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // PC — Power Control (5–100 in 5W steps, 3 digits)
        // ------------------------------------------------------------------
        "PC" => {
            if params.is_empty() {
                query!(format!("PC{:03};", state.power_control))
            } else if params.len() == 3 {
                if let Ok(v) = params.parse::<u8>() {
                    if v >= 5 && v <= 100 && v % 5 == 0 {
                        state.power_control = v;
                        set_ok!("power_control", state.power_control)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // TX — Transmit (PTT on), write-only, no params
        // ------------------------------------------------------------------
        "TX" => {
            state.tx = true;
            set_ok!("tx", true)
        }

        // ------------------------------------------------------------------
        // RX — Receive (PTT off), write-only, no params
        // ------------------------------------------------------------------
        "RX" => {
            state.tx = false;
            set_ok!("tx", false)
        }

        // ------------------------------------------------------------------
        // IF — Transceiver Information (read-only, composite 34-char payload)
        //
        // Payload layout (34 chars after "IF"):
        //   [0..11]  freq (11 digits)
        //   [11..16] step (5 chars, spaces — TS-570D doesn't use this in CAT)
        //   [16..21] RIT/XIT offset (sign + 4 digits, e.g. "+0000")
        //   [21]     RIT enabled (0/1)
        //   [22]     XIT enabled (0/1)
        //   [23]     bank (1 char, space = VFO mode)
        //   [24..26] memory channel (2 digits)
        //   [26]     TX/RX (0=RX, 1=TX)
        //   [27]     mode (1–9)
        //   [28]     VFO/memory (0=VFO, 1=Memory)
        //   [29]     scan status (0/1)
        //   [30]     split (0/1)
        //   [31..33] CTCSS tone number (2 digits)
        //   [33]     tone number (1 char)
        // ------------------------------------------------------------------
        "IF" => {
            let tx_flag = u8::from(state.tx);
            let rit_flag = u8::from(state.rit);
            let xit_flag = u8::from(state.xit);
            let vfo_flag: u8 = 0; // 0 = VFO mode
            let scan_flag = u8::from(state.scan);
            let split_flag = u8::from(state.split);
            let rit_sign = if state.rit_offset >= 0 { '+' } else { '-' };
            let rit_abs = state.rit_offset.unsigned_abs() as u16;
            let tone_digit = state.tone_number % 10;
            query!(format!(
                "IF{freq:011}     {sign}{rit_off:04}{rit}{xit} {mem:02}{tx}{mode}{vfo}{scan}{split}{ctcss:02}{tone:01};",
                freq = state.vfo_a_hz,
                sign = rit_sign,
                rit_off = rit_abs,
                rit = rit_flag,
                xit = xit_flag,
                mem = state.mem_channel,
                tx = tx_flag,
                mode = state.mode,
                vfo = vfo_flag,
                scan = scan_flag,
                split = split_flag,
                ctcss = state.ctcss_tone,
                tone = tone_digit,
            ))
        }

        // ------------------------------------------------------------------
        // ID — Transceiver ID, returns "017" for TS-570D
        // ------------------------------------------------------------------
        "ID" => query!("ID017;".to_string()),

        // ------------------------------------------------------------------
        // AI — Auto Information (0=off, 1=IF periodic, 2=on-change, 3=both)
        // ------------------------------------------------------------------
        "AI" => {
            if params.is_empty() {
                query!(format!("AI{};", state.auto_info))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    if v <= 3 {
                        state.auto_info = v;
                        set_ok!("auto_info", state.auto_info)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // SM — S-Meter reading (read-only)
        //
        // Manual p.80: Answer format is SM<P1:4>; where P1=S-METER VALUE
        // (format 22, 4 digits, 0000–0015).  No selector digit in response.
        // ------------------------------------------------------------------
        "SM" => {
            query!(format!("SM{:04};", state.smeter))
        }

        // ------------------------------------------------------------------
        // NB — Noise Blanker (0=off, 1=on)
        // ------------------------------------------------------------------
        "NB" => {
            if params.is_empty() {
                query!(format!("NB{};", u8::from(state.noise_blanker)))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    state.noise_blanker = v != 0;
                    set_ok!("noise_blanker", state.noise_blanker)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // NR — Noise Reduction level (0=off, 1=NR1, 2=NR2)
        // ------------------------------------------------------------------
        "NR" => {
            if params.is_empty() {
                query!(format!("NR{};", state.noise_reduction))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    if v <= 2 {
                        state.noise_reduction = v;
                        set_ok!("noise_reduction", state.noise_reduction)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // PA — Pre-amplifier (0=off, 1=on)
        // ------------------------------------------------------------------
        "PA" => {
            if params.is_empty() {
                query!(format!("PA{};", u8::from(state.preamp)))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    state.preamp = v != 0;
                    set_ok!("preamp", state.preamp)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // RA — Attenuator (00=off, 01=on, 2-digit)
        // ------------------------------------------------------------------
        "RA" => {
            if params.is_empty() {
                query!(format!("RA{:02};", u8::from(state.attenuator)))
            } else if params.len() == 2 {
                if let Ok(v) = params.parse::<u8>() {
                    state.attenuator = v != 0;
                    set_ok!("attenuator", state.attenuator)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // MG — Microphone Gain (0–100, 3 digits)
        //
        // Manual p.78: Format 31 (MIC GAIN), range 000 (min) – 100 (max).
        // ------------------------------------------------------------------
        "MG" => {
            if params.is_empty() {
                query!(format!("MG{:03};", state.mic_gain))
            } else if params.len() == 3 {
                if let Ok(v) = params.parse::<u8>() {
                    if v <= 100 {
                        state.mic_gain = v;
                        set_ok!("mic_gain", state.mic_gain)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // GT — AGC time constant (0–4, 3 digits: 000–004)
        // ------------------------------------------------------------------
        "GT" => {
            if params.is_empty() {
                query!(format!("GT{:03};", state.agc))
            } else if params.len() == 3 {
                if let Ok(v) = params.parse::<u8>() {
                    if v <= 4 {
                        state.agc = v;
                        set_ok!("agc", state.agc)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // RT — RIT on/off (0=off, 1=on)
        // ------------------------------------------------------------------
        "RT" => {
            if params.is_empty() {
                query!(format!("RT{};", u8::from(state.rit)))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    state.rit = v != 0;
                    set_ok!("rit", state.rit)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // XT — XIT on/off (0=off, 1=on)
        // ------------------------------------------------------------------
        "XT" => {
            if params.is_empty() {
                query!(format!("XT{};", u8::from(state.xit)))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    state.xit = v != 0;
                    set_ok!("xit", state.xit)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // RC — RIT/XIT clear (write-only, no params, silent)
        // ------------------------------------------------------------------
        "RC" => (String::new(), vec![]),

        // ------------------------------------------------------------------
        // RU — RIT/XIT up step (write-only, silent)
        // ------------------------------------------------------------------
        "RU" => (String::new(), vec![]),

        // ------------------------------------------------------------------
        // RD — RIT/XIT down step (write-only, silent)
        // ------------------------------------------------------------------
        "RD" => (String::new(), vec![]),

        // ------------------------------------------------------------------
        // SC — Scan on/off (0=off, 1=on)
        // ------------------------------------------------------------------
        "SC" => {
            if params.is_empty() {
                query!(format!("SC{};", u8::from(state.scan)))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    state.scan = v != 0;
                    set_ok!("scan", state.scan)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // VX — VOX on/off (0=off, 1=on)
        // ------------------------------------------------------------------
        "VX" => {
            if params.is_empty() {
                query!(format!("VX{};", u8::from(state.vox)))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    state.vox = v != 0;
                    set_ok!("vox", state.vox)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // VG — VOX Gain (1–9, 3 digits per Kenwood spec)
        // ------------------------------------------------------------------
        "VG" => {
            if params.is_empty() {
                query!(format!("VG{:03};", state.vox_gain))
            } else if params.len() == 3 {
                if let Ok(v) = params.parse::<u8>() {
                    if (1..=9).contains(&v) {
                        state.vox_gain = v;
                        set_ok!("vox_gain", state.vox_gain)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // VD — VOX Delay (0–3000 ms, 4 digits)
        // ------------------------------------------------------------------
        "VD" => {
            if params.is_empty() {
                query!(format!("VD{:04};", state.vox_delay))
            } else if params.len() == 4 {
                if let Ok(v) = params.parse::<u16>() {
                    if v <= 3000 {
                        state.vox_delay = v;
                        set_ok!("vox_delay", state.vox_delay)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // FR — RX VFO selection (0=A, 1=B, 2=memory)
        // ------------------------------------------------------------------
        "FR" => {
            if params.is_empty() {
                query!(format!("FR{};", state.rx_vfo))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    if v <= 2 {
                        state.rx_vfo = v;
                        set_ok!("rx_vfo", state.rx_vfo)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // FT — TX VFO selection (0=A, 1=B)
        // ------------------------------------------------------------------
        "FT" => {
            if params.is_empty() {
                query!(format!("FT{};", state.tx_vfo))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    if v <= 1 {
                        state.tx_vfo = v;
                        set_ok!("tx_vfo", state.tx_vfo)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // LK — Frequency Lock (0=off, 1=on)
        // ------------------------------------------------------------------
        "LK" => {
            if params.is_empty() {
                query!(format!("LK{};", u8::from(state.freq_lock)))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    state.freq_lock = v != 0;
                    set_ok!("freq_lock", state.freq_lock)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // PS — Power Status (0=off, 1=on)
        // ------------------------------------------------------------------
        "PS" => {
            if params.is_empty() {
                query!(format!("PS{};", u8::from(state.power_on)))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    state.power_on = v != 0;
                    set_ok!("power_on", state.power_on)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // BY — Busy status (read-only, always 0 = not busy)
        // ------------------------------------------------------------------
        "BY" => query!("BY0;".to_string()),

        // ------------------------------------------------------------------
        // PR — Speech Processor (0=off, 1=on)
        // ------------------------------------------------------------------
        "PR" => {
            if params.is_empty() {
                query!(format!("PR{};", u8::from(state.proc)))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    state.proc = v != 0;
                    set_ok!("proc", state.proc)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // MC — Memory Channel (00–99, space + 2 digits)
        // ------------------------------------------------------------------
        "MC" => {
            if params.is_empty() {
                query!(format!("MC {:02};", state.mem_channel))
            } else {
                let trimmed = params.trim();
                if let Ok(v) = trimmed.parse::<u8>() {
                    if v <= 99 {
                        state.mem_channel = v;
                        set_ok!("mem_channel", state.mem_channel)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            }
        }

        // ------------------------------------------------------------------
        // AN — Antenna selection (1=ANT1, 2=ANT2)
        // ------------------------------------------------------------------
        "AN" => {
            if params.is_empty() {
                query!(format!("AN{};", state.antenna))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    if matches!(v, 1 | 2) {
                        state.antenna = v;
                        set_ok!("antenna", state.antenna)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // KS — Keyer Speed (010–060 WPM, 3 digits)
        // ------------------------------------------------------------------
        "KS" => {
            if params.is_empty() {
                query!(format!("KS{:03};", state.keyer_speed))
            } else if params.len() == 3 {
                if let Ok(v) = params.parse::<u8>() {
                    if (10..=60).contains(&v) {
                        state.keyer_speed = v;
                        set_ok!("keyer_speed", state.keyer_speed)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // KY — CW Keying / buffer status
        // Read (KY;): returns buffer status KY0; (buffer available)
        // Set (KY<space><message>;): silent
        // ------------------------------------------------------------------
        "KY" => {
            if params.is_empty() {
                query!("KY0;".to_string())
            } else {
                (String::new(), vec![])
            }
        }

        // ------------------------------------------------------------------
        // PT — CW Pitch (00–12, 2 digits)
        // ------------------------------------------------------------------
        "PT" => {
            if params.is_empty() {
                query!(format!("PT{:02};", state.cw_pitch))
            } else if params.len() == 2 {
                if let Ok(v) = params.parse::<u8>() {
                    if v <= 12 {
                        state.cw_pitch = v;
                        set_ok!("cw_pitch", state.cw_pitch)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // CA — CW Auto Zero-beat (0=off, 1=on)
        // ------------------------------------------------------------------
        "CA" => {
            if params.is_empty() {
                query!(format!("CA{};", u8::from(state.cw_auto_zerobeat)))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    state.cw_auto_zerobeat = v != 0;
                    set_ok!("cw_auto_zerobeat", state.cw_auto_zerobeat)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // AC — Antenna tuner control (3-digit code: XYZ)
        //   X: 0=thru, 1=tune
        //   Y: 0=normal, 1=start tuning
        //   Z: (reserved, 0)
        // ------------------------------------------------------------------
        "AC" => {
            if params.is_empty() {
                // Answer format: AC[P1][P2][P3] — 3 digits; P1 is status-only
                query!(format!("AC{:03};", state.ac_mode))
            } else if params.len() == 2 {
                // SET format: AC[P2][P3] — 2 digits (P2=0:THRU/1:IN, P3=0:off/1:tune)
                if let Ok(v) = params.parse::<u8>() {
                    state.ac_mode = v;
                    set_ok!("ac_mode", state.ac_mode)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // SH — DSP Slope High Cut-off (0–20, 2 digits)
        // ------------------------------------------------------------------
        "SH" => {
            if params.is_empty() {
                query!(format!("SH{:02};", state.sh))
            } else if params.len() == 2 {
                if let Ok(v) = params.parse::<u8>() {
                    if v <= 20 {
                        state.sh = v;
                        set_ok!("sh", state.sh)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // SL — DSP Slope Low Cut-off (0–20, 2 digits)
        // ------------------------------------------------------------------
        "SL" => {
            if params.is_empty() {
                query!(format!("SL{:02};", state.sl))
            } else if params.len() == 2 {
                if let Ok(v) = params.parse::<u8>() {
                    if v <= 20 {
                        state.sl = v;
                        set_ok!("sl", state.sl)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // IS — IF Shift (direction char + 4-digit freq, e.g. "+0500")
        // Space is accepted as equivalent to '+' per manual.
        // ------------------------------------------------------------------
        "IS" => {
            if params.is_empty() {
                query!(format!("IS{}{:04};", state.is_direction, state.is_freq))
            } else if params.len() == 5 {
                let direction = params.chars().next().unwrap_or(' ');
                if direction == '+' || direction == '-' || direction == ' ' {
                    if let Ok(freq) = params[1..].parse::<u16>() {
                        state.is_direction = direction;
                        state.is_freq = freq;
                        set_ok2!("is_direction", state.is_direction, "is_freq", state.is_freq)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // CN — CTCSS Tone Number (01–39, 2 digits)
        // ------------------------------------------------------------------
        "CN" => {
            if params.is_empty() {
                query!(format!("CN{:02};", state.ctcss_tone))
            } else if params.len() == 2 {
                if let Ok(v) = params.parse::<u8>() {
                    if (1..=39).contains(&v) {
                        state.ctcss_tone = v;
                        set_ok!("ctcss_tone", state.ctcss_tone)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // CT — CTCSS on/off (0=off, 1=on)
        // ------------------------------------------------------------------
        "CT" => {
            if params.is_empty() {
                query!(format!("CT{};", u8::from(state.ctcss)))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    state.ctcss = v != 0;
                    set_ok!("ctcss", state.ctcss)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // TN — Tone Number (01–39, 2 digits)
        // ------------------------------------------------------------------
        "TN" => {
            if params.is_empty() {
                query!(format!("TN{:02};", state.tone_number))
            } else if params.len() == 2 {
                if let Ok(v) = params.parse::<u8>() {
                    if (1..=39).contains(&v) {
                        state.tone_number = v;
                        set_ok!("tone_number", state.tone_number)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // TO — Tone on/off (0=off, 1=on)
        // Maps to state.subtone field.
        // ------------------------------------------------------------------
        "TO" => {
            if params.is_empty() {
                query!(format!("TO{};", u8::from(state.subtone)))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    state.subtone = v != 0;
                    set_ok!("subtone", state.subtone)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // BC — Beat Cancel mode (0=off, 1=on, 2=enhanced)
        // ------------------------------------------------------------------
        "BC" => {
            if params.is_empty() {
                query!(format!("BC{};", state.beat_cancel_mode))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    if v <= 2 {
                        state.beat_cancel_mode = v;
                        // Also sync the bool field used by TUI
                        state.beat_cancel = v != 0;
                        set_ok2!(
                            "beat_cancel_mode",
                            state.beat_cancel_mode,
                            "beat_cancel",
                            state.beat_cancel
                        )
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // RM — Meter reading (RM<type>; → RM<type><value:04>;)
        // Types: 1=SWR, 2=COMP, 3=ALC, 4=power; type 0 not in manual but
        // we respond with smeter for any type we don't know.
        // ------------------------------------------------------------------
        "RM" => {
            // params is the meter type digit(s) — we accept 0 or 1 digit
            let meter_type = params.parse::<u8>().unwrap_or(0);
            let value = match meter_type {
                // Use power_control as a proxy for RF power meter
                1 | 4 => u16::from(state.power_control),
                // Default / S-meter proxy
                _ => state.smeter,
            };
            query!(format!("RM{}{:04};", meter_type, value))
        }

        // ------------------------------------------------------------------
        // FS — Fine Step (0=off, 1=on)
        // ------------------------------------------------------------------
        "FS" => {
            if params.is_empty() {
                query!(format!("FS{};", u8::from(state.fine_step)))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    state.fine_step = v != 0;
                    set_ok!("fine_step", state.fine_step)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // SD — Semi Break-in Delay (0000–1000 ms, 4 digits)
        // ------------------------------------------------------------------
        "SD" => {
            if params.is_empty() {
                query!(format!("SD{:04};", state.semi_break_in_delay))
            } else if params.len() == 4 {
                if let Ok(v) = params.parse::<u16>() {
                    if v <= 1000 {
                        state.semi_break_in_delay = v;
                        set_ok!("semi_break_in_delay", state.semi_break_in_delay)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // UP — VFO frequency up by 100 Hz (Menu 02 default step, write-only)
        // ------------------------------------------------------------------
        "UP" => {
            state.vfo_a_hz = state.vfo_a_hz.saturating_add(100);
            set_ok!("vfo_a_hz", state.vfo_a_hz)
        }

        // ------------------------------------------------------------------
        // DN — VFO frequency down by 100 Hz (Menu 02 default step, write-only)
        // ------------------------------------------------------------------
        "DN" => {
            state.vfo_a_hz = state.vfo_a_hz.saturating_sub(100);
            set_ok!("vfo_a_hz", state.vfo_a_hz)
        }

        // ------------------------------------------------------------------
        // VR — Voice Recall (P1=1 or 2, write-only, silent)
        // ------------------------------------------------------------------
        "VR" => {
            if params.len() == 1 {
                if matches!(params, "1" | "2") {
                    (String::new(), vec![])
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // SR — System Reset (P1=1 partial or 2 full, write-only, silent)
        // ------------------------------------------------------------------
        "SR" => {
            if params.len() == 1 {
                if matches!(params, "1" | "2") {
                    (String::new(), vec![])
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // FW — Filter Width (4 digits, 0000–9999)
        // ------------------------------------------------------------------
        "FW" => {
            if params.is_empty() {
                query!(format!("FW{:04};", state.filter_width))
            } else if params.len() == 4 {
                if let Ok(v) = params.parse::<u16>() {
                    state.filter_width = v;
                    set_ok!("filter_width", state.filter_width)
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // EX — Extension Menu (P1=menu number 000–051, P2=selection value 4 digits)
        // ------------------------------------------------------------------
        "EX" => {
            if params.len() == 3 {
                // Read: EX<menu:3>;
                if let Ok(n) = params.parse::<usize>() {
                    if n <= 51 {
                        query!(format!("EX{:03}{:04};", n, state.menu_values[n]))
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else if params.len() == 7 {
                // Set: EX<menu:3><value:4>;
                if let (Ok(n), Ok(v)) = (params[..3].parse::<usize>(), params[3..].parse::<u16>()) {
                    if n <= 51 {
                        state.menu_values[n] = v;
                        set_ok!("menu_values", n)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // LM — Load Message (DRU recording, SET only, silent)
        // P1 = LOAD MESSAGE: 0=cancel recording, 1–3=channel
        // ------------------------------------------------------------------
        "LM" => {
            if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    if v <= 3 {
                        (String::new(), vec![])
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // PB — Play Back (DRU/CW message playback)
        // P1 = PLAYBACK CHANNEL: 0=stop, 1–3=channel
        // ------------------------------------------------------------------
        "PB" => {
            if params.is_empty() {
                query!(format!("PB{};", state.playback_channel))
            } else if params.len() == 1 {
                if let Ok(v) = params.parse::<u8>() {
                    if v <= 3 {
                        state.playback_channel = v;
                        set_ok!("playback_channel", state.playback_channel)
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // MR — Memory Read
        // Read: MR<P1:split><P3:channel:2>;  P1=0 for simplex
        // Answer: MR<P1><_><P3:2><P4:11><P5:1><P6:1><P7:1><P8:2>;
        // ------------------------------------------------------------------
        "MR" => {
            // Accept "MR0NN;" format (P1=split_type 1 digit, P3=channel 2 digits)
            if params.len() >= 3 {
                let split_type = params.chars().next().unwrap_or('0');
                let ch_str = &params[1..];
                if let Ok(ch) = ch_str.trim().parse::<usize>() {
                    if ch <= 99 {
                        let m = &state.memory_channels[ch];
                        if m.freq == 0 {
                            // Vacant: all zeros except channel number
                            query!(format!("MR{} {:02}00000000000000000000;", split_type, ch))
                        } else {
                            query!(format!(
                                "MR{} {:02}{:011}{}{}{}{:02};",
                                split_type,
                                ch,
                                m.freq,
                                m.mode,
                                u8::from(m.lockout),
                                u8::from(m.tone),
                                m.tone_num,
                            ))
                        }
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // MW — Memory Write
        // Set: MW<P1:split><_><P3:channel:2><P4:freq:11><P5:mode><P6:lockout><P7:tone><P8:tone_num:2>
        // ------------------------------------------------------------------
        "MW" => {
            // Minimum params: P1(1) + _ (1) + P3(2) + P4(11) + P5(1) + P6(1) + P7(1) + P8(2) = 20 chars
            if params.len() >= 20 {
                let ch_str = &params[2..4];  // P3: channel at positions 2-3
                if let Ok(ch) = ch_str.trim().parse::<usize>() {
                    if ch <= 99 {
                        let freq_str = &params[4..15];   // P4: freq 11 digits
                        let mode_str = &params[15..16];  // P5: mode 1 digit
                        let lock_str = &params[16..17];  // P6: lockout
                        let tone_str = &params[17..18];  // P7: tone on/off
                        let tnum_str = &params[18..20];  // P8: tone number 2 digits
                        if let (Ok(freq), Ok(mode), Ok(lock), Ok(tone), Ok(tnum)) = (
                            freq_str.parse::<u64>(),
                            mode_str.parse::<u8>(),
                            lock_str.parse::<u8>(),
                            tone_str.parse::<u8>(),
                            tnum_str.parse::<u8>(),
                        ) {
                            state.memory_channels[ch] = crate::radio_state::MemoryChannel {
                                freq,
                                mode,
                                lockout: lock != 0,
                                tone: tone != 0,
                                tone_num: tnum,
                            };
                            set_ok!("memory_channels", ch)
                        } else {
                            query!("?;".to_string())
                        }
                    } else {
                        query!("?;".to_string())
                    }
                } else {
                    query!("?;".to_string())
                }
            } else {
                query!("?;".to_string())
            }
        }

        // ------------------------------------------------------------------
        // FV — Firmware Version (read-only)
        // ------------------------------------------------------------------
        "FV" => query!("FV1.04;".to_string()),

        // ------------------------------------------------------------------
        // Unknown command
        // ------------------------------------------------------------------
        _ => ("?;".to_string(), vec![]),
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
        let (resp, changes) = handle("FA", &mut s);
        assert!(
            resp.starts_with("FA") && resp.ends_with(';'),
            "FA query: {}",
            resp
        );
        // "FA" (2) + 11 digits + ";" (1) = 14 chars total
        assert_eq!(resp.len(), 14, "FA response length: {}", resp);
        let digits = &resp[2..13];
        assert!(digits.chars().all(|c| c.is_ascii_digit()), "{}", resp);
        assert!(changes.is_empty(), "FA query should not produce changes");
    }

    #[test]
    fn test_fa_set_and_query() {
        let mut s = default_state();
        let (resp, changes) = handle("FA00014250000", &mut s);
        assert_eq!(resp, "", "FA set should be silent");
        assert_eq!(s.vfo_a_hz, 14_250_000);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field, "vfo_a_hz");
        assert_eq!(changes[0].value, "14250000");
    }

    #[test]
    fn test_fb_query() {
        let mut s = default_state();
        let (resp, _changes) = handle("FB", &mut s);
        assert!(resp.starts_with("FB") && resp.ends_with(';'));
        // "FB" (2) + 11 digits + ";" (1) = 14 chars total
        assert_eq!(resp.len(), 14);
    }

    #[test]
    fn test_md_set() {
        let mut s = default_state();
        let (resp, changes) = handle("MD1", &mut s);
        assert_eq!(resp, "", "MD set should be silent");
        assert_eq!(s.mode, 1);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field, "mode");
    }

    #[test]
    fn test_tx_rx() {
        let mut s = default_state();
        let (resp, changes) = handle("TX", &mut s);
        assert_eq!(resp, "", "TX should be silent");
        assert!(s.tx);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field, "tx");

        let (resp2, changes2) = handle("RX", &mut s);
        assert_eq!(resp2, "", "RX should be silent");
        assert!(!s.tx);
        assert_eq!(changes2.len(), 1);
        assert_eq!(changes2[0].field, "tx");
    }

    #[test]
    fn test_id_query() {
        let mut s = default_state();
        let (resp, changes) = handle("ID", &mut s);
        assert_eq!(resp, "ID017;");
        assert!(changes.is_empty());
    }

    #[test]
    fn test_unknown_command() {
        let mut s = default_state();
        let (resp, changes) = handle("ZZ", &mut s);
        assert_eq!(resp, "?;");
        assert!(changes.is_empty());
    }

    #[test]
    fn test_if_query() {
        let mut s = default_state();
        let (resp, changes) = handle("IF", &mut s);
        assert!(resp.starts_with("IF"), "IF response: {resp}");
        assert!(resp.ends_with(';'));
        assert!(changes.is_empty());
        // Payload (excluding "IF" and ";") must be exactly 34 chars
        let payload = &resp[2..resp.len()-1];
        assert_eq!(payload.len(), 34, "IF payload length: {payload:?}");
    }
}
