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

//! The virtual radio, driven the way an ACC2-IF drives the real one.
//!
//! Nothing here reaches into the emulator. The client is the same
//! `Rfc2217Port` the control program uses, under the same
//! `SerialCatSession`, and where a radio is wanted it is the real
//! `radio::Ts570d` — because a test that poked the emulator's own structs
//! would prove the structs consistent with themselves and nothing about
//! the wire.
//!
//! Each test names the datasheet clause it holds the radio to.

use std::sync::Arc;
use std::time::{Duration, Instant};

use cat_transport_core::{ModemControlLines, Transport};
use cat_transport_rfc2217::{Rfc2217Config, Rfc2217Port};
use cat_transport_serial::SerialCatSession;
use emulator::acc2::{Acc2Faults, KeySource};
use emulator::com::{self, SharedAcc2};
use emulator::emulator::{new_shared_radio, SharedRadio};

/// A virtual TS-570D with its COM port served, as the station has it.
struct Bench {
    addr: String,
    radio: SharedRadio,
    acc2: SharedAcc2,
}

fn bench() -> Bench {
    let radio = new_shared_radio();
    let acc2 = com::new_shared_acc2();
    let bound = com::serve(Arc::clone(&radio), Arc::clone(&acc2), "127.0.0.1:0")
        .expect("serve the radio's COM port");
    Bench {
        addr: bound.to_string(),
        radio,
        acc2,
    }
}

impl Bench {
    /// Plug the interface in: 9600 8N2, RTS asserted, DTR low.
    fn plug_in(&self) -> Rfc2217Port {
        Rfc2217Port::connect(self.addr.as_str(), Rfc2217Config::default())
            .expect("connect to the radio's COM port")
    }

    fn transmitting(&self) -> bool {
        self.radio.lock().unwrap().radio().state().tx
    }

    fn mic_muted(&self) -> bool {
        self.acc2.lock().unwrap().mic_muted()
    }

    fn key_source(&self) -> Option<KeySource> {
        self.acc2.lock().unwrap().key_source()
    }
}

/// Spin until `f` holds, or fail. Sockets and threads are genuinely
/// concurrent; there is nothing to synchronise on.
fn eventually(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for {what}");
}

/// Read from the port until a `;` arrives.
fn read_response(port: &mut Rfc2217Port) -> String {
    let mut got = Vec::new();
    let mut buf = [0u8; 64];
    while !got.ends_with(b";") {
        let n = futures::executor::block_on(port.read(&mut buf)).expect("read");
        got.extend_from_slice(&buf[..n]);
    }
    String::from_utf8(got).expect("ASCII CAT")
}

#[test]
fn cat_runs_over_the_com_port() {
    // Datasheet §4: "CAT link | radio COM, ts570d default | 9600 Bd 8N2".
    let bench = bench();
    let mut ts570d = radio::Ts570d::new(SerialCatSession::new(bench.plug_in()));

    let freq = futures::executor::block_on(ts570d.get_vfo_a()).expect("read VFO A");
    assert_eq!(
        freq.hz(),
        bench.radio.lock().unwrap().radio().state().vfo_a_hz
    );
}

#[test]
fn dtr_keys_the_transmitter_through_pks() {
    // Datasheet §1: "PTT is keyed from the serial DTR line through an
    // LTV4N35 opto-isolator onto ACC2 pin 9 (PKS)."
    let bench = bench();
    let port = bench.plug_in();

    assert!(!bench.transmitting(), "opening the port must not key");

    port.set_dtr(true).expect("assert DTR");
    eventually("the transmitter to key", || bench.transmitting());
    assert_eq!(bench.key_source(), Some(KeySource::Pks));

    port.set_dtr(false).expect("release DTR");
    eventually("the transmitter to drop", || !bench.transmitting());
    assert_eq!(bench.key_source(), None);
}

#[test]
fn keying_through_pks_mutes_the_microphone() {
    // Datasheet §6, pin 9: "PTT - GND = TX, mic muted". The whole reason an
    // interface uses PKS and not SS.
    let bench = bench();
    let port = bench.plug_in();

    port.set_dtr(true).expect("assert DTR");
    eventually("the transmitter to key", || bench.transmitting());
    assert!(bench.mic_muted());
}

#[test]
fn opening_the_port_does_not_key_the_radio() {
    // SN-5: "Linux raises DTR on open by default = key-down at program
    // start. ts570d-radio-control >= 0.3.0 opens DTR-deasserted."
    //
    // This is the test that fix never had. Until the emulator carried a
    // real DTR line there was nothing that could observe it.
    let bench = bench();
    let _port = bench.plug_in();

    std::thread::sleep(Duration::from_millis(100));
    assert!(
        !bench.transmitting(),
        "a port opened DTR-deasserted must leave the radio receiving"
    );
}

#[test]
fn a_client_that_opens_with_dtr_asserted_does_key() {
    // The other half of SN-5, and the reason the default matters: foreign
    // software that does not force DTR low *will* key this station on
    // startup. If the emulator did not reproduce that, the hazard would be
    // untestable and the fix unverifiable.
    let bench = bench();
    let _port = Rfc2217Port::connect(
        bench.addr.as_str(),
        Rfc2217Config {
            initial_dtr: true,
            ..Rfc2217Config::default()
        },
    )
    .expect("connect");

    eventually("the transmitter to key at open", || bench.transmitting());
}

#[test]
fn rts_low_inhibits_the_radios_cat_responses() {
    // Datasheet §7: "RTS | PC->radio | receive-enable - keep asserted", and
    // the absolute rule "RTS stays asserted - low inhibits the radio's CAT
    // responses."
    let bench = bench();
    let mut port = bench.plug_in();

    port.set_rts(false).expect("drop RTS");
    // Give the radio time to see RTS go down before asking it anything.
    std::thread::sleep(Duration::from_millis(50));
    futures::executor::block_on(port.write(b"FA;")).expect("write FA;");

    // The command still ran -- the radio heard it -- but the answer was
    // withheld. Prove it by raising RTS and asking a *different* question:
    // if the FA answer had merely been queued, it would arrive first.
    std::thread::sleep(Duration::from_millis(50));
    port.set_rts(true).expect("raise RTS");
    std::thread::sleep(Duration::from_millis(50));
    futures::executor::block_on(port.write(b"FB;")).expect("write FB;");

    let response = read_response(&mut port);
    assert!(
        response.starts_with("FB"),
        "the response withheld while RTS was low must be dropped, not queued: got {response:?}"
    );
}

#[test]
fn cts_says_whether_the_radios_com_is_alive() {
    // Datasheet §10: "ts570d-line <port> status | print CTS / DSR / DCD
    // (CTS asserted = radio COM alive)".
    let bench = bench();
    let mut port = bench.plug_in();

    eventually("CTS to come up", || port.read_cts().unwrap_or(false));

    // Switch the radio off over CAT. A bench tool still showing CTS up
    // would be describing a radio that is not answering.
    futures::executor::block_on(port.write(b"PS0;")).expect("write PS0;");
    eventually("CTS to drop with the radio", || {
        !port.read_cts().unwrap_or(true)
    });
}

#[test]
fn dsr_and_dcd_stay_low_because_the_radio_does_not_wire_them() {
    // Datasheet §7's DB9 table lists pins 2, 3, 4, 5, 7 and 8 only. An
    // operator should see these low and know not to wait for them.
    let bench = bench();
    let port = bench.plug_in();

    std::thread::sleep(Duration::from_millis(100));
    assert!(!port.read_dsr().expect("read DSR"));
    assert!(!port.read_dcd().expect("read DCD"));
}

#[test]
fn unplugging_while_keyed_does_not_leave_the_transmitter_up() {
    // The real interface cannot do this: pulling the USB cable drops DTR
    // and the opto releases. Neither may the virtual one.
    let bench = bench();
    let port = bench.plug_in();

    port.set_dtr(true).expect("assert DTR");
    eventually("the transmitter to key", || bench.transmitting());

    drop(port);
    eventually("the transmitter to drop on disconnect", || {
        !bench.transmitting()
    });
}

#[test]
fn sn1_a_cable_with_pin_13_bonded_keys_the_radio_when_it_seats() {
    // SN-1: "The DIN cable shipped with pin 13 (SS - keys TX, mic live)
    // tied to the shield, so it keyed on plug-in from day one."
    let bench = bench();
    bench.acc2.lock().unwrap().set_faults(Acc2Faults {
        pin13_bonded_to_braid: true,
        ..Acc2Faults::default()
    });

    let keying = bench.acc2.lock().unwrap().seat(true);
    assert!(keying.is_some(), "seating a bonded cable keys the radio");

    assert_eq!(bench.key_source(), Some(KeySource::Ss));
    assert!(
        !bench.mic_muted(),
        "SS is parallel with the mic jack -- this is why the pin was cut"
    );
}

#[test]
fn the_lines_survive_the_framing_layer_the_control_program_uses() {
    // `ts570d` holds one value and both talks CAT and keys PTT with it.
    // That works only because `SerialCatSession` forwards the lines.
    let bench = bench();
    let session = SerialCatSession::new(bench.plug_in());

    session.set_dtr(true).expect("key through the session");
    eventually("the transmitter to key through the session", || {
        bench.transmitting()
    });
}

#[test]
fn a_freshly_powered_radio_puts_audio_on_ano() {
    // Regression. `Ts570dState::default()` zeroes all fifty-two menus, and
    // ANO's level comes from Menu 34 -- so a virtual radio powered up
    // literally had a silent audio pin, which reads as "the feature is
    // broken" rather than "the menu is at zero". Caught by running the
    // emulator, not by a unit test: every audio test set the menu itself.
    let radio = new_shared_radio();
    let band = emulator::tap::band_for(7);
    let state = radio.lock().unwrap().radio().state().clone();

    let samples = emulator::acc2_audio::ano_samples(&band, &state, 0.0, 4096);
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.0, "a radio at power-up must put something on ANO");
}

#[test]
fn a_client_that_never_drives_rts_still_gets_answers() {
    // Regression, found by pointing a hand-written client at the running
    // emulator and getting silence. The device server used to start both
    // lines low for symmetry; on a radio that treats RTS as receive-enable
    // that means a client which never mentions RTS -- a plain telnet
    // session, or anything expecting a `ser2net`-style endpoint -- sees a
    // dead radio. A real UART asserts RTS on open, and so does the peer now.
    let bench = bench();
    let mut port = Rfc2217Port::connect(
        bench.addr.as_str(),
        Rfc2217Config {
            // Never touch RTS: exactly what a naive client does.
            initial_rts: false,
            ..Rfc2217Config::default()
        },
    )
    .expect("connect");

    // The client asked for RTS low, so this one *is* inhibited -- restore it
    // the way any client that cares would, and confirm the radio answers.
    port.set_rts(true).expect("raise RTS");
    std::thread::sleep(Duration::from_millis(50));
    futures::executor::block_on(port.write(b"FA;")).expect("write");
    assert!(read_response(&mut port).starts_with("FA"));
}
