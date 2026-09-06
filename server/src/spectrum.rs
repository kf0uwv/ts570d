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

//! Reading the CN4 tap, and publishing what it sees.
//!
//! # Its own thread, on purpose
//!
//! Reading a dongle is blocking I/O and the FFT is real work. Both inside
//! the broker's single-threaded runtime would stall every other client for
//! the duration of each frame — the rigctl bridge WSJT-X is talking to
//! included. So this owns a thread, and hands frames over through the same
//! newest-wins cache the console listener reads.
//!
//! # It follows the dial
//!
//! An IF tap is dial-centred by construction: the SDR is parked on the
//! 73.05 MHz first IF while the radio's local oscillator does the tuning.
//! This reads the dial out of the published state — the same state the
//! console sees — so the axis a console draws and the axis the pipeline
//! computed always agree.
//!
//! # It reconnects
//!
//! A dongle unplugged mid-session, or an emulator restarted, should cost a
//! gap in the waterfall and not a dead console. The CAT side is entirely
//! unaffected either way, which is the point of the tap being a separate
//! device rather than part of the radio's own link.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cat_rigctl::native_bridge::NativeShared;
use cat_signal::{IfTapConfig, SpectrumSource};
use tracing::{info, warn};

/// What a TS-570D's CN4 header actually is.
///
/// Read out of the radio's own declaration rather than restated, so the
/// pipeline and the capability set cannot disagree about the tap.
fn tap_config() -> IfTapConfig {
    match radio::capabilities::TS570D.signal {
        cat_framework::capabilities::SignalSupport::IfTapPoint {
            if_center_hz,
            inverted,
        } => IfTapConfig {
            if_center_hz,
            inverted,
            // Calibrated per station against WWV. Zero until somebody
            // measures theirs -- a wrong non-zero default would be worse
            // than none, because it would look calibrated.
            trim_hz: 0,
        },
        // Unreachable: this radio declares a tap. Kept total rather than
        // panicking, so a change to the declaration degrades to "no
        // spectrum" instead of taking the server down.
        _ => IfTapConfig {
            if_center_hz: 73_050_000,
            inverted: true,
            trim_hz: 0,
        },
    }
}

/// Which IF source the tap thread should be reading.
///
/// A source can be chosen at startup with `--if-out` or later by a console
/// picking one over the native protocol, and both have to reach the same
/// thread. This is that seam: the thread watches it, and notices a change
/// between frames rather than only when its current source dies.
///
/// # Why the opened source is handed over, not the spec
///
/// [`IfSelection::select`] takes a source the caller has already opened.
/// That puts the failure where an operator is waiting for it -- a busy
/// dongle refuses the attach then and there, with the driver's own words
/// -- instead of two seconds later on a background thread with nobody
/// listening. It also means the device is never open twice.
pub struct IfSelection {
    /// A source that has been opened and not yet picked up by the thread.
    pending: Mutex<Option<cat_signal_rtlsdr::IfSource>>,
    /// The last spec chosen, so the thread can reconnect after a drop
    /// without an operator re-picking.
    spec: Mutex<Option<String>>,
    /// Bumped on every selection, so the reader can tell "still the same
    /// source" from "a new one is waiting" without holding a lock per
    /// frame.
    generation: AtomicU64,
}

impl IfSelection {
    /// Start with whatever `--if-out` named, if anything.
    pub fn new(spec: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            pending: Mutex::new(None),
            spec: Mutex::new(spec),
            generation: AtomicU64::new(0),
        })
    }

    /// Hand over an already-opened source, replacing whatever is running.
    pub fn select(&self, spec: String, source: cat_signal_rtlsdr::IfSource) {
        if let Ok(mut slot) = self.pending.lock() {
            *slot = Some(source);
        }
        if let Ok(mut current) = self.spec.lock() {
            *current = Some(spec);
        }
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// Whether anything has ever been selected.
    pub fn is_set(&self) -> bool {
        self.spec.lock().map(|s| s.is_some()).unwrap_or(false)
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    fn take_pending(&self) -> Option<cat_signal_rtlsdr::IfSource> {
        self.pending.lock().ok()?.take()
    }

    fn spec(&self) -> Option<String> {
        self.spec.lock().ok()?.clone()
    }
}

/// Read the selected source forever, publishing frames into `shared`.
///
/// Spawns a thread and returns.
pub fn spawn(shared: Arc<NativeShared>, selection: Arc<IfSelection>) {
    std::thread::spawn(move || loop {
        let generation = selection.generation();
        match run_once(&shared, &selection, generation) {
            // Not a warning: this is what a console choosing a different
            // source looks like from in here, and it is the normal case.
            Ok(Ended::Replaced) => info!("CN4 tap: switching to a new source"),
            Ok(Ended::Idle) => {}
            // A source going away arrives as an error from `next_frame`,
            // so there is no separate "closed cleanly" outcome to report.
            Err(e) => warn!("CN4 tap: {e}"),
        }
        // Slow enough not to spin on a tap that is not there, quick
        // enough that restarting an emulator does not need patience. A
        // replacement is already open and waiting, so it does not pay it.
        if selection.generation() == generation {
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

/// Why a reading run stopped.
enum Ended {
    /// A console selected a different source.
    Replaced,
    /// Nothing is selected at all. Normal for a server started without
    /// `--if-out` and never given a source.
    Idle,
}

fn run_once(
    shared: &NativeShared,
    selection: &IfSelection,
    generation: u64,
) -> Result<Ended, String> {
    // A source a console already opened for us wins: it is known good,
    // and reopening it here would be a second claim on the same dongle.
    let mut source = match selection.take_pending() {
        Some(source) => {
            info!("CN4 tap: using the source the console attached");
            source
        }
        None => match selection.spec() {
            Some(spec) => open_spec(&spec)?,
            None => {
                std::thread::sleep(Duration::from_millis(250));
                return Ok(Ended::Idle);
            }
        },
    };
    read_frames(shared, selection, generation, &mut source)
}

/// Open the source named by `spec`.
///
/// Public because the attach path opens through it too: an operator's
/// choice and a `--if-out` flag must produce the same thing, and two call
/// sites building it separately is how they drift apart.
pub fn open_spec(spec: &str) -> Result<cat_signal_rtlsdr::IfSource, String> {
    // One call, and this crate's whole contribution is `tap_config()`.
    // The rate, the bin count, and whether `addr` names a socket or a
    // local dongle are the library's to decide -- it is the only place
    // that knows which rates an RTL2832U can actually produce. This used
    // to be assembled here against a hard-coded 96 kHz, which is not one
    // of them; the emulator answered anyway, because rtl_tcp is a socket
    // and a socket will serve any number you ask it for.
    let source = cat_signal_rtlsdr::open(
        spec,
        tap_config(),
        cat_signal_rtlsdr::IfSourceConfig::default(),
    )
    .map_err(|e| e.to_string())?;
    info!("CN4 tap connected: {spec}");
    Ok(source)
}

fn read_frames(
    shared: &NativeShared,
    selection: &IfSelection,
    generation: u64,
    source: &mut cat_signal_rtlsdr::IfSource,
) -> Result<Ended, String> {
    let mut last_dial = None;
    loop {
        // Checked per frame, so an operator who picks a new source sees
        // the waterfall change now rather than whenever this one happens
        // to fail. An atomic load, not a lock: this runs at frame rate.
        if selection.generation() != generation {
            return Ok(Ended::Replaced);
        }
        // Follow the dial. Retuning the pipeline is arithmetic, not a
        // command to the dongle: the SDR never moves.
        if let Some(dial) = shared.dial_hz() {
            if last_dial != Some(dial) {
                source.retune(dial);
                last_dial = Some(dial);
            }
        }
        match futures::executor::block_on(source.next_frame()) {
            Ok(frame) => shared.publish_spectrum(frame),
            Err(e) => return Err(e),
        }
    }
}
