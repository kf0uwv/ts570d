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

//! The spoofed CN4 tap, consumed the way the control program will consume
//! it: as an RTL-SDR.
//!
//! Nothing here knows the emulator's internals. It connects with the same
//! `rtl_tcp` client a real dongle needs and runs the same DSP — which is
//! the whole point of the tap speaking a real protocol rather than a
//! private one.

use std::time::{Duration, Instant};

use cat_signal::{IfTapConfig, SpectrumSource};
use cat_signal_rtlsdr::{RtlSdrSource, RtlTcpSource};
use emulator::emulator::{new_shared_radio, SharedRadio};
use emulator::tap::{self, SAMPLE_RATE_HZ};

const FFT: usize = 2048;

fn serve() -> (String, SharedRadio) {
    let radio = new_shared_radio();
    let bound = tap::serve(radio.clone(), "127.0.0.1:0", 7).expect("serve the tap");
    (bound.to_string(), radio)
}

fn tune(radio: &SharedRadio, hz: u64) {
    let mut guard = radio.lock().unwrap();
    let mut out = Vec::new();
    let _ = guard.process_frame(format!("FA{hz:011};"), &mut out);
}

fn source_at(addr: &str, dial_hz: u64, inverted: bool) -> RtlSdrSource<RtlTcpSource> {
    let iq = RtlTcpSource::connect(addr).expect("connect to the tap");
    let mut source = RtlSdrSource::new(
        iq,
        SAMPLE_RATE_HZ,
        FFT,
        // What a TS-570D's CN4 actually is.
        IfTapConfig {
            if_center_hz: 73_050_000,
            inverted,
            trim_hz: 0,
        },
    );
    source.retune(dial_hz);
    source
}

fn frame(source: &mut RtlSdrSource<RtlTcpSource>) -> cat_signal::SpectrumFrame {
    futures::executor::block_on(source.next_frame()).expect("a frame")
}

#[test]
fn the_tap_introduces_itself_as_a_dongle() {
    // A client that does not see RTL0 must refuse rather than read
    // whatever follows as IQ. This is the half of that contract the
    // emulator owns.
    let (addr, _radio) = serve();
    let iq = RtlTcpSource::connect(addr.as_str()).expect("connect");
    assert_eq!(&iq.dongle().magic, b"RTL0");
}

#[test]
fn iq_from_the_tap_becomes_a_spectrum_through_the_real_pipeline() {
    let (addr, radio) = serve();
    tune(&radio, 14_074_000);
    let mut source = source_at(&addr, 14_074_000, true);
    let f = frame(&mut source);
    assert_eq!(f.bins.len(), FFT);
    assert_eq!(f.span_hz, SAMPLE_RATE_HZ);
    let peak = f.bins.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let floor = f.bins.iter().cloned().fold(f32::INFINITY, f32::min);
    assert!(
        peak > floor + 10.0,
        "the tap produced a flat spectrum: {floor}..{peak}"
    );
}

#[test]
fn the_window_follows_the_dial_because_that_is_what_a_tap_does() {
    // The SDR is parked on the IF and the radio's LO does the tuning, so
    // retuning over CAT moves the window. This is the property the
    // console's click-to-tune depends on.
    let (addr, radio) = serve();
    tune(&radio, 14_074_000);
    let mut source = source_at(&addr, 14_074_000, true);
    let before = frame(&mut source);

    tune(&radio, 21_074_000);
    source.retune(21_074_000);

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut after = frame(&mut source);
    while after.bins == before.bins && Instant::now() < deadline {
        after = frame(&mut source);
    }
    assert_eq!(after.center_hz, 21_074_000);
    assert_ne!(
        after.bins, before.bins,
        "the tap served identical spectrum for two different bands"
    );
}

#[test]
fn the_tap_serves_mirrored_iq_so_the_correction_has_something_to_do() {
    // A tap serving un-mirrored IQ would make the control program's
    // un-mirroring cancel a distortion that was never applied -- correct
    // on the bench and wrong the moment real hardware appeared.
    //
    // Read the stream with the correction and without, and require them to
    // disagree. If they match, the tap is not mirroring.
    let (addr, radio) = serve();
    tune(&radio, 14_074_000);
    let corrected = frame(&mut source_at(&addr, 14_074_000, true));
    let uncorrected = frame(&mut source_at(&addr, 14_074_000, false));
    assert_ne!(
        corrected.bins, uncorrected.bins,
        "correcting the inversion changed nothing, so the tap is not mirroring"
    );
}

#[test]
fn two_clients_can_watch_the_same_tap() {
    // A console and a decoder, or two consoles. A single dongle cannot do
    // this, but a virtual tap has no reason to impose the limitation.
    let (addr, radio) = serve();
    tune(&radio, 14_074_000);
    let mut a = source_at(&addr, 14_074_000, true);
    let mut b = source_at(&addr, 14_074_000, true);
    assert_eq!(frame(&mut a).bins.len(), FFT);
    assert_eq!(frame(&mut b).bins.len(), FFT);
}

#[test]
fn the_emulator_does_not_serve_the_console_protocol() {
    // A statement about layering, kept as a test because it is exactly the
    // kind of thing that gets quietly re-added. The emulator is a radio: a
    // CAT port, an ACC2 connector and a CN4 header. The control program is
    // the thing that owns a radio and serves consoles.
    // Checks for a dependency line, not the word: the manifest's comments
    // explain why the dependency is absent, and a substring test on the
    // name fails on its own explanation.
    let offending: Vec<&str> = include_str!("../Cargo.toml")
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .filter(|line| line.starts_with("cat-native"))
        .collect();
    assert!(
        offending.is_empty(),
        "the emulator has taken a dependency on the console protocol again: {offending:?}"
    );
}
