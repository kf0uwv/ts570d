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

//! A console talking to the emulator, over a socket, with signals in the
//! band.
//!
//! The acceptance test for "test with a dummy radio": connect, see what
//! the radio is doing, change it, watch the change stick, and watch the
//! spectrum window follow the dial.

use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cat_native::{Command, Connection, ModeId, RadioHost, ServerMessage};
use emulator::emulator::{new_shared_radio, SharedRadio};
use emulator::network::EmulatedRadio;

fn serve() -> (u16, SharedRadio) {
    let radio = new_shared_radio();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let host = Arc::new(EmulatedRadio::new(radio.clone(), 7));
    std::thread::spawn(move || {
        let _ = cat_native::serve(listener, host);
    });
    (port, radio)
}

fn connect(port: u16, spectrum: bool) -> Connection {
    Connection::connect(("127.0.0.1", port), spectrum).expect("connect")
}

fn next_frame(conn: &mut Connection) -> cat_signal::SpectrumFrame {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(frame) = conn.poll(Some(Duration::from_millis(100))).unwrap() {
            return frame;
        }
        assert!(Instant::now() < deadline, "no spectrum frame arrived");
    }
}

fn tune(radio: &SharedRadio, hz: u64) {
    let mut guard = radio.lock().unwrap();
    let mut out = Vec::new();
    let _ = guard.process_frame(format!("FA{hz:011};"), &mut out);
}

#[test]
fn a_console_connecting_is_told_it_is_a_ts570d() {
    let (port, _radio) = serve();
    let conn = connect(port, false);
    assert_eq!(conn.capabilities().model, "Kenwood TS-570D");
    assert_eq!(conn.capabilities().rx_range.max_hz, 60_000_000);
    assert_eq!(conn.capabilities().menu.unwrap().item_count, 52);
    assert_eq!(conn.capabilities().memory.unwrap().channels.max, 99);
}

#[test]
fn the_console_can_see_the_dial() {
    // The thing the protocol could not do until now.
    let (port, _radio) = serve();
    let mut conn = connect(port, false);
    let state = conn.read_state().expect("read state");
    assert!(
        (500_000..=60_000_000).contains(&state.vfo_a_hz),
        "dial at {} Hz",
        state.vfo_a_hz
    );
}

#[test]
fn tuning_over_the_network_moves_this_emulators_own_radio() {
    // The network interface is a second front-end onto the same radio, not
    // a second radio. If these diverged, the socket would be a dummy that
    // disagrees with the dummy -- which looks exactly like a console bug.
    let (port, radio) = serve();
    let mut conn = connect(port, false);
    assert_eq!(
        conn.command(Command::Retune { hz: 14_074_000 }).unwrap(),
        ServerMessage::Ack
    );
    assert_eq!(
        radio.lock().unwrap().radio().state().vfo_a_hz,
        14_074_000,
        "the emulator's own state did not move"
    );
    assert_eq!(conn.read_state().unwrap().vfo_a_hz, 14_074_000);
}

#[test]
fn setting_a_mode_over_the_network_sticks() {
    let (port, radio) = serve();
    let mut conn = connect(port, false);
    conn.command(Command::SetMode {
        mode: ModeId::CwUpper,
    })
    .unwrap();
    assert_eq!(conn.read_state().unwrap().mode, ModeId::CwUpper);
    // Straight through the radio's own CAT handling, so the emulator's
    // own display agrees.
    assert_eq!(radio.lock().unwrap().radio().state().mode, 3);
}

#[test]
fn a_mode_this_radio_does_not_have_is_refused() {
    // C4FM is an FT-991A mode. The capability set refuses it before the
    // translation layer is asked for a CAT frame it could not build.
    let (port, _radio) = serve();
    let mut conn = connect(port, false);
    assert!(matches!(
        conn.command(Command::SetMode { mode: ModeId::C4fm })
            .unwrap(),
        ServerMessage::Error { .. }
    ));
}

#[test]
fn a_frequency_outside_coverage_is_refused_and_the_dial_does_not_move() {
    let (port, radio) = serve();
    let mut conn = connect(port, false);
    let before = radio.lock().unwrap().radio().state().vfo_a_hz;
    assert!(matches!(
        conn.command(Command::SetFrequency {
            vfo: 0,
            hz: 450_000_000
        })
        .unwrap(),
        ServerMessage::Error { .. }
    ));
    assert_eq!(radio.lock().unwrap().radio().state().vfo_a_hz, before);
}

#[test]
fn the_s_meter_reads_back_with_the_radios_own_scale() {
    let (port, _radio) = serve();
    let mut conn = connect(port, false);
    let descriptor = conn
        .capabilities()
        .meters
        .iter()
        .find(|m| m.kind == cat_native::MeterKind::S)
        .expect("has an S meter")
        .clone();
    assert_eq!(descriptor.raw_range.max, 30);
    // Its table crossed the wire, so a remote console reads the same
    // S-units a local one does.
    assert_eq!(descriptor.s_units.unwrap().label(24), "S9+10");

    match conn
        .command(Command::ReadMeter {
            kind: cat_native::MeterKind::S,
        })
        .unwrap()
    {
        ServerMessage::Meter(sample) => assert!(sample.raw <= 30),
        other => panic!("expected a reading, got {other:?}"),
    }
}

#[test]
fn there_are_signals_in_the_band() {
    // Not just a noise floor. A console pointed at a flat spectrum cannot
    // be tested for anything a console is for.
    let (port, radio) = serve();
    tune(&radio, 14_074_000);
    let mut conn = connect(port, true);

    let mut peak = f32::NEG_INFINITY;
    let mut floor = f32::INFINITY;
    // Several frames: a digital signal transmits in slots, so one frame is
    // not evidence of an empty band.
    for _ in 0..5 {
        let frame = next_frame(&mut conn);
        for p in &frame.bins {
            peak = peak.max(*p);
            floor = floor.min(*p);
        }
    }
    assert!(
        peak > floor + 15.0,
        "the band looks flat: peak {peak}, floor {floor}"
    );
}

#[test]
fn the_spectrum_window_follows_the_dial() {
    // An IF tap is dial-centred by construction. This is the property the
    // GUI's click-to-tune depends on, and the reason the fixture places
    // signals at absolute frequencies.
    let (port, _radio) = serve();
    let mut conn = connect(port, true);

    conn.command(Command::Retune { hz: 14_074_000 }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if next_frame(&mut conn).center_hz == 14_074_000 {
            break;
        }
        assert!(Instant::now() < deadline, "window never reached 14.074");
    }

    conn.command(Command::Retune { hz: 21_074_000 }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if next_frame(&mut conn).center_hz == 21_074_000 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the window never followed the dial"
        );
    }
}

#[test]
fn a_frame_is_low_frequency_first_so_a_console_need_not_know_about_the_tap() {
    // cat-signal's invariant. The TS-570D's real tap is mirrored and the
    // correction belongs in the source; this source has nothing to mirror,
    // but a console must be able to rely on the same rule either way.
    let (port, _radio) = serve();
    let mut conn = connect(port, true);
    let frame = next_frame(&mut conn);
    let first = frame.bin_frequency_hz(0).unwrap();
    let last = frame.bin_frequency_hz(frame.bins.len() - 1).unwrap();
    assert!(first < last, "bins are not low-frequency-first");
    let (low, high) = frame.range_hz();
    assert!(low < frame.center_hz as f64 && (frame.center_hz as f64) < high);
}

#[test]
fn the_same_seed_puts_the_same_signals_in_the_same_places() {
    // "The carrier that was here yesterday" is a useful thing to be able
    // to say while debugging a console.
    let a = EmulatedRadio::new(new_shared_radio(), 99);
    let b = EmulatedRadio::new(new_shared_radio(), 99);
    let fa = RadioHost::spectrum(&a).unwrap();
    let fb = RadioHost::spectrum(&b).unwrap();
    assert_eq!(fa.center_hz, fb.center_hz);
    // Rendered at different instants, so compare where the signals are
    // rather than their exact levels.
    let peak = |f: &cat_signal::SpectrumFrame| {
        f.bins
            .iter()
            .enumerate()
            .max_by(|x, y| x.1.partial_cmp(y.1).unwrap())
            .unwrap()
            .0
    };
    assert_eq!(peak(&fa), peak(&fb));
}

#[test]
fn two_consoles_see_one_radio() {
    let (port, _radio) = serve();
    let mut a = connect(port, false);
    let mut b = connect(port, false);
    a.command(Command::Retune { hz: 7_030_000 }).unwrap();
    assert_eq!(b.read_state().unwrap().vfo_a_hz, 7_030_000);
}
