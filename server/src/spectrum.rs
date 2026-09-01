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

use std::sync::Arc;
use std::time::Duration;

use cat_rigctl::native_bridge::NativeShared;
use cat_signal::{IfTapConfig, SpectrumSource};
use cat_signal_rtlsdr::{RtlSdrSource, RtlTcpSource};
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

/// 96 kHz across the display. Wide enough that a 2.4 kHz SSB signal is a
/// shape rather than a line, narrow enough for an RTL-SDR to sustain.
const SAMPLE_RATE_HZ: u32 = 96_000;

/// One frame per FFT. 2048 bins over 96 kHz is about 47 Hz per bin, which
/// resolves a CW signal comfortably.
const FFT: usize = 2048;

/// Read `addr` forever, publishing frames into `shared`.
///
/// Spawns a thread and returns.
pub fn spawn(shared: Arc<NativeShared>, addr: String) {
    std::thread::spawn(move || loop {
        match run_once(&shared, &addr) {
            Ok(()) => warn!("CN4 tap at {addr} closed the connection"),
            Err(e) => warn!("CN4 tap at {addr}: {e}"),
        }
        // Slow enough not to spin on a tap that is not there, quick
        // enough that restarting an emulator does not need patience.
        std::thread::sleep(Duration::from_secs(2));
    });
}

fn run_once(shared: &NativeShared, addr: &str) -> Result<(), String> {
    let iq = RtlTcpSource::connect(addr).map_err(|e| e.to_string())?;
    info!("CN4 tap connected: {addr}");
    let mut source = RtlSdrSource::new(iq, SAMPLE_RATE_HZ, FFT, tap_config());

    let mut last_dial = None;
    loop {
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
            Err(e) => return Err(format!("{e}")),
        }
    }
}
