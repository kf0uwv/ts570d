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

//! `radio::PttLine`, against a radio.
//!
//! The emulator's COM port is a real RFC 2217 device server with real
//! DTR/RTS/CTS (`docs/adr/0009`), so the capability can be exercised the way
//! the console exercises it — through `Ts570d`, through `SerialCatSession`,
//! over a socket — rather than against a hand-written double that would only
//! prove the double agrees with itself.
//!
//! `emulator/tests/acc2_if.rs` holds the *transport* to the ACC2-IF
//! datasheet. This file holds the `radio` crate's capability to the same
//! behaviour, one layer up, and is the only place the `PttLine` trait meets
//! a radio that can be keyed.
//!
//! Linux-only for the same reason `tests/integration.rs` is: the emulator
//! crate is Unix-only.
#![cfg(target_os = "linux")]

use std::sync::Arc;
use std::time::{Duration, Instant};

use cat_transport_rfc2217::{Rfc2217Config, Rfc2217Port};
use cat_transport_serial::SerialCatSession;
use emulator::com::{self, SharedAcc2};
use emulator::emulator::{new_shared_radio, SharedRadio};
use radio::{PttLine, PttLineKind, Ts570d};

struct Bench {
    addr: String,
    radio: SharedRadio,
    #[allow(dead_code)]
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
    /// The console's own composition: an RFC 2217 port, framed by
    /// `SerialCatSession`, wrapped in the typed client.
    fn console(&self) -> Ts570d<SerialCatSession<Rfc2217Port>> {
        let port = Rfc2217Port::connect(
            self.addr.as_str(),
            Rfc2217Config {
                // Exactly what `src/main.rs` opens with.
                initial_dtr: false,
                initial_rts: true,
                ..Rfc2217Config::default()
            },
        )
        .expect("connect to the radio's COM port");
        Ts570d::new(SerialCatSession::new(port))
    }

    fn transmitting(&self) -> bool {
        self.radio.lock().unwrap().radio().state().tx
    }
}

/// Sockets and threads are genuinely concurrent; there is nothing to
/// synchronise on.
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

#[test]
fn set_ptt_line_keys_the_transmitter() {
    // The whole point of the capability: `PttLine::set_ptt_line` on the
    // radio object the console holds puts the radio into transmit, through
    // the same DTR pin the ACC2-IF opto hangs off.
    let bench = bench();
    let console = bench.console();

    assert!(!bench.transmitting(), "connecting must not key");

    console
        .set_ptt_line(PttLineKind::Dtr, true)
        .expect("assert DTR through the capability");
    eventually("the transmitter to key", || bench.transmitting());

    console
        .set_ptt_line(PttLineKind::Dtr, false)
        .expect("release DTR through the capability");
    eventually("the transmitter to drop", || !bench.transmitting());
}

#[test]
fn the_handle_the_console_uses_keys_the_same_radio() {
    // `ui::run_with_ptt_line` never sees the `Ts570d` -- the radio task owns
    // that. It keys through a handle taken before the radio was handed over,
    // and this is the test that the handle and the radio are the same port.
    let bench = bench();
    let mut console = bench.console();
    let handle = console.ptt_line_handle();

    assert!(handle.ptt_line_available());

    handle
        .set_ptt_line(PttLineKind::Dtr, true)
        .expect("key through the handle");
    eventually("the transmitter to key through the handle", || {
        bench.transmitting()
    });

    // And the radio it keyed is still the radio it talks to.
    let freq = futures::executor::block_on(console.get_vfo_a());
    assert!(
        freq.is_ok(),
        "CAT still works while the line is up: {freq:?}"
    );

    handle
        .set_ptt_line(PttLineKind::Dtr, false)
        .expect("unkey through the handle");
    eventually("the transmitter to drop", || !bench.transmitting());
}

#[test]
fn a_port_with_lines_reports_the_capability_present() {
    let bench = bench();
    let console = bench.console();
    eventually("the capability to be reported", || {
        console.ptt_line_available()
    });
}

#[test]
fn the_handshake_read_back_follows_the_radio() {
    // CTS is the reading an operator acts on: it says the radio's COM port
    // is alive. DSR and DCD are not wired by this radio and must read low,
    // so nobody waits for them.
    let bench = bench();
    let console = bench.console();

    eventually("CTS to come up", || {
        console.read_handshake().map(|h| h.cts).unwrap_or(false)
    });
    let handshake = console.read_handshake().expect("read the handshake");
    assert!(!handshake.dsr);
    assert!(!handshake.dcd);
}

#[test]
fn the_idle_level_leaves_the_radio_receiving_and_answering() {
    // `PttLineKind::idle_level` is not "everything low": RTS is the radio's
    // receive-enable input and it withholds CAT responses while that line is
    // down. This is the test that the state the console restores on Esc, on
    // [Q] and on the panic path is a state the console can still work in.
    let bench = bench();
    let mut console = bench.console();

    console.set_ptt_line(PttLineKind::Dtr, true).unwrap();
    console.set_ptt_line(PttLineKind::Rts, false).unwrap();
    eventually("the transmitter to key", || bench.transmitting());

    for line in [PttLineKind::Dtr, PttLineKind::Rts] {
        console.set_ptt_line(line, line.idle_level()).unwrap();
    }

    eventually("the transmitter to drop", || !bench.transmitting());
    std::thread::sleep(Duration::from_millis(50));
    futures::executor::block_on(console.get_vfo_a()).expect("the radio answers again");
}
