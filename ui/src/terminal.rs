use std::io::{self, Stdout};

use crossterm::{
    event::{self, Event},
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

/// Run all diagnostic commands and return the full result log.
///
/// Each of the 12 commands is run `DIAG_ROUNDS` times. The radio state is
/// SET then GET-checked; some commands also cross-check against `get_information()`.
async fn run_diagnostics<R: Radio>(radio: &mut R) -> Vec<DiagResult> {
    let mut results: Vec<DiagResult> = Vec::new();

    for round in 1..=DIAG_ROUNDS {
        // 1. set_vfo_a(14_195_000) → get_vfo_a() → get_information() frequency
        {
            let label = "set_vfo_a / get_vfo_a";
            let target_hz: u64 = 14_195_000;
            let set_result = match Frequency::new(target_hz) {
                Ok(f) => radio.set_vfo_a(f).await,
                Err(e) => Err(e),
            };
            let (passed, detail) = if let Err(e) = set_result {
                (false, format!("set failed: {}", e))
            } else {
                match radio.get_vfo_a().await {
                    Err(e) => (false, format!("get failed: {}", e)),
                    Ok(f) if f.hz() != target_hz => (
                        false,
                        format!("mismatch: got {} expected {}", f.hz(), target_hz),
                    ),
                    Ok(_) => {
                        // cross-check IF
                        match radio.get_information().await {
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
                        }
                    }
                }
            };
            results.push(DiagResult { label, round, passed, detail });
        }

        // 2. set_mode(Usb) → get_mode() → get_information() mode
        {
            let label = "set_mode / get_mode";
            let target = Mode::Usb;
            let (passed, detail) = match radio.set_mode(target).await {
                Err(e) => (false, format!("set failed: {}", e)),
                Ok(()) => match radio.get_mode().await {
                    Err(e) => (false, format!("get failed: {}", e)),
                    Ok(m) if m != target => (
                        false,
                        format!("mismatch: got {} expected {}", m, target),
                    ),
                    Ok(_) => match radio.get_information().await {
                        Err(e) => (false, format!("IF failed: {}", e)),
                        Ok(info) if info.mode != target => (
                            false,
                            format!(
                                "IF mismatch: got {} expected {}",
                                info.mode, target
                            ),
                        ),
                        Ok(_) => (true, "ok".to_string()),
                    },
                },
            };
            results.push(DiagResult { label, round, passed, detail });
        }

        // 3. set_af_gain(128) → get_af_gain()
        {
            let label = "set_af_gain / get_af_gain";
            let target: u8 = 128;
            let (passed, detail) = match radio.set_af_gain(target).await {
                Err(e) => (false, format!("set failed: {}", e)),
                Ok(()) => match radio.get_af_gain().await {
                    Err(e) => (false, format!("get failed: {}", e)),
                    Ok(v) if v != target => (
                        false,
                        format!("mismatch: got {} expected {}", v, target),
                    ),
                    Ok(_) => (true, "ok".to_string()),
                },
            };
            results.push(DiagResult { label, round, passed, detail });
        }

        // 4. set_rf_gain(200) → get_rf_gain()
        {
            let label = "set_rf_gain / get_rf_gain";
            let target: u8 = 200;
            let (passed, detail) = match radio.set_rf_gain(target).await {
                Err(e) => (false, format!("set failed: {}", e)),
                Ok(()) => match radio.get_rf_gain().await {
                    Err(e) => (false, format!("get failed: {}", e)),
                    Ok(v) if v != target => (
                        false,
                        format!("mismatch: got {} expected {}", v, target),
                    ),
                    Ok(_) => (true, "ok".to_string()),
                },
            };
            results.push(DiagResult { label, round, passed, detail });
        }

        // 5. set_squelch(30) → get_squelch()
        {
            let label = "set_squelch / get_squelch";
            let target: u8 = 30;
            let (passed, detail) = match radio.set_squelch(target).await {
                Err(e) => (false, format!("set failed: {}", e)),
                Ok(()) => match radio.get_squelch().await {
                    Err(e) => (false, format!("get failed: {}", e)),
                    Ok(v) if v != target => (
                        false,
                        format!("mismatch: got {} expected {}", v, target),
                    ),
                    Ok(_) => (true, "ok".to_string()),
                },
            };
            results.push(DiagResult { label, round, passed, detail });
        }

        // 6. set_power(75) → get_power()
        {
            let label = "set_power / get_power";
            let target: u8 = 75;
            let (passed, detail) = match radio.set_power(target).await {
                Err(e) => (false, format!("set failed: {}", e)),
                Ok(()) => match radio.get_power().await {
                    Err(e) => (false, format!("get failed: {}", e)),
                    Ok(v) if v != target => (
                        false,
                        format!("mismatch: got {} expected {}", v, target),
                    ),
                    Ok(_) => (true, "ok".to_string()),
                },
            };
            results.push(DiagResult { label, round, passed, detail });
        }

        // 7. set_preamp(true) → get_preamp()
        {
            let label = "set_preamp / get_preamp";
            let target = true;
            let (passed, detail) = match radio.set_preamp(target).await {
                Err(e) => (false, format!("set failed: {}", e)),
                Ok(()) => match radio.get_preamp().await {
                    Err(e) => (false, format!("get failed: {}", e)),
                    Ok(v) if v != target => (
                        false,
                        format!("mismatch: got {} expected {}", v, target),
                    ),
                    Ok(_) => (true, "ok".to_string()),
                },
            };
            results.push(DiagResult { label, round, passed, detail });
        }

        // 8. set_attenuator(false) → get_attenuator()
        {
            let label = "set_attenuator / get_attenuator";
            let target = false;
            let (passed, detail) = match radio.set_attenuator(target).await {
                Err(e) => (false, format!("set failed: {}", e)),
                Ok(()) => match radio.get_attenuator().await {
                    Err(e) => (false, format!("get failed: {}", e)),
                    Ok(v) if v != target => (
                        false,
                        format!("mismatch: got {} expected {}", v, target),
                    ),
                    Ok(_) => (true, "ok".to_string()),
                },
            };
            results.push(DiagResult { label, round, passed, detail });
        }

        // 9. set_noise_blanker(true) → get_noise_blanker()
        {
            let label = "set_noise_blanker / get_noise_blanker";
            let target = true;
            let (passed, detail) = match radio.set_noise_blanker(target).await {
                Err(e) => (false, format!("set failed: {}", e)),
                Ok(()) => match radio.get_noise_blanker().await {
                    Err(e) => (false, format!("get failed: {}", e)),
                    Ok(v) if v != target => (
                        false,
                        format!("mismatch: got {} expected {}", v, target),
                    ),
                    Ok(_) => (true, "ok".to_string()),
                },
            };
            results.push(DiagResult { label, round, passed, detail });
        }

        // 10. set_agc(1) → get_agc()
        {
            let label = "set_agc / get_agc";
            let target: u8 = 1; // Slow
            let (passed, detail) = match radio.set_agc(target).await {
                Err(e) => (false, format!("set failed: {}", e)),
                Ok(()) => match radio.get_agc().await {
                    Err(e) => (false, format!("get failed: {}", e)),
                    Ok(v) if v != target => (
                        false,
                        format!("mismatch: got {} expected {}", v, target),
                    ),
                    Ok(_) => (true, "ok".to_string()),
                },
            };
            results.push(DiagResult { label, round, passed, detail });
        }

        // 11. set_rit(false) → get_rit() → get_information() rit_enabled
        {
            let label = "set_rit / get_rit";
            let target = false;
            let (passed, detail) = match radio.set_rit(target).await {
                Err(e) => (false, format!("set failed: {}", e)),
                Ok(()) => match radio.get_rit().await {
                    Err(e) => (false, format!("get failed: {}", e)),
                    Ok(v) if v != target => (
                        false,
                        format!("mismatch: got {} expected {}", v, target),
                    ),
                    Ok(_) => match radio.get_information().await {
                        Err(e) => (false, format!("IF failed: {}", e)),
                        Ok(info) if info.rit_enabled != target => (
                            false,
                            format!(
                                "IF mismatch: rit_enabled={} expected {}",
                                info.rit_enabled, target
                            ),
                        ),
                        Ok(_) => (true, "ok".to_string()),
                    },
                },
            };
            results.push(DiagResult { label, round, passed, detail });
        }

        // 12. set_vox(false) → get_vox()
        {
            let label = "set_vox / get_vox";
            let target = false;
            let (passed, detail) = match radio.set_vox(target).await {
                Err(e) => (false, format!("set failed: {}", e)),
                Ok(()) => match radio.get_vox().await {
                    Err(e) => (false, format!("get failed: {}", e)),
                    Ok(v) if v != target => (
                        false,
                        format!("mismatch: got {} expected {}", v, target),
                    ),
                    Ok(_) => (true, "ok".to_string()),
                },
            };
            results.push(DiagResult { label, round, passed, detail });
        }
    }

    results
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
                            let results = run_diagnostics(radio).await;
                            *control = ControlState::Diagnostic(DiagState::Done { results });
                            draw_frame(terminal, state, control)?;
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
