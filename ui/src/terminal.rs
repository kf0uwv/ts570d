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
use radio::{Frequency, MemoryChannelEntry, Mode, PttLine, PttLineKind, Radio, RadioError};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    control::{handle_key, ControlState, ExecuteAction, KeyResult, PttLineAction},
    diag::{DiagResult, DiagState, DIAG_ROUNDS},
    layout::{draw_control_panel, draw_diag_panel, draw_disconnected},
    RadioDisplay, UiError, UiResult,
};

#[cfg(target_os = "windows")]
use crate::win_sched;

// ---------------------------------------------------------------------------
// Single-threaded channel primitive (Rc<RefCell<VecDeque<T>>>)
//
// Sound on Windows too: `run`'s Windows variant (below) keeps both
// `radio_task` and `ui_task` on the one OS thread that calls it,
// cooperatively polled by `win_sched::block_on_two` instead of
// `monoio::spawn` — never `Send` across threads, exactly like the Linux
// `monoio::spawn` model this replaces. See
// `docs/adr/0006-windows-concurrency-model.md`.
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
    /// Begin a diagnostic run. `callsign` (if supplied by the operator) gates
    /// the CW keying test step — see `run_diagnostics_task`.
    StartDiagnostics {
        callsign: Option<String>,
    },
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
        /// True if this step was intentionally not attempted (see
        /// `DiagResult::skipped`).
        skipped: bool,
    },
    DiagDone,
}

// ---------------------------------------------------------------------------
// PTT line
// ---------------------------------------------------------------------------

/// The `radio::PttLine` a console gets when its port has no handshake lines
/// (`--server`, a TCP client). Every method keeps the trait's default, which
/// reports the capability absent, so the `[P]` item is never offered.
// `radio::NoPttLine` -- one definition, so a console and a wiring
// layer cannot disagree about what "no PTT line" behaves like.
use radio::NoPttLine;

/// Owns the PTT-line handle and puts the lines back on the way out.
///
/// "Back" is DTR deasserted and RTS asserted — `PttLineKind::idle_level`, and
/// not "both low": the radio's RTS input is receive-enable and it stops
/// answering CAT while that line is down.
///
/// The `Drop` impl is the last resort, for the panic-unwind path. It cannot
/// retry: this console is single-threaded, so if the radio task is holding
/// the session at that instant, blocking here would guarantee it never lets
/// go. Every path an operator can take — Esc, `[Q]` — releases the line
/// through [`restore_ptt_idle`] first, which does retry. See
/// `docs/adr/0010` for what remains uncovered.
struct PttLineGuard {
    ptt: Box<dyn PttLine>,
    /// Whether a line was ever moved. Nothing to restore if not, and asking
    /// a port with no lines to restore them just produces errors to discard.
    touched: bool,
}

impl PttLineGuard {
    fn new(ptt: Box<dyn PttLine>) -> Self {
        Self {
            ptt,
            touched: false,
        }
    }
}

impl Drop for PttLineGuard {
    fn drop(&mut self) {
        if !self.touched {
            return;
        }
        for line in [PttLineKind::Dtr, PttLineKind::Rts] {
            let _ = self.ptt.set_ptt_line(line, line.idle_level());
        }
    }
}

/// How long to keep asking for a line while the radio task holds the session.
///
/// One CAT command at 9600 Bd is tens of milliseconds; a whole poll cycle is
/// under a second. A second of patience therefore covers the worst case
/// several times over, and still ends rather than hanging the console.
const PTT_BUSY_RETRIES: usize = 200;
const PTT_BUSY_WAIT: Duration = Duration::from_millis(5);

/// Move one line, waiting out any CAT command that is holding the session.
///
/// `RadioError::Busy` is not a failure — it means "the radio task has the
/// session right now". Reporting it to the operator as an error would train
/// them to press the key twice, which on a keying control is the wrong habit.
async fn apply_ptt_line(
    guard: &mut PttLineGuard,
    line: PttLineKind,
    asserted: bool,
) -> Result<(), RadioError> {
    guard.touched = true;
    for _ in 0..PTT_BUSY_RETRIES {
        match guard.ptt.set_ptt_line(line, asserted) {
            Err(RadioError::Busy) => yield_sleep(PTT_BUSY_WAIT).await,
            other => return other,
        }
    }
    Err(RadioError::Busy)
}

/// Put both lines back where an idle console leaves them.
async fn restore_ptt_idle(guard: &mut PttLineGuard) {
    if !guard.touched {
        return;
    }
    for line in [PttLineKind::Dtr, PttLineKind::Rts] {
        let _ = apply_ptt_line(guard, line, line.idle_level()).await;
    }
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

/// Turn a command-line action into something the radio task can do.
///
/// Only the commands that have a counterpart in this console's own action
/// vocabulary are executed; the rest report why rather than failing
/// silently. `cat_native::Command` is the *native protocol's* vocabulary and
/// this console speaks CAT directly, so the two overlap without being the
/// same set — mapping the remainder is a larger job than the command line
/// itself and is deliberately not faked here.
fn apply_console_action(
    action: cat_ui::command::Action,
    view: &mut crate::console::ConsoleView,
    cmd_tx: &Chan<RadioCmd>,
) {
    use cat_native::Command;
    use cat_ui::command::Action;

    match action {
        Action::Quit => ch_send(cmd_tx, RadioCmd::Quit),
        // Handled inside `console::handle_key`; it never reaches here.
        Action::SelectTab(_) => {}
        Action::Radio(command) => match command {
            // The pending grammar: the confirmed value stays on screen and
            // the requested one follows it until a poll confirms.
            Command::SetFrequency { hz, .. } | Command::Retune { hz } => {
                view.pending_vfo_hz = Some(hz);
                ch_send(cmd_tx, RadioCmd::Execute(ExecuteAction::SetVfoA(hz)));
            }
            Command::SetMode { mode } => match native_mode_to_ts570d(mode) {
                Some(m) => ch_send(cmd_tx, RadioCmd::Execute(ExecuteAction::SetMode(m))),
                None => view.message = Some(format!("this radio has no {mode:?} mode")),
            },
            Command::SetSplit { enabled } => {
                // Split on this radio is which VFO transmits, not a flag.
                let tx_vfo = u8::from(enabled);
                ch_send(cmd_tx, RadioCmd::Execute(ExecuteAction::SetTxVfo(tx_vfo)));
            }
            Command::SetMemoryChannel { channel } => match u8::try_from(channel) {
                Ok(n) => ch_send(
                    cmd_tx,
                    RadioCmd::Execute(ExecuteAction::SelectMemoryChannel(n)),
                ),
                Err(_) => view.message = Some(format!("no memory channel {channel}")),
            },
            Command::SetIfShift { hz } => {
                // This radio takes a direction and a magnitude, not a
                // signed offset: `IS` carries ' ', '+' or '-' and then four
                // digits.
                let dir = match hz.signum() {
                    1 => '+',
                    -1 => '-',
                    _ => ' ',
                };
                match u16::try_from(hz.unsigned_abs()) {
                    Ok(magnitude) => ch_send(
                        cmd_tx,
                        RadioCmd::Execute(ExecuteAction::SetIfShift(dir, magnitude)),
                    ),
                    Err(_) => {
                        view.message = Some(format!("{hz} Hz is beyond this radio's IF shift"))
                    }
                }
            }
            // The remaining variants are reads and a filter width. The
            // parser cannot produce any of them today, so this arm is
            // unreachable in practice and still has to say something true
            // if that changes.
            //
            // The two reads would add nothing an operator could see: this
            // console already polls the dial, the mode and the S-meter
            // every cycle and draws them, so a one-shot read has nowhere
            // to put its answer. Filter width has no counterpart in this
            // console's own action set -- the TS-570D sets it through the
            // `SH`/`SL` cut pair, which the `[M]` menu reaches and a single
            // width value does not describe.
            Command::ReadMeter { .. } | Command::ReadState => {
                view.message =
                    Some("this console already polls the radio; there is nothing to read".into());
            }
            other => {
                view.message = Some(format!("{other:?} has no counterpart on this radio"));
            }
        },
    }
}

/// `cat_native::ModeId` to this radio's own mode byte.
///
/// `Mode` is 1-indexed per the CAT protocol, so the byte is the enum's
/// discriminant rather than a position in a list.
fn native_mode_to_ts570d(mode: cat_native::ModeId) -> Option<u8> {
    use cat_native::ModeId;
    Some(
        match mode {
            ModeId::Lsb => radio::Mode::Lsb,
            ModeId::Usb => radio::Mode::Usb,
            ModeId::CwUpper => radio::Mode::Cw,
            ModeId::Fm => radio::Mode::Fm,
            ModeId::Am => radio::Mode::Am,
            ModeId::RttyLsb => radio::Mode::Fsk,
            ModeId::CwLower => radio::Mode::CwReverse,
            ModeId::RttyUsb => radio::Mode::FskReverse,
            _ => return None,
        }
        .as_u8(),
    )
}

/// Draw a single frame using the given radio state and control state.
pub(crate) fn draw_frame(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &RadioDisplay,
    control: &ControlState,
    view: &crate::console::ConsoleView,
    caps: &cat_native::CapabilitiesWire,
) -> UiResult<()> {
    terminal.draw(|f| {
        // The accepted design is the resting state. Everything the design
        // did not place -- this radio's own feature menus, and the
        // TX-gated diagnostics and PTT-line screens -- overlays the tab
        // body, so the console is what an operator looks at and a menu is
        // somewhere they are briefly. See `console::draw`.
        let body = crate::console::draw(f, f.size(), state, view, caps);

        if state.initializing || !state.connected {
            draw_disconnected(f, body, &state.poll_errors, state.initializing);
        } else if let ControlState::Diagnostic(diag) = control {
            draw_diag_panel(f, body, diag);
        } else if !matches!(control, ControlState::Menu) {
            draw_control_panel(f, body, control, state.ptt_line_available);
        }
    })?;
    Ok(())
}

/// Run the radio UI with a separate radio polling task.
///
/// The radio polling/command task and the UI rendering/key-event task run
/// concurrently via `monoio::spawn`, so key events (including Q) are always
/// responsive regardless of radio latency.
#[cfg(target_os = "linux")]
pub async fn run<R: Radio + 'static>(radio: R) -> UiResult<()> {
    run_with_ptt_line(radio, Box::new(NoPttLine)).await
}

/// [`run`], for a console whose port has a PTT handshake line.
///
/// `ptt` is a `radio::PttLine` handle over the *same* port the radio is
/// talking through — `radio::Ts570d::ptt_line_handle` makes one. It is passed
/// separately rather than taken from `radio` because the radio task owns the
/// radio, and the keystroke that unkeys the transmitter must not have to
/// queue behind a poll cycle to be obeyed. See `docs/adr/0010`.
///
/// The handle can always be handed over: if the port turns out to have no
/// lines, `ptt_line_available` says so and the `[P]` item is simply not
/// offered. `run` is the same call with a handle that has none.
#[cfg(target_os = "linux")]
pub async fn run_with_ptt_line<R: Radio + 'static>(
    radio: R,
    ptt: Box<dyn PttLine>,
) -> UiResult<()> {
    run_console(radio, ptt, crate::feeds::ConsoleSources::default()).await
}

/// [`run_with_ptt_line`], for a console that also has signal sources.
///
/// The radio's other two interfaces — the CN4 tap and the ACC2 audio pair —
/// are separate connections from the CAT link, and a console may have
/// either, both or neither. They arrive here already opened, because
/// naming a concrete source type is the wiring layer's job.
#[cfg(target_os = "linux")]
pub async fn run_console<R: Radio + 'static>(
    radio: R,
    ptt: Box<dyn PttLine>,
    sources: crate::feeds::ConsoleSources,
) -> UiResult<()> {
    let terminal = init_terminal()?;

    let cmd_ch: Chan<RadioCmd> = make_chan();
    let update_ch: Chan<RadioUpdate> = make_chan();

    let radio_cmd_rx = Rc::clone(&cmd_ch);
    let radio_update_tx = Rc::clone(&update_ch);

    // Asked once, here, before anything else can be holding the session --
    // see `RadioDisplay::ptt_line_available`.
    let ptt_available = ptt.ptt_line_available();

    // Spawn radio task (runs concurrently on the same thread).
    let radio_handle = monoio::spawn(async move {
        radio_task(radio, radio_cmd_rx, radio_update_tx).await;
    });

    // Run UI task in this context.
    let result = ui_task(terminal, cmd_ch, update_ch, ptt, ptt_available, sources).await;

    // Drop the radio task handle — this cancels the task without blocking.
    // Awaiting it would block for up to 40s while the radio task is stuck in poll_radio_state.
    drop(radio_handle);

    cleanup_terminal()?;
    result
}

/// Windows variant of [`run`]. `monoio::spawn` does not exist on Windows (no
/// `monoio` at all — see `docs/adr/0006-windows-concurrency-model.md`), and
/// a real `std::thread::spawn` worker is not viable either: `radio: R` (a
/// `radio::Ts570d<S>`) is unconditionally `!Send`. Instead both tasks stay
/// on this one OS thread, driven by [`win_sched::block_on_two`]'s hand-rolled
/// round-robin poller — the same "UI task always polled first" fairness
/// property `monoio::spawn` provides, without needing `Send` at all. This is
/// a synchronous function (no outer async runtime exists on Windows to
/// `.await` it); the platform-gated `main()` in `src/main.rs` calls it
/// directly.
#[cfg(target_os = "windows")]
pub fn run<R: Radio + 'static>(radio: R) -> UiResult<()> {
    run_with_ptt_line(radio, Box::new(NoPttLine))
}

/// Windows variant of [`run_with_ptt_line`]. Same contract; see [`run`] for
/// why this one is synchronous.
///
/// The capability itself is cross-platform: `cat-transport-serial`'s Win32
/// backend drives DTR/RTS through `EscapeCommFunction` and reads the
/// handshake through `GetCommModemStatus`, so nothing below this is
/// Linux-specific (radio-cat-rs ADR 0004).
#[cfg(target_os = "windows")]
pub fn run_with_ptt_line<R: Radio + 'static>(radio: R, ptt: Box<dyn PttLine>) -> UiResult<()> {
    run_console(radio, ptt, crate::feeds::ConsoleSources::default())
}

/// Windows variant of [`run_console`]; see [`run`] for why it is synchronous.
#[cfg(target_os = "windows")]
pub fn run_console<R: Radio + 'static>(
    radio: R,
    ptt: Box<dyn PttLine>,
    sources: crate::feeds::ConsoleSources,
) -> UiResult<()> {
    let terminal = init_terminal()?;

    let cmd_ch: Chan<RadioCmd> = make_chan();
    let update_ch: Chan<RadioUpdate> = make_chan();

    let radio_cmd_rx = Rc::clone(&cmd_ch);
    let radio_update_tx = Rc::clone(&update_ch);

    let ptt_available = ptt.ptt_line_available();

    let radio_fut: std::pin::Pin<Box<dyn std::future::Future<Output = ()>>> =
        Box::pin(radio_task(radio, radio_cmd_rx, radio_update_tx));
    let ui_fut = Box::pin(ui_task(
        terminal,
        cmd_ch,
        update_ch,
        ptt,
        ptt_available,
        sources,
    ));

    // Mirrors Linux's `drop(radio_handle)`: block_on_two returns as soon as
    // `ui_fut` resolves, dropping (canceling) `radio_fut` without waiting
    // for it.
    let result = win_sched::block_on_two(ui_fut, radio_fut);

    cleanup_terminal()?;
    result
}

/// `monoio::time::sleep` on Linux; a busy-poll-friendly, waker-free sleep on
/// Windows whose correctness relies on `win_sched::block_on_two`'s own
/// `park_timeout` bound re-polling it often enough (see
/// `docs/adr/0006-windows-concurrency-model.md`). Identical call sites and
/// behavior on Linux to before this indirection was introduced.
#[cfg(target_os = "linux")]
async fn yield_sleep(duration: Duration) {
    monoio::time::sleep(duration).await;
}

#[cfg(target_os = "windows")]
async fn yield_sleep(duration: Duration) {
    win_sched::WinSleep::new(duration).await;
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
                // A field this link cannot reach is not a fault, and
                // reporting each one as an error would put a permanent
                // banner over a console working exactly as it can.
                //
                // The field stays at its struct default -- which is NOT
                // what an unread field should look like, and used to be
                // drawn as though it were a reading. `levels_known` is
                // what keeps that honest now: unless a read answers, the
                // rail draws dashes rather than the default.
                Err(radio::RadioError::NotImplemented) => {}
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
        |info: radio::InformationResponse| {
            state.vfo_a_hz = info.frequency.hz();
            state.mode = info.mode.name().to_string();
            // Beside the label, not instead of it: the label is what an
            // operator reads and this is what the passband derives from.
            // The console used to parse the label back into a mode, which
            // works for exactly one radio's spelling.
            state.mode_id = Some(radio::capabilities::from_mode(info.mode));
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
    poll!("VFO-B", radio.get_vfo_b(), |freq: radio::Frequency| {
        state.vfo_b_hz = freq.hz();
    });
    poll!("SM", radio.get_smeter(), |s: u16| {
        state.smeter = s;
    });
    // Cleared, then set by the first level read that actually answers.
    //
    // Over a serial link every one of these is a real CAT command and they
    // all succeed. Over the console protocol they are served from the
    // server's slow-poll block, which is absent until the first slow poll
    // lands -- and `poll!` leaves a failed read at its struct default, so
    // claiming the rail is known before one has answered would draw
    // `AF 200` at a radio reading `AG034`. They share one source, so one
    // answering means all did. See `RadioDisplay::levels_known`.
    state.levels_known = false;
    poll!("AF", radio.get_af_gain(), |v: u8| {
        state.af_gain = v;
        state.levels_known = true;
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
                skipped: false,
            },
        );
        $results.push(DiagResult {
            label: $label,
            round: $round,
            passed,
            detail,
            skipped: false,
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
                skipped: false,
            },
        );
        $results.push(DiagResult {
            label: $label,
            round: $round,
            passed,
            detail,
            skipped: false,
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
                skipped: false,
            },
        );
        $results.push(DiagResult {
            label: $label,
            round: $round,
            passed,
            detail,
            skipped: false,
        });
    }};
}

// ---------------------------------------------------------------------------
// Snapshot / restore helpers for run_diagnostics
// ---------------------------------------------------------------------------

/// A snapshot of all readable radio state that has a corresponding setter.
/// Every field is `Option<T>` so that individual getter failures are non-fatal.
struct RadioSnapshot {
    vfo_a: Option<radio::Frequency>,
    vfo_b: Option<radio::Frequency>,
    mode: Option<radio::Mode>,
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
///
/// `callsign`, if supplied, is used to identify the CW keying test
/// (`send_cw("TEST {callsign}")`, step 82). If `None`, that step is recorded
/// as skipped rather than transmitting an unidentified CW test — see the
/// `step_idx == 82` arm below.
async fn run_diagnostics_task<R: Radio>(
    radio: &mut R,
    update_tx: &Chan<RadioUpdate>,
    callsign: Option<String>,
) {
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
        "send_cw(TEST <call>)",    // 82
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
                            skipped: false,
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                        skipped: false,
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
                            skipped: false,
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                        skipped: false,
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
                            skipped: false,
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                        skipped: false,
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
                            skipped: false,
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                        skipped: false,
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
                            skipped: false,
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                        skipped: false,
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
                            skipped: false,
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                        skipped: false,
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
                            skipped: false,
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                        skipped: false,
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
                            skipped: false,
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                        skipped: false,
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

                // send_cw("TEST <callsign>"): requires an operator-supplied
                // callsign for station identification. With no callsign this
                // step is recorded as skipped — not sent, and not counted as
                // either a pass or a failure — rather than transmitting
                // unidentified CW. See docs/adr/0007-diagnostics-tx-safety-gate.md.
                82 => {
                    match callsign.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
                        Some(cs) => {
                            let msg = format!("TEST {}", cs);
                            diag_action!(results, update_tx, label, round, radio.send_cw(&msg));
                        }
                        None => {
                            let detail = "skipped: no callsign supplied — CW keying test requires station ID".to_string();
                            ch_send(
                                update_tx,
                                RadioUpdate::DiagProgress {
                                    label,
                                    round,
                                    passed: true,
                                    detail: detail.clone(),
                                    skipped: true,
                                },
                            );
                            results.push(DiagResult {
                                label,
                                round,
                                passed: true,
                                detail,
                                skipped: true,
                            });
                        }
                    }
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
                            skipped: false,
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                        skipped: false,
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
                            skipped: false,
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                        skipped: false,
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
                            skipped: false,
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                        skipped: false,
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
                            skipped: false,
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                        skipped: false,
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
                            skipped: false,
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                        skipped: false,
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
                            skipped: false,
                        },
                    );
                    results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                        skipped: false,
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
                RadioCmd::StartDiagnostics { callsign } => {
                    run_diagnostics_task(&mut radio, &update_tx, callsign).await;
                }
            }
        }

        // 5. Yield ~200ms before next poll cycle.
        yield_sleep(std::time::Duration::from_millis(200)).await;
    }
}

// ---------------------------------------------------------------------------
// UI task — renders frames and handles key events
// ---------------------------------------------------------------------------

async fn ui_task(
    mut terminal: Terminal<CrosstermBackend<Stdout>>,
    cmd_tx: Chan<RadioCmd>,
    update_rx: Chan<RadioUpdate>,
    ptt: Box<dyn PttLine>,
    ptt_available: bool,
    mut sources: crate::feeds::ConsoleSources,
) -> UiResult<()> {
    let mut ptt = PttLineGuard::new(ptt);
    // initializing=true by default
    let mut state = RadioDisplay {
        ptt_line_available: ptt_available,
        ..RadioDisplay::default()
    };
    let mut control = ControlState::Menu;
    // The capability document this console derives its structure from. The
    // GUI is handed one over the network; a serial console builds one from
    // the radio's own static declaration, and both then call the same
    // `cat_ui::workspace::tabs`.
    // The server's document if there is a server, because it is the one
    // carrying the layout that server authored. Otherwise this radio's own
    // declaration, with the layout this radio's crate asks for -- which is
    // the same answer a local server would have published.
    let caps = sources.capabilities.clone().unwrap_or_else(|| {
        let mut caps = cat_native::CapabilitiesWire::from(&radio::capabilities::TS570D);
        caps.layout = Some(radio::console_layout::layout());
        caps
    });
    let mut view = crate::console::ConsoleView::for_capabilities(&caps);
    // What the machine can see, enumerated once by the wiring layer. Copied
    // rather than re-asked per frame: enumerating sound cards talks to the
    // sound server, and doing that at the redraw rate would be a steady drip
    // of syscalls answering a question that only changes when somebody plugs
    // something in.
    view.devices = std::mem::take(&mut sources.devices);
    view.device_selection.select_default(&view.devices);
    let mut if_shift_dir: char = ' ';
    let mut diag_results: Vec<DiagResult> = Vec::new();

    // Draw initial connecting frame immediately.
    draw_frame(&mut terminal, &state, &control, &view, &caps)?;

    loop {
        // 1. Drain all pending radio updates.
        for update in ch_recv_all(&update_rx) {
            match update {
                RadioUpdate::State(s) => {
                    // The radio task builds each snapshot from
                    // `RadioDisplay::default()` and knows nothing about the
                    // port's handshake lines; this is decided once, at
                    // startup, and restamped here.
                    state = RadioDisplay {
                        ptt_line_available: ptt_available,
                        ..s
                    };
                }
                RadioUpdate::ActionFeedback { ok, msg } => {
                    // The status strip, not a panel over the tab body.
                    //
                    // The old console had no strip, so the only place to put
                    // a result was a screen; option 3 has one, and that is
                    // what it is for. Leaving this as a `ControlState` had
                    // two consequences worth naming, because both looked
                    // like unrelated bugs: the overlay hid whichever tab the
                    // operator was on, and the console's own keys stopped
                    // working, since a state that is not `Menu` hands every
                    // key to the radio's menus -- so after any command the
                    // tab digits and `q` went dead.
                    view.message = Some(if ok { msg } else { format!("! {msg}") });
                }
                RadioUpdate::DiagProgress {
                    label,
                    round,
                    passed,
                    detail,
                    skipped,
                } => {
                    diag_results.push(DiagResult {
                        label,
                        round,
                        passed,
                        detail,
                        skipped,
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

        // 2. Refresh the handshake inputs, but only while they are on
        //    screen. `read_handshake` is one ioctl, but it is pointless
        //    anywhere else, and it can legitimately report `Busy` -- in
        //    which case the last reading stands rather than blinking off.
        if let ControlState::PttLine {
            ref mut cts,
            ref mut dsr,
            ..
        } = control
        {
            if let Ok(handshake) = ptt.ptt.read_handshake() {
                *cts = handshake.cts;
                *dsr = handshake.dsr;
            }
        }

        // 3. Take whatever the signal sources have produced, and tell the
        //    spectrum source where the dial is now. An IF tap is
        //    dial-centred by construction, so a console that did not pass
        //    the frequency on would draw a window that no longer matches
        //    the number printed above it.
        if let Some(feed) = sources.spectrum.as_ref() {
            feed.retune(state.vfo_a_hz);
            view.spectrum = feed.frames();
            if let Some(fault) = feed.fault() {
                view.message = Some(fault);
            }
        }
        if let Some(audio) = sources.audio.as_mut() {
            audio.poll();
            view.audio = audio.state();
            if let Some(frame) = audio.latest() {
                view.af_scope = Some(frame.scope.clone());
                view.af_spectrum = Some(frame.spectrum.clone());
            }
            if let Some(fault) = audio.fault() {
                view.message = Some(fault.to_string());
            }
        }
        // Derived from what the radio published for the mode it is in,
        // rather than from a table keyed on a display label.
        view.passband = state
            .mode_id
            .and_then(|mode| cat_ui::af::passband_for(&caps, mode));

        // 3b. A link that answers questions on its own schedule. The local
        //     case has neither of these and pays a pointer check.
        if let Some(feed) = sources.device_feed.as_ref() {
            if let Ok(devices) = feed.lock() {
                if *devices != view.devices {
                    view.devices = devices.clone();
                    // The cursor was placed against the old list -- which
                    // may have been empty, because the answer had not
                    // arrived yet. Re-seating it on the host's default is
                    // what the local path does at startup.
                    view.device_selection.select_default(&view.devices);
                }
            }
        }
        if let Some(notices) = sources.notices.as_ref() {
            if let Ok(mut queue) = notices.lock() {
                // Newest wins: the message line holds one, and the most
                // recent thing the radio's host said is the one an
                // operator is waiting on.
                if let Some(last) = queue.pop() {
                    view.message = Some(last);
                    queue.clear();
                }
            }
        }

        // 4. Draw frame.
        draw_frame(&mut terminal, &state, &control, &view, &caps)?;

        // 5. Handle key events (non-blocking, 10ms poll window).
        if event::poll(std::time::Duration::from_millis(10)).map_err(UiError::Io)? {
            if let Event::Key(key) = event::read().map_err(UiError::Io)? {
                // The console gets first refusal, but only while the radio's
                // own menus are not in the middle of something: a digit
                // inside a submenu, or a keystroke inside a text prompt,
                // belongs to that submenu. `console::handle_key` passes
                // through everything it does not own, so the feature menus
                // keep every key they ever had.
                //
                // `Feedback` counts as resting, and getting that wrong is
                // what made the digits stop working the first time this was
                // wired: any command leaves a feedback message behind, so
                // gating on `Menu` alone meant the tab keys died the moment
                // the operator did anything.
                if matches!(control, ControlState::Menu | ControlState::Feedback { .. }) {
                    match crate::console::handle_key(key, &mut view, &caps) {
                        crate::console::ConsoleKey::Consumed
                        | crate::console::ConsoleKey::Rejected(_) => continue,
                        crate::console::ConsoleKey::Action(action) => {
                            apply_console_action(action, &mut view, &cmd_tx);
                            continue;
                        }
                        crate::console::ConsoleKey::Attach(device) => {
                            // Opening it belongs to the wiring layer; the
                            // console's part is to show the list and report
                            // what came back.
                            match sources.attach.as_ref() {
                                Some(open) => match open(&device) {
                                    Ok(attached) => {
                                        // A local attach has already
                                        // happened by the time this
                                        // returns. A remote one has only
                                        // been asked for, and the answer
                                        // arrives later as a notice --
                                        // saying "attached" here would
                                        // claim something not yet true,
                                        // and would still say it after
                                        // the host refused.
                                        let remote =
                                            matches!(attached, crate::feeds::Attached::Remote);
                                        sources.accept(attached);
                                        view.message = Some(if remote {
                                            format!("asked the radio's host for {}", device.spec)
                                        } else {
                                            format!("attached {}", device.spec)
                                        });
                                    }
                                    Err(e) => view.message = Some(e),
                                },
                                None => {
                                    view.message =
                                        Some("this console cannot attach devices".to_string())
                                }
                            }
                            continue;
                        }
                        crate::console::ConsoleKey::RefreshDevices => {
                            match sources.refresh_devices.as_ref() {
                                Some(refresh) => {
                                    refresh();
                                    view.message = Some("asked again for devices".to_string());
                                }
                                None => {
                                    view.message =
                                        Some("this console cannot re-enumerate devices".to_string())
                                }
                            }
                            continue;
                        }
                        crate::console::ConsoleKey::Passthrough => {}
                    }
                }
                match handle_key(key, &mut control, &state) {
                    KeyResult::Quit => {
                        // Before anything else: a console that exits with the
                        // transmitter keyed is the failure this whole feature
                        // exists to avoid.
                        restore_ptt_idle(&mut ptt).await;
                        ch_send(&cmd_tx, RadioCmd::Quit);
                        return Ok(());
                    }
                    KeyResult::PttLine(action) => match action {
                        PttLineAction::Set { line, asserted } => {
                            match apply_ptt_line(&mut ptt, line, asserted).await {
                                Ok(()) => {
                                    if let ControlState::PttLine {
                                        asserted: ref mut shown,
                                        ref mut error,
                                        ..
                                    } = control
                                    {
                                        *shown = asserted;
                                        *error = None;
                                    }
                                }
                                Err(e) => {
                                    if let ControlState::PttLine { ref mut error, .. } = control {
                                        *error = Some(e.to_string());
                                    }
                                }
                            }
                        }
                        PttLineAction::Idle => restore_ptt_idle(&mut ptt).await,
                    },
                    KeyResult::Continue => {}
                    KeyResult::StartDiag(callsign) => {
                        diag_results.clear();
                        control = ControlState::Diagnostic(DiagState::Running {
                            current_label: "starting\u{2026}",
                            current_round: 1,
                            results: Vec::new(),
                        });
                        ch_send(&cmd_tx, RadioCmd::StartDiagnostics { callsign });
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

        // 5. Yield briefly so the radio task can run.
        yield_sleep(std::time::Duration::from_millis(5)).await;
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
) -> (&'static str, radio::RadioResult<String>) {
    use radio::{MemoryChannelEntry, RadioResult};
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

#[cfg(all(test, target_os = "linux"))]
mod console_action_tests {
    use super::*;

    /// Every verb the shared parser advertises, run through the console's
    /// own action mapping.
    ///
    /// The bar is not "most of them work": a command line that parses a
    /// verb and then silently does nothing is worse than one that refuses
    /// it, because the operator believes the radio moved. So this walks
    /// `cat_ui::command::VERBS` — the list the hint line is generated from
    /// — and asserts each one reaches the radio task.
    #[test]
    fn every_verb_the_command_line_advertises_actually_reaches_the_radio() {
        let caps = cat_native::CapabilitiesWire::from(&radio::capabilities::TS570D);

        // One concrete line per advertised verb. Kept beside the list so a
        // new verb fails here rather than quietly doing nothing.
        let lines = [
            ("f 14.074", true),
            ("t 14074000", true),
            ("m usb", true),
            ("mem 12", true),
            ("shift 400", true),
            ("split", true),
            ("split off", true),
        ];
        assert_eq!(
            lines.len() - 1,
            cat_ui::command::VERBS.len() - 2,
            "a verb was added or removed; add a line for it here \
             (VERBS also carries the bare-digit tab verb and quit)"
        );

        for (line, should_reach_radio) in lines {
            let action = cat_ui::command::parse(line, &caps)
                .unwrap_or_else(|e| panic!("{line:?} did not parse: {e:?}"));

            let cmd_ch: Chan<RadioCmd> = make_chan();
            let mut view = crate::console::ConsoleView::for_capabilities(&caps);
            apply_console_action(action, &mut view, &cmd_ch);

            let sent = ch_recv_all(&cmd_ch).len();
            if should_reach_radio {
                assert_eq!(
                    sent, 1,
                    "{line:?} parsed but sent nothing to the radio; message was {:?}",
                    view.message
                );
            }
        }
    }

    #[test]
    fn a_retune_shows_as_pending_until_the_radio_confirms() {
        // The design's pending grammar. Without this the readout would jump
        // to a frequency the radio has not acknowledged.
        let caps = cat_native::CapabilitiesWire::from(&radio::capabilities::TS570D);
        let action = cat_ui::command::parse("t 14200000", &caps).expect("parse");
        let cmd_ch: Chan<RadioCmd> = make_chan();
        let mut view = crate::console::ConsoleView::for_capabilities(&caps);

        apply_console_action(action, &mut view, &cmd_ch);
        assert_eq!(view.pending_vfo_hz, Some(14_200_000));
    }

    #[test]
    fn a_mode_this_radio_lacks_is_refused_with_a_reason() {
        // `native_mode_to_ts570d` returns None for the modes a TS-570D does
        // not have. That must reach the operator, not vanish.
        let cmd_ch: Chan<RadioCmd> = make_chan();
        let caps = cat_native::CapabilitiesWire::from(&radio::capabilities::TS570D);
        let mut view = crate::console::ConsoleView::for_capabilities(&caps);

        apply_console_action(
            cat_ui::command::Action::Radio(cat_native::Command::SetMode {
                mode: cat_native::ModeId::DataFm,
            }),
            &mut view,
            &cmd_ch,
        );
        assert!(ch_recv_all(&cmd_ch).is_empty(), "nothing was sent");
        assert!(view.message.is_some(), "and the operator was told why");
    }

    #[test]
    fn an_if_shift_carries_its_direction_separately_from_its_magnitude() {
        // `IS` takes ' ', '+' or '-' and four digits, not a signed number.
        let caps = cat_native::CapabilitiesWire::from(&radio::capabilities::TS570D);
        for (hz, want_dir) in [(400i32, '+'), (-400, '-'), (0, ' ')] {
            let cmd_ch: Chan<RadioCmd> = make_chan();
            let mut view = crate::console::ConsoleView::for_capabilities(&caps);
            apply_console_action(
                cat_ui::command::Action::Radio(cat_native::Command::SetIfShift { hz }),
                &mut view,
                &cmd_ch,
            );
            let sent = ch_recv_all(&cmd_ch);
            assert_eq!(sent.len(), 1, "{hz} Hz sent nothing");
            match &sent[0] {
                RadioCmd::Execute(ExecuteAction::SetIfShift(dir, magnitude)) => {
                    assert_eq!(*dir, want_dir, "{hz} Hz got the wrong direction");
                    assert_eq!(*magnitude, hz.unsigned_abs() as u16);
                }
                _ => panic!("{hz} Hz produced the wrong action"),
            }
        }
    }
}
