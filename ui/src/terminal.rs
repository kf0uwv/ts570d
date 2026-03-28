// Copyright 2026 Matt Franklin
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{self, Stdout};
use std::rc::Rc;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use framework::radio::{Frequency, MemoryChannelEntry, Mode, Radio};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    control::{handle_key, ControlState, ExecuteAction, KeyResult},
    diag::{DiagResult, DiagState, DIAG_ROUNDS},
    layout::{
        draw_control_panel, draw_diag_panel, draw_disconnected, draw_errors, draw_header, draw_ui,
        split_areas,
    },
    RadioDisplay, UiError, UiResult,
};

// ---------------------------------------------------------------------------
// Single-threaded channel primitive (Rc<RefCell<VecDeque<T>>>)
// ---------------------------------------------------------------------------

type Chan<T> = Rc<RefCell<VecDeque<T>>>;

fn make_chan<T>() -> Chan<T> {
    Rc::new(RefCell::new(VecDeque::new()))
}

fn ch_send<T>(ch: &Chan<T>, v: T) {
    ch.borrow_mut().push_back(v);
}

fn ch_recv_all<T>(ch: &Chan<T>) -> Vec<T> {
    ch.borrow_mut().drain(..).collect()
}

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

/// Commands sent from the UI task to the radio task.
enum RadioCmd {
    Execute(ExecuteAction),
    StartDiagnostics,
    Quit,
}

/// Updates sent from the radio task to the UI task.
enum RadioUpdate {
    State(RadioDisplay),
    ActionFeedback {
        ok: bool,
        msg: String,
    },
    DiagProgress {
        label: &'static str,
        round: usize,
        passed: bool,
        detail: String,
    },
    DiagDone,
}

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
        if state.initializing || !state.connected {
            draw_disconnected(f, ctrl_area, &state.poll_errors, state.initializing);
        } else if let ControlState::Diagnostic(diag) = control {
            draw_diag_panel(f, ctrl_area, diag);
        } else {
            draw_control_panel(f, ctrl_area, control);
        }
    })?;
    Ok(())
}

/// Run the radio UI with a separate radio polling task.
///
/// The radio polling/command task and the UI rendering/key-event task run
/// concurrently via `monoio::spawn_local`, so key events (including Q) are
/// always responsive regardless of radio latency.
pub async fn run<R: Radio + 'static>(radio: R) -> UiResult<()> {
    let terminal = init_terminal()?;

    let cmd_ch: Chan<RadioCmd> = make_chan();
    let update_ch: Chan<RadioUpdate> = make_chan();

    let radio_cmd_rx = Rc::clone(&cmd_ch);
    let radio_update_tx = Rc::clone(&update_ch);

    // Spawn radio task (runs concurrently on the same thread).
    let radio_handle = monoio::spawn(async move {
        radio_task(radio, radio_cmd_rx, radio_update_tx).await;
    });

    // Run UI task in this context.
    let result = ui_task(terminal, cmd_ch, update_ch).await;

    // Drop the radio task handle — this cancels the task without blocking.
    // Awaiting it would block for up to 40s while the radio task is stuck in poll_radio_state.
    drop(radio_handle);

    cleanup_terminal()?;
    result
}

/// Poll all radio state getters and update `state` in place.
///
/// `state.poll_errors` is cleared at the start of each call and re-populated
/// with any errors from this cycle. Previous values are preserved when a
/// getter fails.
async fn poll_radio_state<R: Radio>(radio: &mut R, state: &mut RadioDisplay) {
    radio.flush_rx();
    state.poll_errors.clear();

    macro_rules! poll {
        ($label:expr, $expr:expr, $ok:expr) => {
            match $expr.await {
                Ok(v) => $ok(v),
                Err(e) => {
                    if state.poll_errors.len() < 20 {
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
    poll!("FR", radio.get_rx_vfo(), |v: u8| {
        state.rx_vfo = v;
    });
    poll!("FT", radio.get_tx_vfo(), |v: u8| {
        state.tx_vfo = v;
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
    ($results:expr, $update_tx:expr, $label:expr, $round:expr, $set_expr:expr, $get_expr:expr, $target:expr) => {{
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
        ch_send(
            $update_tx,
            RadioUpdate::DiagProgress {
                label: $label,
                round: $round,
                passed,
                detail: detail.clone(),
            },
        );
        $results.push(DiagResult {
            label: $label,
            round: $round,
            passed,
            detail,
        });
    }};
}

macro_rules! diag_action {
    ($results:expr, $update_tx:expr, $label:expr, $round:expr, $expr:expr) => {{
        let (passed, detail) = match $expr.await {
            Ok(()) => (true, "ok".to_string()),
            Err(e) => (false, format!("failed: {}", e)),
        };
        ch_send(
            $update_tx,
            RadioUpdate::DiagProgress {
                label: $label,
                round: $round,
                passed,
                detail: detail.clone(),
            },
        );
        $results.push(DiagResult {
            label: $label,
            round: $round,
            passed,
            detail,
        });
    }};
}

macro_rules! diag_get {
    ($results:expr, $update_tx:expr, $label:expr, $round:expr, $expr:expr) => {{
        let (passed, detail) = match $expr.await {
            Ok(_) => (true, "ok".to_string()),
            Err(e) => (false, format!("get failed: {}", e)),
        };
        ch_send(
            $update_tx,
            RadioUpdate::DiagProgress {
                label: $label,
                round: $round,
                passed,
                detail: detail.clone(),
            },
        );
        $results.push(DiagResult {
            label: $label,
            round: $round,
            passed,
            detail,
        });
    }};
}

// ---------------------------------------------------------------------------
// Snapshot / restore helpers for run_diagnostics
// ---------------------------------------------------------------------------

/// A snapshot of all readable radio state that has a corresponding setter.
/// Every field is `Option<T>` so that individual getter failures are non-fatal.
struct RadioSnapshot {
    vfo_a: Option<framework::radio::Frequency>,
    vfo_b: Option<framework::radio::Frequency>,
    mode: Option<framework::radio::Mode>,
    af_gain: Option<u8>,
    rf_gain: Option<u8>,
    squelch: Option<u8>,
    mic_gain: Option<u8>,
    power: Option<u8>,
    agc: Option<u8>,
    noise_blanker: Option<bool>,
    noise_reduction: Option<u8>,
    preamp: Option<bool>,
    attenuator: Option<bool>,
    beat_cancel: Option<u8>,
    if_shift: Option<(char, u16)>,
    speech_processor: Option<bool>,
    vox: Option<bool>,
    vox_gain: Option<u8>,
    vox_delay: Option<u16>,
    power_on: Option<bool>,
    scan: Option<bool>,
    rx_vfo: Option<u8>,
    tx_vfo: Option<u8>,
    memory_channel: Option<u8>,
    memory_ch0: Option<MemoryChannelEntry>,
    antenna: Option<u8>,
    keyer_speed: Option<u8>,
    cw_pitch: Option<u8>,
    cw_auto_zerobeat: Option<bool>,
    semi_break_in_delay: Option<u16>,
    rit: Option<bool>,
    xit: Option<bool>,
    fine_step: Option<bool>,
    frequency_lock: Option<bool>,
    high_cutoff: Option<u8>,
    low_cutoff: Option<u8>,
    ctcss: Option<bool>,
    ctcss_tone_number: Option<u8>,
    tone: Option<bool>,
    tone_number: Option<u8>,
}

/// Snapshot all readable radio state.  Failures on individual fields are silently
/// stored as `None` — the snapshot itself always succeeds.
async fn snapshot_state<R: Radio>(radio: &mut R) -> RadioSnapshot {
    RadioSnapshot {
        vfo_a: radio.get_vfo_a().await.ok(),
        vfo_b: radio.get_vfo_b().await.ok(),
        mode: radio.get_mode().await.ok(),
        af_gain: radio.get_af_gain().await.ok(),
        rf_gain: radio.get_rf_gain().await.ok(),
        squelch: radio.get_squelch().await.ok(),
        mic_gain: radio.get_mic_gain().await.ok(),
        power: radio.get_power().await.ok(),
        agc: radio.get_agc().await.ok(),
        noise_blanker: radio.get_noise_blanker().await.ok(),
        noise_reduction: radio.get_noise_reduction().await.ok(),
        preamp: radio.get_preamp().await.ok(),
        attenuator: radio.get_attenuator().await.ok(),
        beat_cancel: radio.get_beat_cancel().await.ok(),
        if_shift: radio.get_if_shift().await.ok(),
        speech_processor: radio.get_speech_processor().await.ok(),
        vox: radio.get_vox().await.ok(),
        vox_gain: radio.get_vox_gain().await.ok(),
        vox_delay: radio.get_vox_delay().await.ok(),
        power_on: radio.get_power_on().await.ok(),
        scan: radio.get_scan().await.ok(),
        rx_vfo: radio.get_rx_vfo().await.ok(),
        tx_vfo: radio.get_tx_vfo().await.ok(),
        memory_channel: radio.get_memory_channel().await.ok(),
        memory_ch0: radio.read_memory_channel(0).await.ok(),
        antenna: radio.get_antenna().await.ok(),
        keyer_speed: radio.get_keyer_speed().await.ok(),
        cw_pitch: radio.get_cw_pitch().await.ok(),
        cw_auto_zerobeat: radio.get_cw_auto_zerobeat().await.ok(),
        semi_break_in_delay: radio.get_semi_break_in_delay().await.ok(),
        rit: radio.get_rit().await.ok(),
        xit: radio.get_xit().await.ok(),
        fine_step: radio.get_fine_step().await.ok(),
        frequency_lock: radio.get_frequency_lock().await.ok(),
        high_cutoff: radio.get_high_cutoff().await.ok(),
        low_cutoff: radio.get_low_cutoff().await.ok(),
        ctcss: radio.get_ctcss().await.ok(),
        ctcss_tone_number: radio.get_ctcss_tone_number().await.ok(),
        tone: radio.get_tone().await.ok(),
        tone_number: radio.get_tone_number().await.ok(),
    }
}

/// Restore radio state from a snapshot.  Best-effort: individual setter failures
/// are silently ignored.  PTT is cleared first via `receive()`.
/// `power_on` is restored last so other setters have time to complete first.
async fn restore_state<R: Radio>(radio: &mut R, snap: RadioSnapshot) {
    // Always clear PTT first.
    let _ = radio.receive().await;

    if let Some(v) = snap.vfo_a {
        let _ = radio.set_vfo_a(v).await;
    }
    if let Some(v) = snap.vfo_b {
        let _ = radio.set_vfo_b(v).await;
    }
    if let Some(v) = snap.mode {
        let _ = radio.set_mode(v).await;
    }
    if let Some(v) = snap.af_gain {
        let _ = radio.set_af_gain(v).await;
    }
    if let Some(v) = snap.rf_gain {
        let _ = radio.set_rf_gain(v).await;
    }
    if let Some(v) = snap.squelch {
        let _ = radio.set_squelch(v).await;
    }
    if let Some(v) = snap.mic_gain {
        let _ = radio.set_mic_gain(v).await;
    }
    if let Some(v) = snap.power {
        let _ = radio.set_power(v).await;
    }
    if let Some(v) = snap.agc {
        let _ = radio.set_agc(v).await;
    }
    if let Some(v) = snap.noise_blanker {
        let _ = radio.set_noise_blanker(v).await;
    }
    if let Some(v) = snap.noise_reduction {
        let _ = radio.set_noise_reduction(v).await;
    }
    if let Some(v) = snap.preamp {
        let _ = radio.set_preamp(v).await;
    }
    if let Some(v) = snap.attenuator {
        let _ = radio.set_attenuator(v).await;
    }
    if let Some(v) = snap.beat_cancel {
        let _ = radio.set_beat_cancel(v).await;
    }
    if let Some((dir, freq)) = snap.if_shift {
        let _ = radio.set_if_shift(dir, freq).await;
    }
    if let Some(v) = snap.speech_processor {
        let _ = radio.set_speech_processor(v).await;
    }
    if let Some(v) = snap.vox {
        let _ = radio.set_vox(v).await;
    }
    if let Some(v) = snap.vox_gain {
        let _ = radio.set_vox_gain(v).await;
    }
    if let Some(v) = snap.vox_delay {
        let _ = radio.set_vox_delay(v).await;
    }
    if let Some(v) = snap.scan {
        let _ = radio.set_scan(v).await;
    }
    if let Some(v) = snap.rx_vfo {
        let _ = radio.set_rx_vfo(v).await;
    }
    if let Some(v) = snap.tx_vfo {
        let _ = radio.set_tx_vfo(v).await;
    }
    if let Some(v) = snap.memory_channel {
        let _ = radio.set_memory_channel(v).await;
    }
    // Restore memory channel 0: if it was vacant, clear it; otherwise write it back.
    match snap.memory_ch0 {
        Some(entry) if entry.vacant => {
            let _ = radio.clear_memory_channel(0).await;
        }
        Some(entry) => {
            let _ = radio.write_memory_channel(0, entry).await;
        }
        None => {}
    }
    if let Some(v) = snap.antenna {
        let _ = radio.set_antenna(v).await;
    }
    if let Some(v) = snap.keyer_speed {
        let _ = radio.set_keyer_speed(v).await;
    }
    if let Some(v) = snap.cw_pitch {
        let _ = radio.set_cw_pitch(v).await;
    }
    if let Some(v) = snap.cw_auto_zerobeat {
        let _ = radio.set_cw_auto_zerobeat(v).await;
    }
    if let Some(v) = snap.semi_break_in_delay {
        let _ = radio.set_semi_break_in_delay(v).await;
    }
    if let Some(v) = snap.rit {
        let _ = radio.set_rit(v).await;
    }
    if let Some(v) = snap.xit {
        let _ = radio.set_xit(v).await;
    }
    if let Some(v) = snap.fine_step {
        let _ = radio.set_fine_step(v).await;
    }
    if let Some(v) = snap.frequency_lock {
        let _ = radio.set_frequency_lock(v).await;
    }
    if let Some(v) = snap.high_cutoff {
        let _ = radio.set_high_cutoff(v).await;
    }
    if let Some(v) = snap.low_cutoff {
        let _ = radio.set_low_cutoff(v).await;
    }
    if let Some(v) = snap.ctcss {
        let _ = radio.set_ctcss(v).await;
    }
    if let Some(v) = snap.ctcss_tone_number {
        let _ = radio.set_ctcss_tone_number(v).await;
    }
    if let Some(v) = snap.tone {
        let _ = radio.set_tone(v).await;
    }
    if let Some(v) = snap.tone_number {
        let _ = radio.set_tone_number(v).await;
    }
    // Restore power_on last — if it was off, the above restores still need to complete first.
    if let Some(v) = snap.power_on {
        let _ = radio.set_power_on(v).await;
    }
}

// Each step is (label, closure returning future).  We collect them as a
// sequence of closures so we can update the Running state before each one.
// Since futures are not object-safe we instead encode the step list as a
// static slice of labels and drive the execution in a plain match below.

/// Total number of unique diagnostic steps (one per method).
pub(crate) const DIAG_STEP_COUNT: usize = 107;

// ---------------------------------------------------------------------------
// Diagnostic task (sends DiagProgress / DiagDone to the UI task)
// ---------------------------------------------------------------------------

/// Run all diagnostic commands, sending `DiagProgress` updates via `update_tx`
/// and a final `DiagDone` when complete.
///
/// Each step covers exactly one method. Pressing [Esc] between steps aborts.
async fn run_diagnostics_task<R: Radio>(radio: &mut R, update_tx: &Chan<RadioUpdate>) {
    let mut results: Vec<DiagResult> = Vec::new();

    // Snapshot all readable radio state before the test loop begins.
    let snapshot = snapshot_state(radio).await;

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
        // --- Memory (5) ---
        "set_memory_channel",   // 64
        "get_memory_channel",   // 65
        "read_memory_channel",  // 66
        "write_memory_channel", // 67
        "clear_memory_channel", // 68
        // --- Antenna (5) ---
        "set_antenna(1)",         // 69
        "set_antenna(2)",         // 70
        "get_antenna",            // 71
        "set_antenna_tuner_thru", // 72
        "start_antenna_tuning",   // 73
        // --- CW (9) ---
        "set_keyer_speed",         // 74
        "get_keyer_speed",         // 75
        "set_cw_pitch",            // 76
        "get_cw_pitch",            // 77
        "set_cw_auto_zerobeat",    // 78
        "get_cw_auto_zerobeat",    // 79
        "set_semi_break_in_delay", // 80
        "get_semi_break_in_delay", // 81
        "send_cw(TEST)",           // 82
        // --- Audio filter (4) ---
        "set_high_cutoff", // 83
        "get_high_cutoff", // 84
        "set_low_cutoff",  // 85
        "get_low_cutoff",  // 86
        // --- CTCSS / Tone (8) ---
        "set_ctcss_tone_number", // 87
        "get_ctcss_tone_number", // 88
        "set_ctcss",             // 89
        "get_ctcss",             // 90
        "set_tone_number",       // 91
        "get_tone_number",       // 92
        "set_tone",              // 93
        "get_tone",              // 94
        // --- Meters (2) ---
        "get_smeter",     // 95
        "get_meter(RM1)", // 96
        // --- Identity / Info (3) ---
        "get_id",          // 97
        "get_information", // 98
        "is_busy",         // 99
        // --- Misc actions (5) ---
        "mic_up",        // 100
        "mic_down",      // 101
        "set_auto_info", // 102
        "voice_recall",  // 103
        "reset",         // 104
        // --- IF cross-checks (2) ---
        "if_crosscheck:vfo_a", // 105
        "if_crosscheck:mode",  // 106
    ];

    'outer: for round in 1..=DIAG_ROUNDS {
        for (step_idx, &label) in LABELS.iter().enumerate() {
            // Abort check
            if check_esc() {
                break 'outer;
            }

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
                    ch_send(
                        update_tx,
                        RadioUpdate::DiagProgress {
                            label,
                            round,
                            passed,
                            detail: detail.clone(),
                        },
                    );
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
                    ch_send(
                        update_tx,
                        RadioUpdate::DiagProgress {
                            label,
                            round,
                            passed,
                            detail: detail.clone(),
                        },
                    );
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
                    ch_send(
                        update_tx,
                        RadioUpdate::DiagProgress {
                            label,
                            round,
                            passed,
                            detail: detail.clone(),
                        },
                    );
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
                    ch_send(
                        update_tx,
                        RadioUpdate::DiagProgress {
                            label,
                            round,
                            passed,
                            detail: detail.clone(),
                        },
                    );
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
                    ch_send(
                        update_tx,
                        RadioUpdate::DiagProgress {
                            label,
                            round,
                            passed,
                            detail: detail.clone(),
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // get_fine_step: verify Ok
                5 => {
                    diag_get!(results, update_tx, label, round, radio.get_fine_step());
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
                    ch_send(
                        update_tx,
                        RadioUpdate::DiagProgress {
                            label,
                            round,
                            passed,
                            detail: detail.clone(),
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // get_frequency_lock: verify Ok
                7 => {
                    diag_get!(results, update_tx, label, round, radio.get_frequency_lock());
                }

                // --- Mode ---

                // set_mode(USB): set Usb, verify Ok + get == Usb
                8 => {
                    let target = Mode::Usb;
                    diag_set_get!(
                        results,
                        update_tx,
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
                        update_tx,
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
                        update_tx,
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
                        update_tx,
                        label,
                        round,
                        radio.set_mode(target),
                        radio.get_mode(),
                        target
                    );
                }

                // get_mode: verify Ok
                12 => {
                    diag_get!(results, update_tx, label, round, radio.get_mode());
                }

                // --- RIT/XIT ---

                // set_rit(on): set true, verify Ok
                13 => {
                    diag_action!(results, update_tx, label, round, radio.set_rit(true));
                }

                // set_rit(off): set false, verify Ok + get == false
                14 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_rit(target),
                        radio.get_rit(),
                        target
                    );
                }

                // get_rit: verify Ok
                15 => {
                    diag_get!(results, update_tx, label, round, radio.get_rit());
                }

                // clear_rit: verify Ok
                16 => {
                    diag_action!(results, update_tx, label, round, radio.clear_rit());
                }

                // rit_up: verify Ok
                17 => {
                    diag_action!(results, update_tx, label, round, radio.rit_up());
                }

                // rit_down: verify Ok
                18 => {
                    diag_action!(results, update_tx, label, round, radio.rit_down());
                }

                // set_xit(on): set true, verify Ok
                19 => {
                    diag_action!(results, update_tx, label, round, radio.set_xit(true));
                }

                // set_xit(off): set false, verify Ok + get == false
                20 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_xit(target),
                        radio.get_xit(),
                        target
                    );
                }

                // get_xit: verify Ok
                21 => {
                    diag_get!(results, update_tx, label, round, radio.get_xit());
                }

                // --- Gains ---

                // set_af_gain: set 128, verify Ok + get == 128
                22 => {
                    let target: u8 = 128;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_af_gain(target),
                        radio.get_af_gain(),
                        target
                    );
                }

                // get_af_gain: verify Ok
                23 => {
                    diag_get!(results, update_tx, label, round, radio.get_af_gain());
                }

                // set_rf_gain: set 200, verify Ok + get == 200
                24 => {
                    let target: u8 = 200;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_rf_gain(target),
                        radio.get_rf_gain(),
                        target
                    );
                }

                // get_rf_gain: verify Ok
                25 => {
                    diag_get!(results, update_tx, label, round, radio.get_rf_gain());
                }

                // set_squelch: set 30, verify Ok + get == 30
                26 => {
                    let target: u8 = 30;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_squelch(target),
                        radio.get_squelch(),
                        target
                    );
                }

                // get_squelch: verify Ok
                27 => {
                    diag_get!(results, update_tx, label, round, radio.get_squelch());
                }

                // set_mic_gain: set 50, verify Ok + get == 50
                28 => {
                    let target: u8 = 50;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_mic_gain(target),
                        radio.get_mic_gain(),
                        target
                    );
                }

                // get_mic_gain: verify Ok
                29 => {
                    diag_get!(results, update_tx, label, round, radio.get_mic_gain());
                }

                // set_power: set 75, verify Ok + get == 75
                30 => {
                    let target: u8 = 75;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_power(target),
                        radio.get_power(),
                        target
                    );
                }

                // get_power: verify Ok
                31 => {
                    diag_get!(results, update_tx, label, round, radio.get_power());
                }

                // --- Receiver features ---

                // set_agc(Slow): set 1, verify Ok + get == 1
                32 => {
                    let target: u8 = 1;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_agc(target),
                        radio.get_agc(),
                        target
                    );
                }

                // get_agc: verify Ok
                33 => {
                    diag_get!(results, update_tx, label, round, radio.get_agc());
                }

                // set_noise_blanker: set true, verify Ok + get == true
                34 => {
                    let target = true;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_noise_blanker(target),
                        radio.get_noise_blanker(),
                        target
                    );
                }

                // get_noise_blanker: verify Ok
                35 => {
                    diag_get!(results, update_tx, label, round, radio.get_noise_blanker());
                }

                // set_noise_reduction: set 1, verify Ok + get == 1
                36 => {
                    let target: u8 = 1;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_noise_reduction(target),
                        radio.get_noise_reduction(),
                        target
                    );
                }

                // get_noise_reduction: verify Ok
                37 => {
                    diag_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.get_noise_reduction()
                    );
                }

                // set_preamp: set true, verify Ok + get == true
                38 => {
                    let target = true;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_preamp(target),
                        radio.get_preamp(),
                        target
                    );
                }

                // get_preamp: verify Ok
                39 => {
                    diag_get!(results, update_tx, label, round, radio.get_preamp());
                }

                // set_attenuator: set false, verify Ok + get == false
                40 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_attenuator(target),
                        radio.get_attenuator(),
                        target
                    );
                }

                // get_attenuator: verify Ok
                41 => {
                    diag_get!(results, update_tx, label, round, radio.get_attenuator());
                }

                // set_beat_cancel: set 0, verify Ok + get == 0
                42 => {
                    let target: u8 = 0;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_beat_cancel(target),
                        radio.get_beat_cancel(),
                        target
                    );
                }

                // get_beat_cancel: verify Ok
                43 => {
                    diag_get!(results, update_tx, label, round, radio.get_beat_cancel());
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
                    ch_send(
                        update_tx,
                        RadioUpdate::DiagProgress {
                            label,
                            round,
                            passed,
                            detail: detail.clone(),
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // get_if_shift: verify Ok
                45 => {
                    diag_get!(results, update_tx, label, round, radio.get_if_shift());
                }

                // --- TX features ---

                // set_speech_processor: set false, verify Ok + get == false
                46 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_speech_processor(target),
                        radio.get_speech_processor(),
                        target
                    );
                }

                // get_speech_processor: verify Ok
                47 => {
                    diag_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.get_speech_processor()
                    );
                }

                // set_vox: set false, verify Ok + get == false
                48 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_vox(target),
                        radio.get_vox(),
                        target
                    );
                }

                // get_vox: verify Ok
                49 => {
                    diag_get!(results, update_tx, label, round, radio.get_vox());
                }

                // set_vox_gain: set 5, verify Ok + get == 5
                50 => {
                    let target: u8 = 5;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_vox_gain(target),
                        radio.get_vox_gain(),
                        target
                    );
                }

                // get_vox_gain: verify Ok
                51 => {
                    diag_get!(results, update_tx, label, round, radio.get_vox_gain());
                }

                // set_vox_delay: set 300, verify Ok + get == 300
                52 => {
                    let target: u16 = 300;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_vox_delay(target),
                        radio.get_vox_delay(),
                        target
                    );
                }

                // get_vox_delay: verify Ok
                53 => {
                    diag_get!(results, update_tx, label, round, radio.get_vox_delay());
                }

                // set_power_on: set true, verify Ok + get == true
                54 => {
                    let target = true;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_power_on(target),
                        radio.get_power_on(),
                        target
                    );
                }

                // get_power_on: verify Ok
                55 => {
                    diag_get!(results, update_tx, label, round, radio.get_power_on());
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
                    ch_send(
                        update_tx,
                        RadioUpdate::DiagProgress {
                            label,
                            round,
                            passed,
                            detail: detail.clone(),
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // receive: call receive(), verify Ok
                57 => {
                    diag_action!(results, update_tx, label, round, radio.receive());
                }

                // --- Scan ---

                // set_scan: set false, verify Ok + get == false
                58 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_scan(target),
                        radio.get_scan(),
                        target
                    );
                }

                // get_scan: verify Ok
                59 => {
                    diag_get!(results, update_tx, label, round, radio.get_scan());
                }

                // --- VFO routing ---

                // set_rx_vfo: set 0, verify Ok + get == 0
                60 => {
                    let target: u8 = 0;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_rx_vfo(target),
                        radio.get_rx_vfo(),
                        target
                    );
                }

                // get_rx_vfo: verify Ok
                61 => {
                    diag_get!(results, update_tx, label, round, radio.get_rx_vfo());
                }

                // set_tx_vfo: set 0, verify Ok + get == 0
                62 => {
                    let target: u8 = 0;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_tx_vfo(target),
                        radio.get_tx_vfo(),
                        target
                    );
                }

                // get_tx_vfo: verify Ok
                63 => {
                    diag_get!(results, update_tx, label, round, radio.get_tx_vfo());
                }

                // --- Memory ---

                // set_memory_channel: set 1, verify Ok + get == 1
                64 => {
                    let target: u8 = 1;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_memory_channel(target),
                        radio.get_memory_channel(),
                        target
                    );
                }

                // get_memory_channel: verify Ok
                65 => {
                    diag_get!(results, update_tx, label, round, radio.get_memory_channel());
                }

                // read_memory_channel: read channel 0, verify Ok (may be vacant)
                66 => {
                    diag_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.read_memory_channel(0)
                    );
                }

                // write_memory_channel: write a USB entry to channel 0, verify Ok
                67 => {
                    let entry = MemoryChannelEntry {
                        channel: 0,
                        split: false,
                        freq_hz: 14_195_000,
                        mode: 2, // USB
                        lockout: false,
                        tone_type: 0,
                        tone_number: 0,
                        vacant: false,
                    };
                    diag_action!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.write_memory_channel(0, entry)
                    );
                }

                // clear_memory_channel: clear channel 0, verify Ok
                68 => {
                    diag_action!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.clear_memory_channel(0)
                    );
                }

                // --- Antenna ---

                // set_antenna(1): set 1, verify Ok + get == 1
                69 => {
                    let target: u8 = 1;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_antenna(target),
                        radio.get_antenna(),
                        target
                    );
                }

                // set_antenna(2): set 2, verify Ok + get == 2
                70 => {
                    let target: u8 = 2;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_antenna(target),
                        radio.get_antenna(),
                        target
                    );
                }

                // get_antenna: verify Ok
                71 => {
                    diag_get!(results, update_tx, label, round, radio.get_antenna());
                }

                // set_antenna_tuner_thru: call, verify Ok
                72 => {
                    diag_action!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_antenna_tuner_thru(true)
                    );
                }

                // start_antenna_tuning: call, verify Ok
                73 => {
                    diag_action!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.start_antenna_tuning()
                    );
                }

                // --- CW ---

                // set_keyer_speed: set 20, verify Ok + get == 20
                74 => {
                    let target: u8 = 20;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_keyer_speed(target),
                        radio.get_keyer_speed(),
                        target
                    );
                }

                // get_keyer_speed: verify Ok
                75 => {
                    diag_get!(results, update_tx, label, round, radio.get_keyer_speed());
                }

                // set_cw_pitch: set index 7 (~600 Hz), verify Ok + get == 7
                76 => {
                    let target: u8 = 7;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_cw_pitch(target),
                        radio.get_cw_pitch(),
                        target
                    );
                }

                // get_cw_pitch: verify Ok
                77 => {
                    diag_get!(results, update_tx, label, round, radio.get_cw_pitch());
                }

                // set_cw_auto_zerobeat: set false, verify Ok + get == false
                78 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_cw_auto_zerobeat(target),
                        radio.get_cw_auto_zerobeat(),
                        target
                    );
                }

                // get_cw_auto_zerobeat: verify Ok
                79 => {
                    diag_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.get_cw_auto_zerobeat()
                    );
                }

                // set_semi_break_in_delay: set 50, verify Ok + get == 50
                80 => {
                    let target: u16 = 50;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_semi_break_in_delay(target),
                        radio.get_semi_break_in_delay(),
                        target
                    );
                }

                // get_semi_break_in_delay: verify Ok
                81 => {
                    diag_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.get_semi_break_in_delay()
                    );
                }

                // send_cw("TEST"): call, verify Ok
                82 => {
                    diag_action!(results, update_tx, label, round, radio.send_cw("TEST"));
                }

                // --- Audio filter ---

                // set_high_cutoff: set index 14, verify Ok + get == 14
                83 => {
                    let target: u8 = 14;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_high_cutoff(target),
                        radio.get_high_cutoff(),
                        target
                    );
                }

                // get_high_cutoff: verify Ok
                84 => {
                    diag_get!(results, update_tx, label, round, radio.get_high_cutoff());
                }

                // set_low_cutoff: set index 3, verify Ok + get == 3
                85 => {
                    let target: u8 = 3;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_low_cutoff(target),
                        radio.get_low_cutoff(),
                        target
                    );
                }

                // get_low_cutoff: verify Ok
                86 => {
                    diag_get!(results, update_tx, label, round, radio.get_low_cutoff());
                }

                // --- CTCSS / Tone ---

                // set_ctcss_tone_number: set 1, verify Ok + get == 1
                87 => {
                    let target: u8 = 1;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_ctcss_tone_number(target),
                        radio.get_ctcss_tone_number(),
                        target
                    );
                }

                // get_ctcss_tone_number: verify Ok
                88 => {
                    diag_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.get_ctcss_tone_number()
                    );
                }

                // set_ctcss: set false, verify Ok + get == false
                89 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_ctcss(target),
                        radio.get_ctcss(),
                        target
                    );
                }

                // get_ctcss: verify Ok
                90 => {
                    diag_get!(results, update_tx, label, round, radio.get_ctcss());
                }

                // set_tone_number: set 1, verify Ok + get == 1
                91 => {
                    let target: u8 = 1;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_tone_number(target),
                        radio.get_tone_number(),
                        target
                    );
                }

                // get_tone_number: verify Ok
                92 => {
                    diag_get!(results, update_tx, label, round, radio.get_tone_number());
                }

                // set_tone: set false, verify Ok + get == false
                93 => {
                    let target = false;
                    diag_set_get!(
                        results,
                        update_tx,
                        label,
                        round,
                        radio.set_tone(target),
                        radio.get_tone(),
                        target
                    );
                }

                // get_tone: verify Ok
                94 => {
                    diag_get!(results, update_tx, label, round, radio.get_tone());
                }

                // --- Meters ---

                // get_smeter: verify Ok, value 0..=30
                95 => {
                    let (passed, detail) = match radio.get_smeter().await {
                        Err(e) => (false, format!("get failed: {}", e)),
                        Ok(v) if v > 30 => (false, format!("out of range: {} > 30", v)),
                        Ok(_) => (true, "ok".to_string()),
                    };
                    ch_send(
                        update_tx,
                        RadioUpdate::DiagProgress {
                            label,
                            round,
                            passed,
                            detail: detail.clone(),
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // get_meter(RM1): verify Ok or error accepted
                96 => {
                    let (passed, detail) = match radio.get_meter(1).await {
                        Ok(_) => (true, "ok".to_string()),
                        Err(e) => (false, format!("failed: {}", e)),
                    };
                    ch_send(
                        update_tx,
                        RadioUpdate::DiagProgress {
                            label,
                            round,
                            passed,
                            detail: detail.clone(),
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // --- Identity / Info ---

                // get_id: verify Ok, non-zero
                97 => {
                    let (passed, detail) = match radio.get_id().await {
                        Err(e) => (false, format!("get failed: {}", e)),
                        Ok(0) => (false, "id returned 0".to_string()),
                        Ok(_) => (true, "ok".to_string()),
                    };
                    ch_send(
                        update_tx,
                        RadioUpdate::DiagProgress {
                            label,
                            round,
                            passed,
                            detail: detail.clone(),
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // get_information: verify Ok (all fields parse)
                98 => {
                    diag_get!(results, update_tx, label, round, radio.get_information());
                }

                // is_busy: verify Ok
                99 => {
                    diag_get!(results, update_tx, label, round, radio.is_busy());
                }

                // --- Misc actions ---

                // mic_up: verify Ok
                100 => {
                    diag_action!(results, update_tx, label, round, radio.mic_up());
                }

                // mic_down: verify Ok
                101 => {
                    diag_action!(results, update_tx, label, round, radio.mic_down());
                }

                // set_auto_info: set false (0), verify Ok
                102 => {
                    diag_action!(results, update_tx, label, round, radio.set_auto_info(0));
                }

                // voice_recall: call voice 1, verify Ok
                103 => {
                    diag_action!(results, update_tx, label, round, radio.voice_recall(1));
                }

                // reset: partial reset, verify Ok
                104 => {
                    diag_action!(results, update_tx, label, round, radio.reset(false));
                }

                // --- IF cross-checks ---

                // if_crosscheck:vfo_a — set_vfo_a then verify get_information().frequency
                105 => {
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
                    ch_send(
                        update_tx,
                        RadioUpdate::DiagProgress {
                            label,
                            round,
                            passed,
                            detail: detail.clone(),
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                // if_crosscheck:mode — set_mode(Usb) then verify get_information().mode
                106 => {
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
                    ch_send(
                        update_tx,
                        RadioUpdate::DiagProgress {
                            label,
                            round,
                            passed,
                            detail: detail.clone(),
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }

                _ => {
                    // Defensive: should never reach here
                    let passed = false;
                    let detail = "unimplemented step".to_string();
                    ch_send(
                        update_tx,
                        RadioUpdate::DiagProgress {
                            label,
                            round,
                            passed,
                            detail: detail.clone(),
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                }
            }
        }
    }

    // Restore all snapshotted radio state unconditionally (best-effort).
    restore_state(radio, snapshot).await;

    // Signal the UI task that diagnostics are complete.
    ch_send(update_tx, RadioUpdate::DiagDone);
}

// ---------------------------------------------------------------------------
// Radio task — owns the radio, polls state, executes commands
// ---------------------------------------------------------------------------

async fn radio_task<R: Radio + 'static>(
    mut radio: R,
    cmd_rx: Chan<RadioCmd>,
    update_tx: Chan<RadioUpdate>,
) {
    let mut fail_cycles: u32 = 0;
    let mut if_shift_dir: char = ' ';

    loop {
        // 1. Poll radio state.
        let mut state = RadioDisplay {
            initializing: false,
            ..RadioDisplay::default()
        };
        poll_radio_state(&mut radio, &mut state).await;

        // 2. Update connection health.
        const FAIL_THRESHOLD: usize = 10;
        if state.poll_errors.len() >= FAIL_THRESHOLD {
            fail_cycles = fail_cycles.saturating_add(1);
        } else {
            fail_cycles = 0;
        }
        state.connected = fail_cycles < 3;
        state.initializing = false;

        // 3. Send state snapshot to UI task.
        ch_send(&update_tx, RadioUpdate::State(state));

        // 4. Process all pending commands from the UI task.
        for cmd in ch_recv_all(&cmd_rx) {
            match cmd {
                RadioCmd::Quit => return,
                RadioCmd::Execute(action) => {
                    let (desc, result) =
                        execute_action(&mut radio, action, &mut if_shift_dir).await;
                    let (ok, msg) = match result {
                        Ok(m) => (
                            true,
                            if m.is_empty() {
                                format!("OK: {}", desc)
                            } else {
                                m
                            },
                        ),
                        Err(e) => (false, format!("Error: {}", e)),
                    };
                    ch_send(&update_tx, RadioUpdate::ActionFeedback { ok, msg });
                }
                RadioCmd::StartDiagnostics => {
                    run_diagnostics_task(&mut radio, &update_tx).await;
                }
            }
        }

        // 5. Yield ~200ms before next poll cycle.
        monoio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

// ---------------------------------------------------------------------------
// UI task — renders frames and handles key events
// ---------------------------------------------------------------------------

async fn ui_task(
    mut terminal: Terminal<CrosstermBackend<Stdout>>,
    cmd_tx: Chan<RadioCmd>,
    update_rx: Chan<RadioUpdate>,
) -> UiResult<()> {
    let mut state = RadioDisplay::default(); // initializing=true by default
    let mut control = ControlState::Menu;
    let mut if_shift_dir: char = ' ';
    let mut diag_results: Vec<DiagResult> = Vec::new();

    // Draw initial connecting frame immediately.
    draw_frame(&mut terminal, &state, &control)?;

    loop {
        // 1. Drain all pending radio updates.
        for update in ch_recv_all(&update_rx) {
            match update {
                RadioUpdate::State(s) => state = s,
                RadioUpdate::ActionFeedback { ok, msg } => {
                    control = ControlState::Feedback {
                        message: msg,
                        is_error: !ok,
                    };
                }
                RadioUpdate::DiagProgress {
                    label,
                    round,
                    passed,
                    detail,
                } => {
                    diag_results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                    });
                    if let ControlState::Diagnostic(DiagState::Running {
                        ref mut current_label,
                        ref mut current_round,
                        ref mut results,
                    }) = control
                    {
                        *current_label = label;
                        *current_round = round;
                        *results = diag_results.clone();
                    }
                }
                RadioUpdate::DiagDone => {
                    let results = std::mem::take(&mut diag_results);
                    control = ControlState::Diagnostic(DiagState::Done { results, scroll: 0 });
                }
            }
        }

        // 2. Draw frame.
        draw_frame(&mut terminal, &state, &control)?;

        // 3. Handle key events (non-blocking, 10ms poll window).
        if event::poll(std::time::Duration::from_millis(10)).map_err(UiError::Io)? {
            if let Event::Key(key) = event::read().map_err(UiError::Io)? {
                match handle_key(key, &mut control, &state) {
                    KeyResult::Quit => {
                        ch_send(&cmd_tx, RadioCmd::Quit);
                        return Ok(());
                    }
                    KeyResult::Continue => {}
                    KeyResult::StartDiag => {
                        diag_results.clear();
                        control = ControlState::Diagnostic(DiagState::Running {
                            current_label: "starting\u{2026}",
                            current_round: 1,
                            results: Vec::new(),
                        });
                        ch_send(&cmd_tx, RadioCmd::StartDiagnostics);
                    }
                    KeyResult::Execute(action) => {
                        // SetIfShiftDir is UI-only — no radio call needed.
                        if let ExecuteAction::SetIfShiftDir(dir) = action {
                            if_shift_dir = dir;
                            let dir_name = match dir {
                                '+' => "+",
                                '-' => "-",
                                _ => "Center",
                            };
                            control = ControlState::Feedback {
                                message: format!("IF shift direction set to {}", dir_name),
                                is_error: false,
                            };
                        } else {
                            // For SetIfShift, embed the stored direction.
                            let action = if let ExecuteAction::SetIfShift(_, freq) = action {
                                ExecuteAction::SetIfShift(if_shift_dir, freq)
                            } else {
                                action
                            };
                            ch_send(&cmd_tx, RadioCmd::Execute(action));
                        }
                    }
                }
            }
        }

        // 4. Yield briefly so the radio task can run.
        monoio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

/// Execute a radio action, returning a human-readable description and the result.
///
/// Returns `(&'static str, RadioResult<String>)` where the `String` is extra feedback
/// (non-empty for `ReadMemoryChannel`, empty for all other commands).
/// `if_shift_dir` is updated in-place when `SetIfShiftDir` is processed.
async fn execute_action<R: Radio>(
    radio: &mut R,
    action: ExecuteAction,
    if_shift_dir: &mut char,
) -> (&'static str, framework::radio::RadioResult<String>) {
    use framework::radio::{MemoryChannelEntry, RadioResult};
    use ExecuteAction::*;

    // Helper to convert a unit result to a String result
    fn ok_unit(r: RadioResult<()>) -> RadioResult<String> {
        r.map(|()| String::new())
    }

    match action {
        SetVfoA(hz) => {
            let r = match Frequency::new(hz) {
                Ok(f) => radio.set_vfo_a(f).await,
                Err(e) => Err(e),
            };
            ("VFO A set", ok_unit(r))
        }
        SetVfoB(hz) => {
            let r = match Frequency::new(hz) {
                Ok(f) => radio.set_vfo_b(f).await,
                Err(e) => Err(e),
            };
            ("VFO B set", ok_unit(r))
        }
        SetAfGain(v) => ("AF gain set", ok_unit(radio.set_af_gain(v).await)),
        SetRfGain(v) => ("RF gain set", ok_unit(radio.set_rf_gain(v).await)),
        SetSqLevel(v) => ("Squelch set", ok_unit(radio.set_squelch(v).await)),
        SetMicGain(v) => ("MIC gain set", ok_unit(radio.set_mic_gain(v).await)),
        SetPower(v) => ("TX power set", ok_unit(radio.set_power(v).await)),
        SetVoxGain(v) => ("VOX gain set", ok_unit(radio.set_vox_gain(v).await)),
        SetVoxDelay(v) => ("VOX delay set", ok_unit(radio.set_vox_delay(v).await)),
        SetKeyerSpeed(v) => ("Keyer speed set", ok_unit(radio.set_keyer_speed(v).await)),
        SetMode(m) => {
            let r = match Mode::try_from(m) {
                Ok(mode) => radio.set_mode(mode).await,
                Err(e) => Err(e),
            };
            ("Mode set", ok_unit(r))
        }
        SetAgc(v) => ("AGC set", ok_unit(radio.set_agc(v).await)),
        SetNoiseReduction(v) => (
            "Noise reduction set",
            ok_unit(radio.set_noise_reduction(v).await),
        ),
        SetAntenna(v) => ("Antenna set", ok_unit(radio.set_antenna(v).await)),
        ToggleRit(on) => ("RIT toggled", ok_unit(radio.set_rit(on).await)),
        ToggleXit(on) => ("XIT toggled", ok_unit(radio.set_xit(on).await)),
        ToggleNb(on) => (
            "Noise blanker toggled",
            ok_unit(radio.set_noise_blanker(on).await),
        ),
        TogglePreamp(on) => ("Preamp toggled", ok_unit(radio.set_preamp(on).await)),
        ToggleAtt(on) => (
            "Attenuator toggled",
            ok_unit(radio.set_attenuator(on).await),
        ),
        ToggleVox(on) => ("VOX toggled", ok_unit(radio.set_vox(on).await)),
        ToggleScan(on) => ("Scan toggled", ok_unit(radio.set_scan(on).await)),
        ToggleLock(on) => (
            "Frequency lock toggled",
            ok_unit(radio.set_frequency_lock(on).await),
        ),
        ToggleFine(on) => ("Fine step toggled", ok_unit(radio.set_fine_step(on).await)),

        // --- Frequency group ---
        SetRxVfo(v) => ("RX VFO set", ok_unit(radio.set_rx_vfo(v).await)),
        SetTxVfo(v) => ("TX VFO set", ok_unit(radio.set_tx_vfo(v).await)),
        ClearRit => ("RIT cleared", ok_unit(radio.clear_rit().await)),
        RitUp => ("RIT up", ok_unit(radio.rit_up().await)),
        RitDown => ("RIT down", ok_unit(radio.rit_down().await)),

        // --- Memory group ---
        SelectMemoryChannel(ch) => (
            "Memory channel selected",
            ok_unit(radio.set_memory_channel(ch).await),
        ),
        ReadMemoryChannel(ch) => {
            let result = radio.read_memory_channel(ch).await;
            match result {
                Ok(entry) => {
                    let mode_name = match entry.mode {
                        1 => "LSB",
                        2 => "USB",
                        3 => "CW",
                        4 => "FM",
                        5 => "AM",
                        6 => "FSK",
                        7 => "CW-R",
                        9 => "FSK-R",
                        _ => "?",
                    };
                    let msg = if entry.vacant {
                        format!("CH {:02}: vacant", ch)
                    } else {
                        let mhz = entry.freq_hz as f64 / 1_000_000.0;
                        let lock_str = if entry.lockout { " [locked]" } else { "" };
                        let tone_str = if entry.tone_type != 0 {
                            format!(" [tone#{}]", entry.tone_number)
                        } else {
                            String::new()
                        };
                        format!(
                            "CH {:02}: {:.6} MHz {}{}{}",
                            ch, mhz, mode_name, lock_str, tone_str
                        )
                    };
                    ("Memory channel read", Ok(msg))
                }
                Err(e) => ("Memory channel read", Err(e)),
            }
        }
        WriteMemoryChannelFromVfoA(ch) => {
            let freq_result = radio.get_vfo_a().await;
            let mode_result = radio.get_mode().await;
            match (freq_result, mode_result) {
                (Ok(freq), Ok(mode)) => {
                    let entry = MemoryChannelEntry {
                        channel: ch,
                        split: false,
                        freq_hz: freq.hz(),
                        mode: mode.as_u8(),
                        lockout: false,
                        tone_type: 0,
                        tone_number: 0,
                        vacant: false,
                    };
                    (
                        "Memory channel written from VFO A",
                        ok_unit(radio.write_memory_channel(ch, entry).await),
                    )
                }
                (Err(e), _) | (_, Err(e)) => ("Memory channel written from VFO A", Err(e)),
            }
        }
        WriteMemoryChannelFromVfoB(ch) => {
            let freq_result = radio.get_vfo_b().await;
            let mode_result = radio.get_mode().await;
            match (freq_result, mode_result) {
                (Ok(freq), Ok(mode)) => {
                    let entry = MemoryChannelEntry {
                        channel: ch,
                        split: false,
                        freq_hz: freq.hz(),
                        mode: mode.as_u8(),
                        lockout: false,
                        tone_type: 0,
                        tone_number: 0,
                        vacant: false,
                    };
                    (
                        "Memory channel written from VFO B",
                        ok_unit(radio.write_memory_channel(ch, entry).await),
                    )
                }
                (Err(e), _) | (_, Err(e)) => ("Memory channel written from VFO B", Err(e)),
            }
        }
        ClearMemoryChannel(ch) => (
            "Memory channel cleared",
            ok_unit(radio.clear_memory_channel(ch).await),
        ),

        // --- Mode/DSP group ---
        SetBeatCancel(v) => ("Beat cancel set", ok_unit(radio.set_beat_cancel(v).await)),
        SetIfShiftDir(dir) => {
            // Store the direction locally, no radio call
            *if_shift_dir = dir;
            let dir_name = match dir {
                '+' => "+",
                '-' => "-",
                _ => "Center",
            };
            (
                "IF shift direction set",
                Ok(format!("IF shift direction set to {}", dir_name)),
            )
        }
        SetIfShift(_placeholder_dir, freq) => {
            // Use the stored direction
            let dir = *if_shift_dir;
            ("IF shift set", ok_unit(radio.set_if_shift(dir, freq).await))
        }
        SetHighCut(v) => ("DSP high cut set", ok_unit(radio.set_high_cutoff(v).await)),
        SetLowCut(v) => ("DSP low cut set", ok_unit(radio.set_low_cutoff(v).await)),

        // --- Transmit group ---
        Transmit => ("PTT transmit", ok_unit(radio.transmit().await)),
        PttReceive => ("PTT receive", ok_unit(radio.receive().await)),
        SetSpeechProcessor(on) => (
            "Speech processor set",
            ok_unit(radio.set_speech_processor(on).await),
        ),
        SetAntennaThru(on) => (
            "Antenna tuner thru set",
            ok_unit(radio.set_antenna_tuner_thru(on).await),
        ),
        StartAntennaTuning => (
            "Antenna tuning started",
            ok_unit(radio.start_antenna_tuning().await),
        ),

        // --- CW group ---
        SetCwPitch(v) => ("CW pitch set", ok_unit(radio.set_cw_pitch(v).await)),
        SetSemiBreakInDelay(v) => (
            "Semi break-in delay set",
            ok_unit(radio.set_semi_break_in_delay(v).await),
        ),
        SetCwAutoZerobeat(on) => (
            "CW auto zero-beat set",
            ok_unit(radio.set_cw_auto_zerobeat(on).await),
        ),
        SendCw(msg) => ("CW message sent", ok_unit(radio.send_cw(&msg).await)),

        // --- Tones group ---
        SetCtcss(on) => ("CTCSS set", ok_unit(radio.set_ctcss(on).await)),
        SetCtcssToneNumber(n) => (
            "CTCSS tone number set",
            ok_unit(radio.set_ctcss_tone_number(n).await),
        ),
        SetTone(on) => ("Tone set", ok_unit(radio.set_tone(on).await)),
        SetToneNumber(n) => ("Tone number set", ok_unit(radio.set_tone_number(n).await)),

        // --- System group ---
        SetAutoInfo(v) => ("Auto-info set", ok_unit(radio.set_auto_info(v).await)),
        SetPowerOn(on) => ("Power on/off set", ok_unit(radio.set_power_on(on).await)),
        VoiceRecall(v) => ("Voice recall", ok_unit(radio.voice_recall(v).await)),
        ResetPartial => ("Reset partial", ok_unit(radio.reset(false).await)),
        ResetFull => ("Reset full", ok_unit(radio.reset(true).await)),
    }
}
