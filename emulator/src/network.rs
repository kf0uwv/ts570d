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

//! The emulator's network front-end: a dummy radio to point a console at.
//!
//! # It is the same radio
//!
//! This does not simulate a TS-570D a second time. `apply` translates a
//! native command into the CAT frame a real client would have sent and
//! feeds it to the **same** `CatFramework` the PTY serves, so the network
//! interface and the serial one cannot disagree about what a command does.
//! Tune over the network and the emulator's own TUI moves.
//!
//! That is worth the small translation layer. A second state machine would
//! be a dummy radio that disagrees with the dummy radio, and the first
//! time the two drifted it would look exactly like a console bug.
//!
//! # The spectrum is synthetic, and lives at real frequencies
//!
//! `cat_signal::synthetic::Band` holds emitters at absolute frequencies,
//! and each frame renders whatever window the dial is currently on. So
//! tuning moves the window over a fixed landscape the way a radio does,
//! and click-to-tune has something true to be tested against.
//!
//! The window follows the dial because that is what an IF tap does: the
//! SDR sits on the fixed intermediate frequency while the local oscillator
//! tracks the dial, so the picture is always centred on where the radio is
//! listening. A fixture that kept a fixed window would make a console look
//! correct while hiding the one behaviour that matters.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use cat_native::{Command, MeterKind, MeterSample, ModeId, RadioHost, RadioState};
use cat_signal::synthetic::{Band, Emission, Emitter};
use cat_signal::SpectrumFrame;

use crate::emulator::SharedRadio;

/// How much spectrum the tap shows at once.
///
/// 48 kHz is an ordinary RTL-SDR capture rate, and wide enough that a
/// 2.4 kHz SSB signal is a visible shape rather than a line.
const SPAN_HZ: u32 = 48_000;

/// Bins per frame. More than a display has columns, so the renderer's
/// peak-hold has something to actually pick between.
const BINS: usize = 1024;

/// The emulator, as a native-protocol host.
pub struct EmulatedRadio {
    radio: SharedRadio,
    capabilities: &'static cat_framework::capabilities::RadioCapabilities,
    band: Band,
    started: Instant,
    sequence: AtomicU64,
}

impl EmulatedRadio {
    /// Wrap a shared radio, with a band of synthetic signals around it.
    ///
    /// `seed` makes the band reproducible: the same seed gives the same
    /// signals in the same places every run, which is what lets a test say
    /// "there should be a carrier here" and a person say "it looked like
    /// that yesterday too".
    pub fn new(radio: SharedRadio, seed: u64) -> Self {
        let coverage = radio::capabilities::TS570D.rx_range;
        Self {
            radio,
            capabilities: &radio::capabilities::TS570D,
            band: populate(coverage.min_hz, coverage.max_hz, seed),
            started: Instant::now(),
            sequence: AtomicU64::new(0),
        }
    }

    /// Seconds since start, as the band's time base.
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// The CAT frame a real client would send for `command`.
    ///
    /// `None` for a command with no TS-570D equivalent, which is refused
    /// rather than silently ignored — a console that asked for something
    /// and got an `Ack` for nothing would be worse off than one that got
    /// an error.
    fn cat_frame(command: &Command) -> Option<String> {
        Some(match command {
            // `FA`/`FB` take 11 digits.
            Command::SetFrequency { vfo: 0, hz } | Command::Retune { hz } => {
                format!("FA{hz:011};")
            }
            Command::SetFrequency { vfo: _, hz } => format!("FB{hz:011};"),
            Command::SetMode { mode } => format!("MD{};", ts570d_mode_digit(*mode)?),
            Command::SetSplit { enabled } => {
                // Split is expressed as which VFO transmits.
                format!("FT{};", u8::from(*enabled))
            }
            Command::SetMemoryChannel { channel } => format!("MC{channel:03};"),
            Command::SetIfShift { hz } => {
                let direction = if *hz < 0 { 'D' } else { 'U' };
                format!("IS{direction}{:04};", hz.unsigned_abs().min(9999))
            }
            // Reads are answered from state, not sent to the radio.
            Command::ReadMeter { .. } | Command::ReadState => return None,
            // The TS-570D exposes no CAT-selectable filter width, which
            // `capabilities` already says -- so the session refuses this
            // before it ever reaches here.
            Command::SetFilterWidth { .. } => return None,
        })
    }
}

impl RadioHost for EmulatedRadio {
    fn capabilities(&self) -> &'static cat_framework::capabilities::RadioCapabilities {
        self.capabilities
    }

    fn state(&self) -> RadioState {
        let guard = self.radio.lock().expect("radio lock");
        let s = guard.radio().state();
        RadioState {
            vfo_a_hz: s.vfo_a_hz,
            vfo_b_hz: s.vfo_b_hz,
            mode: mode_from_digit(s.mode),
            split: s.split,
            transmitting: s.tx,
            memory_channel: Some(u16::from(s.mem_channel)),
            // The radio stores IF shift as a direction character and a
            // magnitude; the protocol wants one signed number.
            if_shift_hz: Some(if s.is_direction == 'D' {
                -i32::from(s.is_freq)
            } else {
                i32::from(s.is_freq)
            }),
            // No CAT-selectable width on this radio, which `capabilities`
            // already declares -- reporting one here would contradict it.
            filter_width_hz: None,
            meters: vec![MeterSample {
                kind: MeterKind::S,
                raw: s.smeter,
            }],
        }
    }

    fn apply(&self, command: &Command) -> Result<(), String> {
        let Some(frame) = Self::cat_frame(command) else {
            return Err("the TS-570D has no CAT command for that".to_string());
        };
        let mut guard = self.radio.lock().expect("radio lock");
        let mut response = Vec::new();
        guard
            .process_frame(&frame, &mut response)
            .map(|_| ())
            .map_err(|e| format!("the radio rejected {frame}: {e:?}"))
    }

    fn spectrum(&self) -> Option<SpectrumFrame> {
        // Centred on the dial, because that is what an IF tap does.
        let center_hz = self.radio.lock().ok()?.radio().state().vfo_a_hz;
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        Some(
            self.band
                .frame(center_hz, SPAN_HZ, BINS, self.now(), sequence),
        )
    }
}

/// The HF amateur bands, as this radio sees them.
///
/// Signals are not spread evenly over 60 MHz. They are crowded into the
/// bands people transmit in, and everywhere else is close to empty — which
/// is a fact worth reproducing, because a console that only ever sees a
/// busy spectrum is never tested against a quiet one.
///
/// `(low, high, emitters)`. The counts are chosen so a 48 kHz window
/// inside a band usually holds a handful of signals.
///
/// The first attempt at these was eight times too dense and the waterfall
/// came out a solid wall of colour — one emitter every 1.2 kHz across
/// 20 m, so a 48 kHz view held forty of them and nothing had any space
/// around it. Only looking at it showed that; every test still passed,
/// because "there is a signal in the window" was exactly what they
/// asserted. The rule of thumb is roughly one signal per 6-10 kHz of
/// window, which leaves each one its own shape.
const HF_BANDS: &[(u64, u64, usize)] = &[
    (1_800_000, 2_000_000, 25),
    (3_500_000, 4_000_000, 62),
    (5_330_500, 5_405_000, 9),
    (7_000_000, 7_300_000, 37),
    (10_100_000, 10_150_000, 6),
    (14_000_000, 14_350_000, 43),
    (18_068_000, 18_168_000, 12),
    (21_000_000, 21_450_000, 56),
    (24_890_000, 24_990_000, 12),
    (28_000_000, 29_700_000, 212),
    (50_000_000, 54_000_000, 500),
];

/// A band of synthetic signals across this radio's coverage.
fn populate(min_hz: u64, max_hz: u64, seed: u64) -> Band {
    let mut band = Band::empty(-110.0, seed);
    for (low, high, count) in HF_BANDS {
        // Clamped, so a radio with narrower coverage than the band plan
        // does not end up with emitters it can never tune to.
        let low = (*low).max(min_hz);
        let high = (*high).min(max_hz);
        if low < high {
            band.populate_range(low, high, *count, seed ^ low);
        }
    }
    // A handful of things outside the bands: shortwave broadcast carriers,
    // and one wideband noise source of the kind that makes an operator go
    // looking for a switching supply. Sparse on purpose -- the space
    // between bands should feel quiet.
    band.populate_range(min_hz, max_hz, 40, seed ^ 0xBEEF);
    band.push(Emitter::new((min_hz + max_hz) / 3, Emission::Noise, -96.0));
    band
}

/// Serve the native protocol on `addr`, backed by `radio`.
///
/// Spawns a thread and returns; the emulator's own loop keeps running.
pub fn serve(radio: SharedRadio, addr: &str, seed: u64) -> std::io::Result<()> {
    let listener = std::net::TcpListener::bind(addr)?;
    let bound = listener.local_addr()?;
    println!("NATIVE_LISTEN={bound}");
    let host = Arc::new(EmulatedRadio::new(radio, seed));
    std::thread::spawn(move || {
        if let Err(e) = cat_native::serve(listener, host) {
            eprintln!("native listener stopped: {e}");
        }
    });
    Ok(())
}

/// The TS-570D's `MD` digit for a mode, or `None` if it has no such mode.
fn ts570d_mode_digit(mode: ModeId) -> Option<u8> {
    Some(match mode {
        ModeId::Lsb => 1,
        ModeId::Usb => 2,
        ModeId::CwUpper => 3,
        ModeId::Fm => 4,
        ModeId::Am => 5,
        ModeId::RttyLsb => 6,
        ModeId::CwLower => 7,
        ModeId::RttyUsb => 9,
        _ => return None,
    })
}

/// The mode a `MD` digit means.
///
/// An unrecognised digit reports USB rather than failing: this is a
/// readout, and a console that lost the whole state because one field was
/// odd would be worse than one showing a slightly wrong mode.
fn mode_from_digit(digit: u8) -> ModeId {
    match digit {
        1 => ModeId::Lsb,
        3 => ModeId::CwUpper,
        4 => ModeId::Fm,
        5 => ModeId::Am,
        6 => ModeId::RttyLsb,
        7 => ModeId::CwLower,
        9 => ModeId::RttyUsb,
        _ => ModeId::Usb,
    }
}
