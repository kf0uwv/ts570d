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
    layout::{draw_control_panel, draw_errors, draw_header, draw_ui, split_areas},
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
        draw_control_panel(f, ctrl_area, control);
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
