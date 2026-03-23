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
    layout::{draw_control_panel, draw_header, draw_ui, split_areas},
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
        let (header_area, status_area, ctrl_area) = split_areas(area);
        draw_header(f, header_area);
        draw_ui(f, status_area, state);
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
/// Each getter failure is silently ignored — the previous value is preserved.
/// This is called at the top of every outer loop iteration and also immediately
/// after a successful command execution so the UI reflects the new state.
async fn poll_radio_state<R: Radio>(radio: &mut R, state: &mut RadioDisplay) {
    if let Ok(info) = radio.get_information().await {
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
    if let Ok(freq) = radio.get_vfo_b().await {
        state.vfo_b_hz = freq.hz();
    }
    if let Ok(s) = radio.get_smeter().await {
        state.smeter = s;
    }
    if let Ok(v) = radio.get_af_gain().await {
        state.af_gain = v;
    }
    if let Ok(v) = radio.get_rf_gain().await {
        state.rf_gain = v;
    }
    if let Ok(v) = radio.get_squelch().await {
        state.squelch = v;
    }
    if let Ok(v) = radio.get_mic_gain().await {
        state.mic_gain = v;
    }
    if let Ok(v) = radio.get_power().await {
        state.power_pct = v;
    }
    if let Ok(v) = radio.get_agc().await {
        state.agc = v;
    }
    if let Ok(v) = radio.get_noise_blanker().await {
        state.noise_blanker = v;
    }
    if let Ok(v) = radio.get_noise_reduction().await {
        state.noise_reduction = v;
    }
    if let Ok(v) = radio.get_preamp().await {
        state.preamp = v;
    }
    if let Ok(v) = radio.get_attenuator().await {
        state.attenuator = v;
    }
    if let Ok(v) = radio.get_speech_processor().await {
        state.speech_processor = v;
    }
    if let Ok(v) = radio.get_beat_cancel().await {
        state.beat_cancel = v;
    }
    if let Ok(v) = radio.get_vox().await {
        state.vox = v;
    }
    if let Ok(v) = radio.get_antenna().await {
        state.antenna = v;
    }
    if let Ok(v) = radio.get_frequency_lock().await {
        state.freq_lock = v;
    }
    if let Ok(v) = radio.get_fine_step().await {
        state.fine_step = v;
    }
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
