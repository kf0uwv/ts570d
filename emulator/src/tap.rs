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

//! A spoofed CN4 IF tap, presenting itself as an RTL-SDR.
//!
//! # This is hardware, not a service
//!
//! The emulator's job is to be a radio: a CAT port, an ACC2 connector, and
//! a CN4 header with a dongle on it. It is emphatically **not** a server —
//! the control program is the thing that owns a radio and serves consoles,
//! and an earlier version of this crate had the emulator serving the
//! console protocol directly, which put the radio and the server in the
//! same box and made the control program unnecessary to test anything.
//!
//! So this speaks `rtl_tcp`, the protocol librtlsdr ships for putting a
//! dongle on the network. The control program connects to it with exactly
//! the client it would use for a real dongle, and every correction runs for
//! real: the FFT, the windowing, and the un-mirroring that a TS-570D needs
//! because its LO1 is high-side.
//!
//! # It is mirrored on purpose
//!
//! The IQ this serves is **mirrored**, because that is what comes off CN4.
//! A tap that served un-mirrored IQ would make the control program's
//! correction cancel a distortion that was never applied, and everything
//! would look right while being wrong the moment real hardware appeared.
//!
//! # It follows the dial
//!
//! An IF tap is dial-centred by construction: the SDR is parked on the
//! 73.05 MHz first IF and the radio's local oscillator does the tuning. So
//! the window this serves is centred on whatever the emulated radio's VFO A
//! currently is, and retuning over CAT moves it.

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::time::Instant;

use cat_signal::synthetic::{Band, Emission, Emitter};

use crate::emulator::SharedRadio;

/// What the dongle reports at 96 kHz.
pub const SAMPLE_RATE_HZ: u32 = 96_000;

/// Samples per write. About 21 ms at 96 kHz — small enough that retuning
/// shows up promptly, large enough not to syscall per sample.
const BLOCK: usize = 2048;

/// The HF amateur bands, as this radio sees them.
///
/// Signals are not spread evenly over 60 MHz; they crowd into the bands
/// people transmit in, and everywhere else is close to empty. Reproducing
/// that matters because a console that only ever sees a busy spectrum is
/// never tested against a quiet band.
///
/// `(low, high, emitters)`, at roughly one signal per 8 kHz of window. An
/// earlier set was eight times denser and rendered as a solid wall of
/// colour with nothing to point at — every test still passed, because
/// "there is a signal in the window" was exactly what they asserted.
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
pub fn populate(min_hz: u64, max_hz: u64, seed: u64) -> Band {
    let mut band = Band::empty(-110.0, seed);
    for (low, high, count) in HF_BANDS {
        // Clamped, so a radio with narrower coverage than the band plan
        // does not get emitters it can never tune to.
        let low = (*low).max(min_hz);
        let high = (*high).min(max_hz);
        if low < high {
            band.populate_range(low, high, *count, seed ^ low);
        }
    }
    // A few things outside the bands -- shortwave carriers -- and one
    // wideband noise source of the kind that sends an operator looking for
    // a switching supply. Sparse on purpose: between the bands should feel
    // quiet.
    band.populate_range(min_hz, max_hz, 40, seed ^ 0xBEEF);
    band.push(Emitter::new((min_hz + max_hz) / 3, Emission::Noise, -96.0));
    band
}

/// Serve the tap. Spawns a thread and returns the bound address.
pub fn serve(radio: SharedRadio, addr: &str, seed: u64) -> std::io::Result<std::net::SocketAddr> {
    let listener = TcpListener::bind(addr)?;
    let bound = listener.local_addr()?;
    let coverage = radio::capabilities::TS570D.rx_range;
    let band = populate(coverage.min_hz, coverage.max_hz, seed);

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let band = band.clone();
            let radio = radio.clone();
            std::thread::spawn(move || {
                let _ = serve_one(stream, radio, band);
            });
        }
    });
    Ok(bound)
}

fn serve_one(mut stream: TcpStream, radio: SharedRadio, band: Band) -> std::io::Result<()> {
    stream.set_nodelay(true)?;

    // The twelve-byte greeting: magic, tuner id, gain count. A client that
    // does not see `RTL0` should refuse us rather than read whatever
    // follows as IQ.
    let mut greeting = [0u8; 12];
    greeting[..4].copy_from_slice(b"RTL0");
    // Tuner 5 is R820T, which is what the common dongles carry.
    greeting[4..8].copy_from_slice(&5u32.to_be_bytes());
    greeting[8..12].copy_from_slice(&29u32.to_be_bytes());
    stream.write_all(&greeting)?;
    stream.flush()?;

    let started = Instant::now();
    let mut sent: u64 = 0;
    loop {
        // Centred on the dial, because that is what an IF tap does.
        let dial_hz = radio.lock().expect("radio lock").radio().state().vfo_a_hz;
        let t = started.elapsed().as_secs_f64();
        // Mirrored: this is what comes off CN4.
        let bytes = band.iq_bytes(dial_hz, SAMPLE_RATE_HZ, BLOCK, t, true);
        stream.write_all(&bytes)?;
        stream.flush()?;

        // Pace to the sample rate. A dongle delivers in real time, and a
        // tap that ran flat out would let a console's waterfall scroll at
        // whatever speed the CPU allowed -- which looks fine until the
        // timebase means something.
        sent += BLOCK as u64;
        let due = std::time::Duration::from_secs_f64(sent as f64 / f64::from(SAMPLE_RATE_HZ));
        if let Some(sleep) = due.checked_sub(started.elapsed()) {
            std::thread::sleep(sleep);
        }
    }
}
