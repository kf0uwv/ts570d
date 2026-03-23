use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use framework::radio::{Frequency, Mode, Radio};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    control::{handle_key, ControlState, ExecuteAction, KeyResult},
    diag::{DiagResult, DiagState, DIAG_ROUNDS},
    layout::{draw_control_panel, draw_diag_panel, draw_errors, draw_header, draw_ui, split_areas},
    RadioDisplay, UiError, UiResult,
};

/// Initialize the terminal: enable raw mode and enter the alternate screen.
pub(crate) fn init_terminal() -> UiResult<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Restore the terminal to its normal state.
pub(crate) fn cleanup_terminal() -> UiResult<()> {
    disable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, LeaveAlternateScreen)?;
    Ok(())
}

/// Draw a single frame using the given radio state and control state.
pub(crate) fn draw_frame(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &RadioDisplay,
    control: &ControlState,
) -> UiResult<()> {
    terminal.draw(|f| {
        let area = f.size();
        let (header_area, status_area, errors_area, ctrl_area) = split_areas(area);
        draw_header(f, header_area);
        draw_ui(f, status_area, state);
        draw_errors(f, errors_area, state);
        if let ControlState::Diagnostic(diag) = control {
            draw_diag_panel(f, ctrl_area, diag);
        } else {
            draw_control_panel(f, ctrl_area, control);
        }
    })?;
    Ok(())
}

/// Run the radio UI — polls radio state and renders until 'q' is pressed.
pub async fn run<R: Radio>(radio: &mut R) -> UiResult<()> {
    let mut terminal = init_terminal()?;
    let mut state = RadioDisplay::default();
    let mut control = ControlState::Menu;
    let result = run_radio_loop(radio, &mut terminal, &mut state, &mut control).await;
    cleanup_terminal()?;
    result
}

/// Poll all radio state getters and update `state` in place.
///
/// `state.poll_errors` is cleared at the start of each call and re-populated
/// with any errors from this cycle (capped at 5). Previous values are
/// preserved when a getter fails.
/// This is called at the top of every outer loop iteration and also immediately
/// after a successful command execution so the UI reflects the new state.
async fn poll_radio_state<R: Radio>(radio: &mut R, state: &mut RadioDisplay) {
    state.poll_errors.clear();

    macro_rules! poll {
        ($label:expr, $expr:expr, $ok:expr) => {
            match $expr.await {
                Ok(v) => $ok(v),
                Err(e) => {
                    if state.poll_errors.len() < 5 {
                        state.poll_errors.push(format!("{}: {}", $label, e));
                    }
                }
            }
        };
    }

    poll!(
        "IF",
        radio.get_information(),
        |info: framework::radio::InformationResponse| {
            state.vfo_a_hz = info.frequency.hz();
            state.mode = info.mode.name().to_string();
            state.tx = info.tx_rx;
            state.rit = info.rit_enabled;
            state.xit = info.xit_enabled;
            state.rit_xit_offset_hz = info.rit_xit_offset;
            state.split = info.split;
            state.scan = info.scan_status != 0;
            state.memory_channel = info.memory_channel;
            state.memory_mode = info.vfo_memory != 0;
            state.ctcss = info.ctcss_tone != 0;
        }
    );
    poll!(
        "VFO-B",
        radio.get_vfo_b(),
        |freq: framework::radio::Frequency| {
            state.vfo_b_hz = freq.hz();
        }
    );
    poll!("SM", radio.get_smeter(), |s: u16| {
        state.smeter = s;
    });
    poll!("AF", radio.get_af_gain(), |v: u8| {
        state.af_gain = v;
    });
    poll!("RF", radio.get_rf_gain(), |v: u8| {
        state.rf_gain = v;
    });
    poll!("SQ", radio.get_squelch(), |v: u8| {
        state.squelch = v;
    });
    poll!("MG", radio.get_mic_gain(), |v: u8| {
        state.mic_gain = v;
    });
    poll!("PC", radio.get_power(), |v: u8| {
        state.power_pct = v;
    });
    poll!("GT", radio.get_agc(), |v: u8| {
        state.agc = v;
    });
    poll!("NB", radio.get_noise_blanker(), |v: bool| {
        state.noise_blanker = v;
    });
    poll!("NR", radio.get_noise_reduction(), |v: u8| {
        state.noise_reduction = v;
    });
    poll!("PA", radio.get_preamp(), |v: bool| {
        state.preamp = v;
    });
    poll!("RA", radio.get_attenuator(), |v: bool| {
        state.attenuator = v;
    });
    poll!("PR", radio.get_speech_processor(), |v: bool| {
        state.speech_processor = v;
    });
    poll!("BC", radio.get_beat_cancel(), |v: u8| {
        state.beat_cancel = v;
    });
    poll!("VX", radio.get_vox(), |v: bool| {
        state.vox = v;
    });
    poll!("AN", radio.get_antenna(), |v: u8| {
        state.antenna = v;
    });
    poll!("LK", radio.get_frequency_lock(), |v: bool| {
        state.freq_lock = v;
    });
    poll!("FS", radio.get_fine_step(), |v: bool| {
        state.fine_step = v;
    });
}

// ---------------------------------------------------------------------------
// Diagnostic helpers
// ---------------------------------------------------------------------------

/// Check whether [Esc] has been pressed (non-blocking).
fn check_esc() -> bool {
    if event::poll(Duration::ZERO).unwrap_or(false) {
        if let Ok(Event::Key(k)) = event::read() {
            if k.code == KeyCode::Esc {
                return true;
            }
        }
    }
    false
}

macro_rules! diag_set_get {
    // set then get, compare with ==
    ($results:expr, $label:expr, $round:expr, $set_expr:expr, $get_expr:expr, $target:expr) => {{
        let (passed, detail) = match $set_expr.await {
            Err(e) => (false, format!("set failed: {}", e)),
            Ok(()) => match $get_expr.await {
                Err(e) => (false, format!("get failed: {}", e)),
                Ok(v) if v != $target => (
                    false,
                    format!("mismatch: got {:?} expected {:?}", v, $target),
                ),
                Ok(_) => (true, "ok".to_string()),
            },
        };
        $results.push(DiagResult {
            label: $label,
            round: $round,
            passed,
            detail,
        });
    }};
}

macro_rules! diag_action {
    ($results:expr, $label:expr, $round:expr, $expr:expr) => {{
        let (passed, detail) = match $expr.await {
            Ok(()) => (true, "ok".to_string()),
            Err(e) => (false, format!("failed: {}", e)),
        };
        $results.push(DiagResult {
            label: $label,
            round: $round,
            passed,
            detail,
        });
    }};
}

macro_rules! diag_get {
    ($results:expr, $label:expr, $round:expr, $expr:expr) => {{
        let (passed, detail) = match $expr.await {
            Ok(_) => (true, "ok".to_string()),
            Err(e) => (false, format!("get failed: {}", e)),
        };
        $results.push(DiagResult {
            label: $label,
            round: $round,
            passed,
            detail,
        });
    }};
}

// Each step is (label, closure returning future).  We collect them as a
// sequence of closures so we can update the Running state before each one.
// Since futures are not object-safe we instead encode the step list as a
// static slice of labels and drive the execution in a plain match below.

/// Total number of unique diagnostic steps (one per method).
pub(crate) const DIAG_STEP_COUNT: usize = 104;

/// Run all diagnostic commands with live UI updates.
///
/// The control state is mutated in-place so the terminal can render progress
/// as each step executes.  Pressing [Esc] between steps aborts the run.
///
/// Each step covers exactly one method:
///   - Setter step: calls setter, records result; internally calls getter to
///     verify the set took effect (but records only the setter result).
///   - Getter step: calls getter, verifies value is in sensible range.
///   - Action step: calls action, verifies Ok.
async fn run_diagnostics<R: Radio>(
    radio: &mut R,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    radio_state: &mut RadioDisplay,
    control: &mut ControlState,
) -> UiResult<()> {
    let mut results: Vec<DiagResult> = Vec::new();

    // One label per method, in execution order.
    const LABELS: &[&str] = &[
        // --- VFO / Frequency (8) ---
        "set_vfo_a",          // 0
        "get_vfo_a",          // 1
        "set_vfo_b",          // 2
        "get_vfo_b",          // 3
        "set_fine_step",      // 4
        "get_fine_step",      // 5
        "set_frequency_lock", // 6
        "get_frequency_lock", // 7
        // --- Mode (5) ---
        "set_mode(USB)", // 8
        "set_mode(LSB)", // 9
        "set_mode(CW)",  // 10
        "set_mode(FM)",  // 11
        "get_mode",      // 12
        // --- RIT/XIT (9) ---
        "set_rit(on)",  // 13
        "set_rit(off)", // 14
        "get_rit",      // 15
        "clear_rit",    // 16
        "rit_up",       // 17
        "rit_down",     // 18
        "set_xit(on)",  // 19
        "set_xit(off)", // 20
        "get_xit",      // 21
        // --- Gains (10) ---
        "set_af_gain",  // 22
        "get_af_gain",  // 23
        "set_rf_gain",  // 24
        "get_rf_gain",  // 25
        "set_squelch",  // 26
        "get_squelch",  // 27
        "set_mic_gain", // 28
        "get_mic_gain", // 29
        "set_power",    // 30
        "get_power",    // 31
        // --- Receiver features (14) ---
        "set_agc(Slow)",       // 32
        "get_agc",             // 33
        "set_noise_blanker",   // 34
        "get_noise_blanker",   // 35
        "set_noise_reduction", // 36
        "get_noise_reduction", // 37
        "set_preamp",          // 38
        "get_preamp",          // 39
        "set_attenuator",      // 40
        "get_attenuator",      // 41
        "set_beat_cancel",     // 42
        "get_beat_cancel",     // 43
        "set_if_shift",        // 44
        "get_if_shift",        // 45
        // --- TX features (12) ---
        "set_speech_processor", // 46
        "get_speech_processor", // 47
        "set_vox",              // 48
        "get_vox",              // 49
        "set_vox_gain",         // 50
        "get_vox_gain",         // 51
        "set_vox_delay",        // 52
        "get_vox_delay",        // 53
        "set_power_on",         // 54
        "get_power_on",         // 55
        "transmit",             // 56
        "receive",              // 57
        // --- Scan (2) ---
        "set_scan", // 58
        "get_scan", // 59
        // --- VFO routing (4) ---
        "set_rx_vfo", // 60
        "get_rx_vfo", // 61
        "set_tx_vfo", // 62
        "get_tx_vfo", // 63
        // --- Memory (2) ---
        "set_memory_channel", // 64
        "get_memory_channel", // 65
        // --- Antenna (5) ---
        "set_antenna(1)",         // 66
        "set_antenna(2)",         // 67
        "get_antenna",            // 68
        "set_antenna_tuner_thru", // 69
        "start_antenna_tuning",   // 70
        // --- CW (9) ---
        "set_keyer_speed",         // 71
        "get_keyer_speed",         // 72
        "set_cw_pitch",            // 73
        "get_cw_pitch",            // 74
        "set_cw_auto_zerobeat",    // 75
        "get_cw_auto_zerobeat",    // 76
        "set_semi_break_in_delay", // 77
        "get_semi_break_in_delay", // 78
        "send_cw(TEST)",           // 79
        // --- Audio filter (4) ---
        "set_high_cutoff", // 80
        "get_high_cutoff", // 81
        "set_low_cutoff",  // 82
        "get_low_cutoff",  // 83
        // --- CTCSS / Tone (8) ---
        "set_ctcss_tone_number", // 84
        "get_ctcss_tone_number", // 85
        "set_ctcss",             // 86
        "get_ctcss",             // 87
        "set_tone_number",       // 88
        "get_tone_number",       // 89
        "set_tone",              // 90
        "get_tone",              // 91
        // --- Meters (2) ---
        "get_smeter",     // 92
        "get_meter(RM1)", // 93
        // --- Identity / Info (3) ---
        "get_id",          // 94
        "get_information", // 95
        "is_busy",         // 96
        // --- Misc actions (5) ---
        "mic_up",        // 97
        "mic_down",      // 98
        "set_auto_info", // 99
        "voice_recall",  // 100
        "reset",         // 101
        // --- IF cross-checks (2) ---
        "if_crosscheck:vfo_a", // 102
        "if_crosscheck:mode",  // 103
    ];

    'outer: for round in 1..=DIAG_ROUNDS {
        for (step_idx, &label) in LABELS.iter().enumerate() {
            // Abort check
            if check_esc() {
                break 'outer;
            }

            // Update Running state with current step info
            *control = ControlState::Diagnostic(DiagState::Running {
                current_label: label,
                current_round: round,
                results: results.clone(),
            });
            draw_frame(terminal, radio_state, control)?;

            // Execute the step — one method per arm
            match step_idx {
                // --- VFO / Frequency ---

                // set_vfo_a: set 14_195_000 Hz, internally verify get == target
                0 => {
                    let target_hz: u64 = 14_195_000;
                    let (passed, detail) = match Frequency::new(target_hz) {
                        Err(e) => (false, format!("freq invalid: {}", e)),
                        Ok(f) => match radio.set_vfo_a(f).await {
                            Err(e) => (false, format!("set failed: {}", e)),
                            Ok(()) => match radio.get_vfo_a().await {
                                Err(e) => (false, format!("verify get failed: {}", e)),
                                Ok(v) if v.hz() != target_hz => (
                                    false,
                                    format!(
                                        "verify mismatch: got {} expected {}",
                                        v.hz(),
                                        target_hz
                                    ),
                                ),
                                Ok(_) => (true, "ok".to_string()),
                            },
                        },
                    };
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // get_vfo_a: verify in amateur range
                1 => {
                    let (passed, detail) = match radio.get_vfo_a().await {
                        Err(e) => (false, format!("get failed: {}", e)),
                        Ok(v) if !(500_000..=60_000_000).contains(&v.hz()) => {
                            (false, format!("out of range: {} Hz", v.hz()))
                        }
                        Ok(_) => (true, "ok".to_string()),
                    };
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // set_vfo_b: set 7_100_000 Hz, internally verify get == target
                2 => {
                    let target_hz: u64 = 7_100_000;
                    let (passed, detail) = match Frequency::new(target_hz) {
                        Err(e) => (false, format!("freq invalid: {}", e)),
                        Ok(f) => match radio.set_vfo_b(f).await {
                            Err(e) => (false, format!("set failed: {}", e)),
                            Ok(()) => match radio.get_vfo_b().await {
                                Err(e) => (false, format!("verify get failed: {}", e)),
                                Ok(v) if v.hz() != target_hz => (
                                    false,
                                    format!(
                                        "verify mismatch: got {} expected {}",
                                        v.hz(),
                                        target_hz
                                    ),
                                ),
                                Ok(_) => (true, "ok".to_string()),
                            },
                        },
                    };
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // get_vfo_b: verify in amateur range
                3 => {
                    let (passed, detail) = match radio.get_vfo_b().await {
                        Err(e) => (false, format!("get failed: {}", e)),
                        Ok(v) if !(500_000..=60_000_000).contains(&v.hz()) => {
                            (false, format!("out of range: {} Hz", v.hz()))
                        }
                        Ok(_) => (true, "ok".to_string()),
                    };
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // set_fine_step: set false, verify Ok + get == false
                4 => {
                    let target = false;
                    let (passed, detail) = match radio.set_fine_step(target).await {
                        Err(e) => (false, format!("set failed: {}", e)),
                        Ok(()) => match radio.get_fine_step().await {
                            Err(e) => (false, format!("verify get failed: {}", e)),
                            Ok(v) if v != target => (
                                false,
                                format!("verify mismatch: got {} expected {}", v, target),
                            ),
                            Ok(_) => (true, "ok".to_string()),
                        },
                    };
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // get_fine_step: verify Ok
                5 => {
                    diag_get!(results, label, round, radio.get_fine_step());
                }

                // set_frequency_lock: set false, verify Ok + get == false
                6 => {
                    let target = false;
                    let (passed, detail) = match radio.set_frequency_lock(target).await {
                        Err(e) => (false, format!("set failed: {}", e)),
                        Ok(()) => match radio.get_frequency_lock().await {
                            Err(e) => (false, format!("verify get failed: {}", e)),
                            Ok(v) if v != target => (
                                false,
                                format!("verify mismatch: got {} expected {}", v, target),
                            ),
                            Ok(_) => (true, "ok".to_string()),
                        },
                    };
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // get_frequency_lock: verify Ok
                7 => {
                    diag_get!(results, label, round, radio.get_frequency_lock());
                }

                // --- Mode ---

                // set_mode(USB): set Usb, verify Ok + get == Usb
                8 => {
                    let target = Mode::Usb;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_mode(target),
                        radio.get_mode(),
                        target
                    );
                }

                // set_mode(LSB): set Lsb, verify Ok + get == Lsb
                9 => {
                    let target = Mode::Lsb;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_mode(target),
                        radio.get_mode(),
                        target
                    );
                }

                // set_mode(CW): set Cw, verify Ok + get == Cw
                10 => {
                    let target = Mode::Cw;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_mode(target),
                        radio.get_mode(),
                        target
                    );
                }

                // set_mode(FM): set Fm, verify Ok + get == Fm
                11 => {
                    let target = Mode::Fm;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_mode(target),
                        radio.get_mode(),
                        target
                    );
                }

                // get_mode: verify Ok
                12 => {
                    diag_get!(results, label, round, radio.get_mode());
                }

                // --- RIT/XIT ---

                // set_rit(on): set true, verify Ok
                13 => {
                    diag_action!(results, label, round, radio.set_rit(true));
                }

                // set_rit(off): set false, verify Ok + get == false
                14 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_rit(target),
                        radio.get_rit(),
                        target
                    );
                }

                // get_rit: verify Ok
                15 => {
                    diag_get!(results, label, round, radio.get_rit());
                }

                // clear_rit: verify Ok
                16 => {
                    diag_action!(results, label, round, radio.clear_rit());
                }

                // rit_up: verify Ok
                17 => {
                    diag_action!(results, label, round, radio.rit_up());
                }

                // rit_down: verify Ok
                18 => {
                    diag_action!(results, label, round, radio.rit_down());
                }

                // set_xit(on): set true, verify Ok
                19 => {
                    diag_action!(results, label, round, radio.set_xit(true));
                }

                // set_xit(off): set false, verify Ok + get == false
                20 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_xit(target),
                        radio.get_xit(),
                        target
                    );
                }

                // get_xit: verify Ok
                21 => {
                    diag_get!(results, label, round, radio.get_xit());
                }

                // --- Gains ---

                // set_af_gain: set 128, verify Ok + get == 128
                22 => {
                    let target: u8 = 128;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_af_gain(target),
                        radio.get_af_gain(),
                        target
                    );
                }

                // get_af_gain: verify Ok
                23 => {
                    diag_get!(results, label, round, radio.get_af_gain());
                }

                // set_rf_gain: set 200, verify Ok + get == 200
                24 => {
                    let target: u8 = 200;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_rf_gain(target),
                        radio.get_rf_gain(),
                        target
                    );
                }

                // get_rf_gain: verify Ok
                25 => {
                    diag_get!(results, label, round, radio.get_rf_gain());
                }

                // set_squelch: set 30, verify Ok + get == 30
                26 => {
                    let target: u8 = 30;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_squelch(target),
                        radio.get_squelch(),
                        target
                    );
                }

                // get_squelch: verify Ok
                27 => {
                    diag_get!(results, label, round, radio.get_squelch());
                }

                // set_mic_gain: set 50, verify Ok + get == 50
                28 => {
                    let target: u8 = 50;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_mic_gain(target),
                        radio.get_mic_gain(),
                        target
                    );
                }

                // get_mic_gain: verify Ok
                29 => {
                    diag_get!(results, label, round, radio.get_mic_gain());
                }

                // set_power: set 75, verify Ok + get == 75
                30 => {
                    let target: u8 = 75;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_power(target),
                        radio.get_power(),
                        target
                    );
                }

                // get_power: verify Ok
                31 => {
                    diag_get!(results, label, round, radio.get_power());
                }

                // --- Receiver features ---

                // set_agc(Slow): set 1, verify Ok + get == 1
                32 => {
                    let target: u8 = 1;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_agc(target),
                        radio.get_agc(),
                        target
                    );
                }

                // get_agc: verify Ok
                33 => {
                    diag_get!(results, label, round, radio.get_agc());
                }

                // set_noise_blanker: set true, verify Ok + get == true
                34 => {
                    let target = true;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_noise_blanker(target),
                        radio.get_noise_blanker(),
                        target
                    );
                }

                // get_noise_blanker: verify Ok
                35 => {
                    diag_get!(results, label, round, radio.get_noise_blanker());
                }

                // set_noise_reduction: set 1, verify Ok + get == 1
                36 => {
                    let target: u8 = 1;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_noise_reduction(target),
                        radio.get_noise_reduction(),
                        target
                    );
                }

                // get_noise_reduction: verify Ok
                37 => {
                    diag_get!(results, label, round, radio.get_noise_reduction());
                }

                // set_preamp: set true, verify Ok + get == true
                38 => {
                    let target = true;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_preamp(target),
                        radio.get_preamp(),
                        target
                    );
                }

                // get_preamp: verify Ok
                39 => {
                    diag_get!(results, label, round, radio.get_preamp());
                }

                // set_attenuator: set false, verify Ok + get == false
                40 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_attenuator(target),
                        radio.get_attenuator(),
                        target
                    );
                }

                // get_attenuator: verify Ok
                41 => {
                    diag_get!(results, label, round, radio.get_attenuator());
                }

                // set_beat_cancel: set 0, verify Ok + get == 0
                42 => {
                    let target: u8 = 0;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_beat_cancel(target),
                        radio.get_beat_cancel(),
                        target
                    );
                }

                // get_beat_cancel: verify Ok
                43 => {
                    diag_get!(results, label, round, radio.get_beat_cancel());
                }

                // set_if_shift: set (' ', 0), verify Ok + get matches
                44 => {
                    let target_dir = ' ';
                    let target_freq: u16 = 0;
                    let (passed, detail) = match radio.set_if_shift(target_dir, target_freq).await {
                        Err(e) => (false, format!("set failed: {}", e)),
                        Ok(()) => match radio.get_if_shift().await {
                            Err(e) => (false, format!("verify get failed: {}", e)),
                            Ok((dir, freq)) if dir != target_dir || freq != target_freq => (
                                false,
                                format!(
                                    "verify mismatch: got ({:?},{}) expected ({:?},{})",
                                    dir, freq, target_dir, target_freq
                                ),
                            ),
                            Ok(_) => (true, "ok".to_string()),
                        },
                    };
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // get_if_shift: verify Ok
                45 => {
                    diag_get!(results, label, round, radio.get_if_shift());
                }

                // --- TX features ---

                // set_speech_processor: set false, verify Ok + get == false
                46 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_speech_processor(target),
                        radio.get_speech_processor(),
                        target
                    );
                }

                // get_speech_processor: verify Ok
                47 => {
                    diag_get!(results, label, round, radio.get_speech_processor());
                }

                // set_vox: set false, verify Ok + get == false
                48 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_vox(target),
                        radio.get_vox(),
                        target
                    );
                }

                // get_vox: verify Ok
                49 => {
                    diag_get!(results, label, round, radio.get_vox());
                }

                // set_vox_gain: set 50, verify Ok + get == 50
                50 => {
                    let target: u8 = 50;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_vox_gain(target),
                        radio.get_vox_gain(),
                        target
                    );
                }

                // get_vox_gain: verify Ok
                51 => {
                    diag_get!(results, label, round, radio.get_vox_gain());
                }

                // set_vox_delay: set 300, verify Ok + get == 300
                52 => {
                    let target: u16 = 300;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_vox_delay(target),
                        radio.get_vox_delay(),
                        target
                    );
                }

                // get_vox_delay: verify Ok
                53 => {
                    diag_get!(results, label, round, radio.get_vox_delay());
                }

                // set_power_on: set true, verify Ok + get == true
                54 => {
                    let target = true;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_power_on(target),
                        radio.get_power_on(),
                        target
                    );
                }

                // get_power_on: verify Ok
                55 => {
                    diag_get!(results, label, round, radio.get_power_on());
                }

                // transmit: call transmit(), verify Ok, then call receive() to restore
                56 => {
                    let (passed, detail) = match radio.transmit().await {
                        Err(e) => (false, format!("transmit failed: {}", e)),
                        Ok(()) => {
                            // Restore receive mode
                            let _ = radio.receive().await;
                            (true, "ok".to_string())
                        }
                    };
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // receive: call receive(), verify Ok
                57 => {
                    diag_action!(results, label, round, radio.receive());
                }

                // --- Scan ---

                // set_scan: set false, verify Ok + get == false
                58 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_scan(target),
                        radio.get_scan(),
                        target
                    );
                }

                // get_scan: verify Ok
                59 => {
                    diag_get!(results, label, round, radio.get_scan());
                }

                // --- VFO routing ---

                // set_rx_vfo: set 0, verify Ok + get == 0
                60 => {
                    let target: u8 = 0;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_rx_vfo(target),
                        radio.get_rx_vfo(),
                        target
                    );
                }

                // get_rx_vfo: verify Ok
                61 => {
                    diag_get!(results, label, round, radio.get_rx_vfo());
                }

                // set_tx_vfo: set 0, verify Ok + get == 0
                62 => {
                    let target: u8 = 0;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_tx_vfo(target),
                        radio.get_tx_vfo(),
                        target
                    );
                }

                // get_tx_vfo: verify Ok
                63 => {
                    diag_get!(results, label, round, radio.get_tx_vfo());
                }

                // --- Memory ---

                // set_memory_channel: set 1, verify Ok + get == 1
                64 => {
                    let target: u8 = 1;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_memory_channel(target),
                        radio.get_memory_channel(),
                        target
                    );
                }

                // get_memory_channel: verify Ok
                65 => {
                    diag_get!(results, label, round, radio.get_memory_channel());
                }

                // --- Antenna ---

                // set_antenna(1): set 1, verify Ok + get == 1
                66 => {
                    let target: u8 = 1;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_antenna(target),
                        radio.get_antenna(),
                        target
                    );
                }

                // set_antenna(2): set 2, verify Ok + get == 2
                67 => {
                    let target: u8 = 2;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_antenna(target),
                        radio.get_antenna(),
                        target
                    );
                }

                // get_antenna: verify Ok
                68 => {
                    diag_get!(results, label, round, radio.get_antenna());
                }

                // set_antenna_tuner_thru: call, verify Ok
                69 => {
                    diag_action!(results, label, round, radio.set_antenna_tuner_thru(true));
                }

                // start_antenna_tuning: call, verify Ok
                70 => {
                    diag_action!(results, label, round, radio.start_antenna_tuning());
                }

                // --- CW ---

                // set_keyer_speed: set 20, verify Ok + get == 20
                71 => {
                    let target: u8 = 20;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_keyer_speed(target),
                        radio.get_keyer_speed(),
                        target
                    );
                }

                // get_keyer_speed: verify Ok
                72 => {
                    diag_get!(results, label, round, radio.get_keyer_speed());
                }

                // set_cw_pitch: set index 7 (~600 Hz), verify Ok + get == 7
                73 => {
                    let target: u8 = 7;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_cw_pitch(target),
                        radio.get_cw_pitch(),
                        target
                    );
                }

                // get_cw_pitch: verify Ok
                74 => {
                    diag_get!(results, label, round, radio.get_cw_pitch());
                }

                // set_cw_auto_zerobeat: set false, verify Ok + get == false
                75 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_cw_auto_zerobeat(target),
                        radio.get_cw_auto_zerobeat(),
                        target
                    );
                }

                // get_cw_auto_zerobeat: verify Ok
                76 => {
                    diag_get!(results, label, round, radio.get_cw_auto_zerobeat());
                }

                // set_semi_break_in_delay: set 50, verify Ok + get == 50
                77 => {
                    let target: u16 = 50;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_semi_break_in_delay(target),
                        radio.get_semi_break_in_delay(),
                        target
                    );
                }

                // get_semi_break_in_delay: verify Ok
                78 => {
                    diag_get!(results, label, round, radio.get_semi_break_in_delay());
                }

                // send_cw("TEST"): call, verify Ok
                79 => {
                    diag_action!(results, label, round, radio.send_cw("TEST"));
                }

                // --- Audio filter ---

                // set_high_cutoff: set index 14, verify Ok + get == 14
                80 => {
                    let target: u8 = 14;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_high_cutoff(target),
                        radio.get_high_cutoff(),
                        target
                    );
                }

                // get_high_cutoff: verify Ok
                81 => {
                    diag_get!(results, label, round, radio.get_high_cutoff());
                }

                // set_low_cutoff: set index 3, verify Ok + get == 3
                82 => {
                    let target: u8 = 3;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_low_cutoff(target),
                        radio.get_low_cutoff(),
                        target
                    );
                }

                // get_low_cutoff: verify Ok
                83 => {
                    diag_get!(results, label, round, radio.get_low_cutoff());
                }

                // --- CTCSS / Tone ---

                // set_ctcss_tone_number: set 1, verify Ok + get == 1
                84 => {
                    let target: u8 = 1;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_ctcss_tone_number(target),
                        radio.get_ctcss_tone_number(),
                        target
                    );
                }

                // get_ctcss_tone_number: verify Ok
                85 => {
                    diag_get!(results, label, round, radio.get_ctcss_tone_number());
                }

                // set_ctcss: set false, verify Ok + get == false
                86 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_ctcss(target),
                        radio.get_ctcss(),
                        target
                    );
                }

                // get_ctcss: verify Ok
                87 => {
                    diag_get!(results, label, round, radio.get_ctcss());
                }

                // set_tone_number: set 1, verify Ok + get == 1
                88 => {
                    let target: u8 = 1;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_tone_number(target),
                        radio.get_tone_number(),
                        target
                    );
                }

                // get_tone_number: verify Ok
                89 => {
                    diag_get!(results, label, round, radio.get_tone_number());
                }

                // set_tone: set false, verify Ok + get == false
                90 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_tone(target),
                        radio.get_tone(),
                        target
                    );
                }

                // get_tone: verify Ok
                91 => {
                    diag_get!(results, label, round, radio.get_tone());
                }

                // --- Meters ---

                // get_smeter: verify Ok, value 0..=30
                92 => {
                    let (passed, detail) = match radio.get_smeter().await {
                        Err(e) => (false, format!("get failed: {}", e)),
                        Ok(v) if v > 30 => (false, format!("out of range: {} > 30", v)),
                        Ok(_) => (true, "ok".to_string()),
                    };
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // get_meter(RM1): verify Ok or error accepted
                93 => {
                    let (passed, detail) = match radio.get_meter(1).await {
                        Ok(_) => (true, "ok".to_string()),
                        Err(e) => (false, format!("failed: {}", e)),
                    };
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // --- Identity / Info ---

                // get_id: verify Ok, non-zero
                94 => {
                    let (passed, detail) = match radio.get_id().await {
                        Err(e) => (false, format!("get failed: {}", e)),
                        Ok(0) => (false, "id returned 0".to_string()),
                        Ok(_) => (true, "ok".to_string()),
                    };
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // get_information: verify Ok (all fields parse)
                95 => {
                    diag_get!(results, label, round, radio.get_information());
                }

                // is_busy: verify Ok
                96 => {
                    diag_get!(results, label, round, radio.is_busy());
                }

                // --- Misc actions ---

                // mic_up: verify Ok
                97 => {
                    diag_action!(results, label, round, radio.mic_up());
                }

                // mic_down: verify Ok
                98 => {
                    diag_action!(results, label, round, radio.mic_down());
                }

                // set_auto_info: set false (0), verify Ok
                99 => {
                    diag_action!(results, label, round, radio.set_auto_info(0));
                }

                // voice_recall: call voice 1, verify Ok
                100 => {
                    diag_action!(results, label, round, radio.voice_recall(1));
                }

                // reset: partial reset, verify Ok
                101 => {
                    diag_action!(results, label, round, radio.reset(false));
                }

                // --- IF cross-checks ---

                // if_crosscheck:vfo_a — set_vfo_a then verify get_information().frequency
                102 => {
                    let target_hz: u64 = 14_195_000;
                    let (passed, detail) = match Frequency::new(target_hz) {
                        Err(e) => (false, format!("freq invalid: {}", e)),
                        Ok(f) => match radio.set_vfo_a(f).await {
                            Err(e) => (false, format!("set_vfo_a failed: {}", e)),
                            Ok(()) => match radio.get_information().await {
                                Err(e) => (false, format!("IF failed: {}", e)),
                                Ok(info) if info.frequency.hz() != target_hz => (
                                    false,
                                    format!(
                                        "IF mismatch: got {} expected {}",
                                        info.frequency.hz(),
                                        target_hz
                                    ),
                                ),
                                Ok(_) => (true, "ok".to_string()),
                            },
                        },
                    };
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // if_crosscheck:mode — set_mode(Usb) then verify get_information().mode
                103 => {
                    let target = Mode::Usb;
                    let (passed, detail) = match radio.set_mode(target).await {
                        Err(e) => (false, format!("set_mode failed: {}", e)),
                        Ok(()) => match radio.get_information().await {
                            Err(e) => (false, format!("IF failed: {}", e)),
                            Ok(info) if info.mode != target => (
                                false,
                                format!("IF mismatch: got {} expected {}", info.mode, target),
                            ),
                            Ok(_) => (true, "ok".to_string()),
                        },
                    };
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                _ => {
                    // Defensive: should never reach here
                    results.push(DiagResult {
                        label,
                        round,
                        passed: false,
                        detail: "unimplemented step".to_string(),
                    });
                }
            }
        }
    }

    *control = ControlState::Diagnostic(DiagState::Done { results });
    draw_frame(terminal, radio_state, control)?;
    Ok(())
}

async fn run_radio_loop<R: Radio>(
    radio: &mut R,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut RadioDisplay,
    control: &mut ControlState,
) -> UiResult<()> {
    loop {
        // --- Phase 3: Poll radio state ---
        poll_radio_state(radio, state).await;

        // --- Phase 4: Draw frame ---
        draw_frame(terminal, state, control)?;

        // --- Phases 1+2: Event loop (200ms window, 10ms steps) ---
        let mut elapsed = std::time::Duration::ZERO;
        let poll_interval = std::time::Duration::from_millis(200);
        let check_step = std::time::Duration::from_millis(10);

        while elapsed < poll_interval {
            // Phase 1: check for key events
            if event::poll(std::time::Duration::from_millis(10)).map_err(UiError::Io)? {
                if let Event::Key(key) = event::read().map_err(UiError::Io)? {
                    // Phase 2: handle key, possibly execute a radio command
                    match handle_key(key, control) {
                        KeyResult::Quit => return Ok(()),
                        KeyResult::Continue => {
                            // Re-draw immediately so user sees state change
                            draw_frame(terminal, state, control)?;
                        }
                        KeyResult::StartDiag => {
                            // Show "Running" state immediately, then run diagnostics
                            draw_frame(terminal, state, control)?;
                            run_diagnostics(radio, terminal, state, control).await?;
                        }
                        KeyResult::Execute(action) => {
                            let (desc, result) = execute_action(radio, action).await;
                            match result {
                                Ok(()) => {
                                    // Re-poll immediately so the status panel reflects
                                    // the new radio state before the next 200ms cycle.
                                    poll_radio_state(radio, state).await;
                                    *control = ControlState::Feedback {
                                        message: format!("OK: {}", desc),
                                        is_error: false,
                                    };
                                }
                                Err(e) => {
                                    *control = ControlState::Feedback {
                                        message: format!("Error: {}", e),
                                        is_error: true,
                                    };
                                }
                            }
                            draw_frame(terminal, state, control)?;
                        }
                    }
                }
            }
            monoio::time::sleep(check_step).await;
            elapsed += check_step;
        }
    }
}

/// Execute a radio action, returning a human-readable description and the result.
async fn execute_action<R: Radio>(
    radio: &mut R,
    action: ExecuteAction,
) -> (&'static str, framework::radio::RadioResult<()>) {
    use ExecuteAction::*;
    match action {
        SetVfoA(hz) => {
            let r = match Frequency::new(hz) {
                Ok(f) => radio.set_vfo_a(f).await,
                Err(e) => Err(e),
            };
            ("VFO A set", r)
        }
        SetVfoB(hz) => {
            let r = match Frequency::new(hz) {
                Ok(f) => radio.set_vfo_b(f).await,
                Err(e) => Err(e),
            };
            ("VFO B set", r)
        }
        SetAfGain(v) => ("AF gain set", radio.set_af_gain(v).await),
        SetRfGain(v) => ("RF gain set", radio.set_rf_gain(v).await),
        SetSqLevel(v) => ("Squelch set", radio.set_squelch(v).await),
        SetMicGain(v) => ("MIC gain set", radio.set_mic_gain(v).await),
        SetPower(v) => ("TX power set", radio.set_power(v).await),
        SetVoxGain(v) => ("VOX gain set", radio.set_vox_gain(v).await),
        SetVoxDelay(v) => ("VOX delay set", radio.set_vox_delay(v).await),
        SetKeyerSpeed(v) => ("Keyer speed set", radio.set_keyer_speed(v).await),
        SetMode(m) => {
            let r = match Mode::try_from(m) {
                Ok(mode) => radio.set_mode(mode).await,
                Err(e) => Err(e),
            };
            ("Mode set", r)
        }
        SetAgc(v) => ("AGC set", radio.set_agc(v).await),
        SetNoiseReduction(v) => ("Noise reduction set", radio.set_noise_reduction(v).await),
        SetAntenna(v) => ("Antenna set", radio.set_antenna(v).await),
        ToggleRit(on) => ("RIT toggled", radio.set_rit(on).await),
        ToggleXit(on) => ("XIT toggled", radio.set_xit(on).await),
        ToggleNb(on) => ("Noise blanker toggled", radio.set_noise_blanker(on).await),
        TogglePreamp(on) => ("Preamp toggled", radio.set_preamp(on).await),
        ToggleAtt(on) => ("Attenuator toggled", radio.set_attenuator(on).await),
        ToggleVox(on) => ("VOX toggled", radio.set_vox(on).await),
        ToggleProc(on) => (
            "Speech processor toggled",
            radio.set_speech_processor(on).await,
        ),
        ToggleScan(on) => ("Scan toggled", radio.set_scan(on).await),
        ToggleLock(on) => ("Frequency lock toggled", radio.set_frequency_lock(on).await),
        ToggleFine(on) => ("Fine step toggled", radio.set_fine_step(on).await),
    }
}
