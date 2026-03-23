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

/// Total number of unique diagnostic steps (commands, not rounds).
pub(crate) const DIAG_STEP_COUNT: usize = 64;

/// Run all diagnostic commands with live UI updates.
///
/// The control state is mutated in-place so the terminal can render progress
/// as each step executes.  Pressing [Esc] between steps aborts the run.
async fn run_diagnostics<R: Radio>(
    radio: &mut R,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    radio_state: &mut RadioDisplay,
    control: &mut ControlState,
) -> UiResult<()> {
    let mut results: Vec<DiagResult> = Vec::new();

    // The step labels, in the order they are executed.  They are defined here
    // so that draw_diag_panel can display them in the Running state.
    const LABELS: &[&str] = &[
        // SET+GET round-trips
        "set_vfo_a/get_vfo_a",
        "set_vfo_b/get_vfo_b",
        "set_mode(USB)/get_mode",
        "set_mode(LSB)/get_mode",
        "set_mode(CW)/get_mode",
        "set_af_gain/get_af_gain",
        "set_rf_gain/get_rf_gain",
        "set_squelch/get_squelch",
        "set_mic_gain/get_mic_gain",
        "set_power/get_power",
        "set_agc(Slow)/get_agc",
        "set_noise_blanker/get_noise_blanker",
        "set_noise_reduction/get_noise_reduction",
        "set_preamp/get_preamp",
        "set_attenuator/get_attenuator",
        "set_rit(on)/get_rit",
        "set_rit(off)/get_rit",
        "set_xit(on)/get_xit",
        "set_xit(off)/get_xit",
        "set_scan/get_scan",
        "set_vox/get_vox",
        "set_vox_gain/get_vox_gain",
        "set_vox_delay/get_vox_delay",
        "set_rx_vfo/get_rx_vfo",
        "set_tx_vfo/get_tx_vfo",
        "set_frequency_lock/get_frequency_lock",
        "set_fine_step/get_fine_step",
        "set_power_on/get_power_on",
        "set_speech_processor/get_speech_processor",
        "set_memory_channel/get_memory_channel",
        "set_antenna(1)/get_antenna",
        "set_antenna(2)/get_antenna",
        "set_keyer_speed/get_keyer_speed",
        "set_cw_pitch/get_cw_pitch",
        "set_high_cutoff/get_high_cutoff",
        "set_low_cutoff/get_low_cutoff",
        "set_ctcss_tone_number/get_ctcss_tone_number",
        "set_ctcss/get_ctcss",
        "set_tone_number/get_tone_number",
        "set_tone/get_tone",
        "set_beat_cancel/get_beat_cancel",
        "set_if_shift/get_if_shift",
        "set_semi_break_in_delay/get_semi_break_in_delay",
        "set_cw_auto_zerobeat/get_cw_auto_zerobeat",
        // IF cross-checks
        "if_crosscheck:set_vfo_a",
        "if_crosscheck:set_mode(USB)",
        // GET-only
        "get_smeter",
        "get_id",
        "get_information",
        "is_busy",
        "get_meter(RM1)",
        // ACTION-only
        "transmit+receive",
        "receive",
        "clear_rit",
        "rit_up",
        "rit_down",
        "mic_up",
        "mic_down",
        "send_cw(TEST)",
        "set_antenna_tuner_thru",
        "start_antenna_tuning",
        "set_auto_info(0)",
        "voice_recall",
        "reset",
    ];

    'outer: for round in 1..=DIAG_ROUNDS {
        for (step_idx, &label) in LABELS.iter().enumerate() {
            // Abort check
            if check_esc() {
                // Mark remaining as skipped
                let remaining = (LABELS.len() - step_idx) + (DIAG_ROUNDS - round) * LABELS.len();
                for _ in 0..remaining {
                    // Don't push — just stop
                }
                break 'outer;
            }

            // Update Running state with current step info
            *control = ControlState::Diagnostic(DiagState::Running {
                current_label: label,
                current_round: round,
                results: results.clone(),
            });
            draw_frame(terminal, radio_state, control)?;

            // Execute the step
            match step_idx {
                // set_vfo_a/get_vfo_a
                0 => {
                    let target_hz: u64 = 14_195_000;
                    let (passed, detail) = match Frequency::new(target_hz) {
                        Err(e) => (false, format!("freq invalid: {}", e)),
                        Ok(f) => match radio.set_vfo_a(f).await {
                            Err(e) => (false, format!("set failed: {}", e)),
                            Ok(()) => match radio.get_vfo_a().await {
                                Err(e) => (false, format!("get failed: {}", e)),
                                Ok(v) if v.hz() != target_hz => (
                                    false,
                                    format!("mismatch: got {} expected {}", v.hz(), target_hz),
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

                // set_vfo_b/get_vfo_b
                1 => {
                    let target_hz: u64 = 7_100_000;
                    let (passed, detail) = match Frequency::new(target_hz) {
                        Err(e) => (false, format!("freq invalid: {}", e)),
                        Ok(f) => match radio.set_vfo_b(f).await {
                            Err(e) => (false, format!("set failed: {}", e)),
                            Ok(()) => match radio.get_vfo_b().await {
                                Err(e) => (false, format!("get failed: {}", e)),
                                Ok(v) if v.hz() != target_hz => (
                                    false,
                                    format!("mismatch: got {} expected {}", v.hz(), target_hz),
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

                // set_mode(USB)/get_mode
                2 => {
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

                // set_mode(LSB)/get_mode
                3 => {
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

                // set_mode(CW)/get_mode
                4 => {
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

                // set_af_gain/get_af_gain
                5 => {
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

                // set_rf_gain/get_rf_gain
                6 => {
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

                // set_squelch/get_squelch
                7 => {
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

                // set_mic_gain/get_mic_gain
                8 => {
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

                // set_power/get_power
                9 => {
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

                // set_agc(Slow)/get_agc
                10 => {
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

                // set_noise_blanker/get_noise_blanker
                11 => {
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

                // set_noise_reduction/get_noise_reduction
                12 => {
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

                // set_preamp/get_preamp
                13 => {
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

                // set_attenuator/get_attenuator
                14 => {
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

                // set_rit(on)/get_rit
                15 => {
                    let target = true;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_rit(target),
                        radio.get_rit(),
                        target
                    );
                }

                // set_rit(off)/get_rit
                16 => {
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

                // set_xit(on)/get_xit
                17 => {
                    let target = true;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_xit(target),
                        radio.get_xit(),
                        target
                    );
                }

                // set_xit(off)/get_xit
                18 => {
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

                // set_scan/get_scan
                19 => {
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

                // set_vox/get_vox
                20 => {
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

                // set_vox_gain/get_vox_gain
                21 => {
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

                // set_vox_delay/get_vox_delay
                22 => {
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

                // set_rx_vfo/get_rx_vfo
                23 => {
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

                // set_tx_vfo/get_tx_vfo
                24 => {
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

                // set_frequency_lock/get_frequency_lock
                25 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_frequency_lock(target),
                        radio.get_frequency_lock(),
                        target
                    );
                }

                // set_fine_step/get_fine_step
                26 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        label,
                        round,
                        radio.set_fine_step(target),
                        radio.get_fine_step(),
                        target
                    );
                }

                // set_power_on/get_power_on
                27 => {
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

                // set_speech_processor/get_speech_processor
                28 => {
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

                // set_memory_channel/get_memory_channel
                29 => {
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

                // set_antenna(1)/get_antenna
                30 => {
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

                // set_antenna(2)/get_antenna
                31 => {
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

                // set_keyer_speed/get_keyer_speed
                32 => {
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

                // set_cw_pitch/get_cw_pitch
                33 => {
                    // The protocol uses an index 00-12, not Hz.
                    // Index 7 = 600 Hz (approximately), use index 7.
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

                // set_high_cutoff/get_high_cutoff
                34 => {
                    // High cutoff is an index 00-20; use index 14 (~2800 Hz region)
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

                // set_low_cutoff/get_low_cutoff
                35 => {
                    // Low cutoff is an index 00-20; use index 3 (~300 Hz region)
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

                // set_ctcss_tone_number/get_ctcss_tone_number
                36 => {
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

                // set_ctcss/get_ctcss
                37 => {
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

                // set_tone_number/get_tone_number
                38 => {
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

                // set_tone/get_tone
                39 => {
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

                // set_beat_cancel/get_beat_cancel
                40 => {
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

                // set_if_shift/get_if_shift
                41 => {
                    let target_dir = ' ';
                    let target_freq: u16 = 0;
                    let (passed, detail) = match radio.set_if_shift(target_dir, target_freq).await {
                        Err(e) => (false, format!("set failed: {}", e)),
                        Ok(()) => match radio.get_if_shift().await {
                            Err(e) => (false, format!("get failed: {}", e)),
                            Ok((dir, freq)) if dir != target_dir || freq != target_freq => (
                                false,
                                format!(
                                    "mismatch: got ({:?},{}) expected ({:?},{})",
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

                // set_semi_break_in_delay/get_semi_break_in_delay
                42 => {
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

                // set_cw_auto_zerobeat/get_cw_auto_zerobeat
                43 => {
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

                // IF cross-check: after set_vfo_a → verify get_information().frequency
                44 => {
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

                // IF cross-check: after set_mode(USB) → verify get_information().mode
                45 => {
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

                // get_smeter — GET-only, verify Ok, value 0..=30
                46 => {
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

                // get_id — GET-only, verify Ok, non-zero
                47 => {
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

                // get_information — GET-only, verify Ok
                48 => {
                    diag_get!(results, label, round, radio.get_information());
                }

                // is_busy — GET-only, verify Ok
                49 => {
                    diag_get!(results, label, round, radio.is_busy());
                }

                // get_meter(RM1) — GET-only, Ok or Err accepted (some emulators don't support)
                50 => {
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

                // transmit+receive — action pair
                51 => {
                    let (passed, detail) = match radio.transmit().await {
                        Err(e) => (false, format!("transmit failed: {}", e)),
                        Ok(()) => match radio.receive().await {
                            Err(e) => (false, format!("receive failed: {}", e)),
                            Ok(()) => (true, "ok".to_string()),
                        },
                    };
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // receive — standalone action
                52 => {
                    diag_action!(results, label, round, radio.receive());
                }

                // clear_rit
                53 => {
                    diag_action!(results, label, round, radio.clear_rit());
                }

                // rit_up
                54 => {
                    diag_action!(results, label, round, radio.rit_up());
                }

                // rit_down
                55 => {
                    diag_action!(results, label, round, radio.rit_down());
                }

                // mic_up
                56 => {
                    diag_action!(results, label, round, radio.mic_up());
                }

                // mic_down
                57 => {
                    diag_action!(results, label, round, radio.mic_down());
                }

                // send_cw("TEST")
                58 => {
                    diag_action!(results, label, round, radio.send_cw("TEST"));
                }

                // set_antenna_tuner_thru
                59 => {
                    diag_action!(results, label, round, radio.set_antenna_tuner_thru(true));
                }

                // start_antenna_tuning
                60 => {
                    diag_action!(results, label, round, radio.start_antenna_tuning());
                }

                // set_auto_info(0)
                61 => {
                    diag_action!(results, label, round, radio.set_auto_info(0));
                }

                // voice_recall
                62 => {
                    diag_action!(results, label, round, radio.voice_recall(1));
                }

                // reset
                63 => {
                    diag_action!(results, label, round, radio.reset(false));
                }

                _ => {
                    // Should not happen; defensive
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
