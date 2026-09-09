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

//! Swapping the IF source under a running server.
//!
//! A console on another machine picks a source and expects the waterfall
//! to change. The failure worth testing for is the quiet one: the attach
//! is accepted, the picker highlights the new device, and the thread keeps
//! reading the old one because it is blocked in `next_frame` and will not
//! look at the selection again until its current source dies.
//!
//! So these use two *distinguishable* sources -- carriers at different
//! offsets -- rather than two of the same thing. A test that swapped one
//! dongle for an identical one would pass whether or not the swap
//! happened.

use std::io::Write;
use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cat_rigctl::native_bridge::NativeShared;
use cat_signal::synthetic::{Band, Emission, Emitter};
use server::spectrum::{open_spec, IfSelection};

/// The dial the emulated radio sits on, and the rate `IfSourceConfig`
/// defaults to. The band has to be generated at the rate the pipeline
/// will assume, or the axis and the data disagree.
const DIAL_HZ: u64 = 14_074_000;
const RATE_HZ: u32 = 240_000;
const FFT: usize = 2048;

/// Serve a band as an rtl_tcp dongle would, forever.
fn serve_band(offset_hz: i64) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let band = Band::empty(-110.0, 1).with(Emitter::new(
        (DIAL_HZ as i64 + offset_hz) as u64,
        Emission::Cw,
        -40.0,
    ));
    std::thread::spawn(move || {
        while let Ok((mut stream, _)) = listener.accept() {
            let band = band.clone();
            std::thread::spawn(move || {
                let mut greeting = [0u8; 12];
                greeting[..4].copy_from_slice(b"RTL0");
                if stream.write_all(&greeting).is_err() {
                    return;
                }
                let mut t = 0.0f64;
                loop {
                    // The tap is inverted on a TS-570D, and the server
                    // corrects for it; generating inverted keeps the
                    // carrier where the arithmetic says it should land.
                    let bytes = band.iq_bytes(DIAL_HZ, RATE_HZ, FFT, t, true);
                    if stream.write_all(&bytes).is_err() {
                        return;
                    }
                    t += FFT as f64 / f64::from(RATE_HZ);
                }
            });
        }
    });
    format!("127.0.0.1:{port}")
}

fn peak_bin(frame: &cat_signal::SpectrumFrame) -> usize {
    frame
        .bins
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(i, _)| i)
        .unwrap()
}

/// Wait for a published frame whose peak satisfies `want`.
fn wait_for_peak(shared: &NativeShared, want: impl Fn(usize) -> bool) -> Option<usize> {
    use cat_native::RadioHost;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Some(frame) = shared.spectrum() {
            let bin = peak_bin(&frame);
            if want(bin) {
                return Some(bin);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

/// A non-zero, negative station calibration, so a swap that silently
/// dropped or re-signed the trim shows up rather than reading the same as
/// the uncalibrated default.
const TRIM_HZ: i32 = -115;

#[test]
fn a_swapped_in_source_is_opened_with_the_stations_calibration() {
    // Three different callers open a source -- the `--if-out` flag, a
    // console's attach, and the reconnect after a dongle drops -- and a
    // waterfall that was calibrated until the dongle was replugged would
    // be a miserable bug to chase. The selection is where all three can
    // read the same number.
    let selection = IfSelection::new(Some(serve_band(0)), TRIM_HZ);
    assert_eq!(selection.trim_hz(), TRIM_HZ);

    let source =
        open_spec(&serve_band(0), selection.trim_hz()).expect("the virtual dongle should open");
    selection.select("swapped".to_string(), source);

    // Surviving a swap is the property under test: `select` replaces the
    // source, never the station's calibration.
    assert_eq!(selection.trim_hz(), TRIM_HZ);
}

#[test]
fn a_console_picking_a_new_source_changes_what_the_waterfall_shows() {
    let low = serve_band(-60_000);
    let high = serve_band(60_000);

    let shared = NativeShared::new(&radio::capabilities::TS570D);
    let selection = IfSelection::new(Some(low.clone()), TRIM_HZ);
    server::spectrum::spawn(Arc::clone(&shared), Arc::clone(&selection));

    // The dial has to be published or the pipeline has nothing to centre
    // on, and every frame would report the same window.
    let bins = FFT;
    let first = wait_for_peak(&shared, |bin| bin < bins / 2)
        .expect("the first source never produced a frame with its carrier below centre");

    // What a console's attach does: open, then hand over.
    let source =
        open_spec(&high, selection.trim_hz()).expect("the second virtual dongle should open");
    selection.select(high.clone(), source);

    let second = wait_for_peak(&shared, |bin| bin > bins / 2).expect(
        "the waterfall never showed the second source -- the thread kept reading the first",
    );

    assert_ne!(first, second);
}

#[test]
fn a_source_that_will_not_open_is_refused_before_anything_is_swapped() {
    // The reason the attach opens the source rather than storing a spec:
    // a bad choice fails while the operator is still looking at the
    // picker, and the running source is left alone.
    let good = serve_band(-60_000);
    let shared = NativeShared::new(&radio::capabilities::TS570D);
    let selection = IfSelection::new(Some(good), TRIM_HZ);
    server::spectrum::spawn(Arc::clone(&shared), Arc::clone(&selection));

    assert!(wait_for_peak(&shared, |_| true).is_some());

    let err = match open_spec("127.0.0.1:1", selection.trim_hz()) {
        Err(e) => e,
        Ok(_) => panic!("nothing listens on port 1, so opening it must fail"),
    };
    assert!(!err.is_empty(), "a refusal must say something");

    // Still reading the source it had.
    assert!(
        wait_for_peak(&shared, |_| true).is_some(),
        "a failed attach took down the running source"
    );
}
