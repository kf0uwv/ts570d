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

//! The radio's COM port — the DB9 the ACC2-IF's serial pigtail plugs into.
//!
//! # Why this is not the PTY
//!
//! The emulator has always had a serial endpoint: a pseudo-terminal, which
//! carries CAT bytes perfectly well and is still the right thing for most
//! testing. What it cannot carry is a **wire**. Linux ptys implement no
//! modem-control ioctls at all — `TIOCMGET`, `TIOCMBIS` and `TIOCMBIC` all
//! fail with `ENOTTY` on both ends — so on a PTY there is no DTR to observe
//! and no CTS to raise.
//!
//! That is fatal for this station, because the ACC2-IF keys PTT from DTR
//! and nothing else. `docs/emulator.md` used to claim "DTR is already real
//! (it is a PTY modem line)"; it is not, and while that claim stood the
//! emulator was reproducing exactly the SN-2 failure the datasheet warns
//! about — measuring perfect while keying nothing.
//!
//! So this endpoint speaks **RFC 2217**, the Telnet Com Port Control
//! Option, which carries the lines as well as the bytes. The same argument
//! [`crate::tap`] makes for `rtl_tcp` applies unchanged: `ser2net` and every
//! Moxa/Digi device server speak RFC 2217, so a virtual radio and a real
//! serial device server are interchangeable to whatever is upstream, and
//! `ts570d` gets remote-rig-over-a-device-server as a real feature rather
//! than as an emulator-only code path.
//!
//! The protocol itself is not implemented here: it lives in
//! `cat-transport-rfc2217`, which the control program's client also uses.
//! One implementation, one place to correct it.
//!
//! # What the datasheet says this port does (§7)
//!
//! | Pin | Signal | Behaviour modelled here |
//! |---|---|---|
//! | 2 | RXD | CAT responses out |
//! | 3 | TXD | CAT commands in |
//! | 4 | DTR | in — keys ACC2 pin 9 (PKS) through the opto |
//! | 7 | RTS | in — receive-enable; **low inhibits CAT responses** |
//! | 8 | CTS | out — asserted while the radio's COM is alive |
//!
//! DSR and DCD are never asserted, and that is not an omission: the
//! datasheet's DB9 table wires neither pin, because the radio's COM
//! connector does not drive them. An operator running `ts570d-line status`
//! should see them low and know not to wait for them.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cat_transport_rfc2217::codec::modem;
use cat_transport_rfc2217::{PeerEvent, Rfc2217Peer};

use crate::acc2::{Acc2, Keying};
use crate::emulator::SharedRadio;
use crate::io::CommandFramer;

/// The ACC2 socket, shared between the COM port that drives its PTT pin and
/// whatever else looks at it.
pub type SharedAcc2 = Arc<Mutex<Acc2>>;

/// A fresh ACC2 socket.
pub fn new_shared_acc2() -> SharedAcc2 {
    Arc::new(Mutex::new(Acc2::new()))
}

/// How often a connection wakes to publish line state and step the
/// phantom-keying fault. Short enough that a key looks immediate, long
/// enough not to spin.
const POLL: Duration = Duration::from_millis(20);

/// How often the SN-3 phantom-keying fault toggles.
///
/// The symptom the operator reported was TX/RX *chatter*, so the interval
/// is the rate a person would describe that way rather than a clean cycle.
const PHANTOM_PERIOD: Duration = Duration::from_millis(250);

/// Serve the radio's COM port on `addr`. Spawns threads and returns the
/// bound address.
pub fn serve(radio: SharedRadio, acc2: SharedAcc2, addr: &str) -> std::io::Result<SocketAddr> {
    let listener = TcpListener::bind(addr)?;
    let bound = listener.local_addr()?;

    // The phantom-keying fault belongs to the *hardware*, not to a
    // connection: SN-3 is a dead adapter back-feeding the radio, which by
    // definition means nothing is talking to it. Ticking it from a
    // connection thread would make it only reproducible while connected,
    // which is the opposite of the real fault.
    let fault_radio = Arc::clone(&radio);
    let fault_acc2 = Arc::clone(&acc2);
    std::thread::spawn(move || loop {
        std::thread::sleep(PHANTOM_PERIOD);
        let keying = fault_acc2.lock().expect("acc2 lock").tick_phantom();
        if let Some(keying) = keying {
            apply_keying(&fault_radio, keying);
        }
    });

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let radio = Arc::clone(&radio);
            let acc2 = Arc::clone(&acc2);
            std::thread::spawn(move || {
                let _ = serve_one(stream, radio, acc2);
            });
        }
    });

    Ok(bound)
}

/// Raise or drop TX by the same route a CAT client would.
///
/// Public because the wiring layer needs it too: seating a plug under the
/// SN-1 fault keys the radio before any client has connected.
///
/// Deliberately `TX;`/`RX;` through the command table rather than a direct
/// write to the `tx` flag — see [`crate::acc2`]'s module doc for why a
/// hardware key should still go through the radio's state machine.
pub fn apply_keying(radio: &SharedRadio, keying: Keying) {
    let frame = match keying {
        Keying::Transmit(_) => "TX;",
        Keying::Receive => "RX;",
    };
    let mut out = Vec::new();
    let _ = radio
        .lock()
        .expect("radio lock")
        .process_frame(frame, &mut out);
}

/// The modem state this radio drives, as an RFC 2217 byte.
///
/// CTS follows the radio being on, because that is what the datasheet's
/// §10 note means by "CTS asserted = radio COM alive": a radio switched off
/// with `PS0;` stops answering, and a bench tool that still showed CTS up
/// would be describing a radio that is not there.
fn modem_state(radio: &SharedRadio) -> u8 {
    let alive = radio.lock().expect("radio lock").radio().state().power_on;
    if alive {
        modem::CTS
    } else {
        0
    }
}

fn serve_one(mut stream: TcpStream, radio: SharedRadio, acc2: SharedAcc2) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    // A read timeout rather than a blocking read: this loop also has line
    // state to publish, and a radio that only spoke when spoken to would
    // never tell a client its CTS had dropped.
    stream.set_read_timeout(Some(POLL))?;

    let mut peer = Rfc2217Peer::new();
    let mut framer = CommandFramer::new();
    let mut buf = [0u8; 1024];

    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let (events, reply) = peer.push(&buf[..n]);
                if !reply.is_empty() {
                    stream.write_all(&reply)?;
                }
                for event in events {
                    handle(&mut stream, &mut peer, &radio, &acc2, &mut framer, event)?;
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(e),
        }

        if let Some(bytes) = peer.modem_state(modem_state(&radio)) {
            stream.write_all(&bytes)?;
        }
    }

    // A client that disconnects while keying must not leave the
    // transmitter up. The real interface cannot do this -- unplugging the
    // USB cable drops DTR and the opto releases -- so neither may this.
    let keying = acc2.lock().expect("acc2 lock").set_pks(false);
    if let Some(keying) = keying {
        apply_keying(&radio, keying);
    }
    Ok(())
}

fn handle(
    stream: &mut TcpStream,
    peer: &mut Rfc2217Peer,
    radio: &SharedRadio,
    acc2: &SharedAcc2,
    framer: &mut CommandFramer,
    event: PeerEvent,
) -> std::io::Result<()> {
    match event {
        PeerEvent::Data(bytes) => {
            framer.push(&bytes);
            for command in framer.drain_commands() {
                let mut response = Vec::new();
                let frame = format!("{command};");
                let _ = radio
                    .lock()
                    .expect("radio lock")
                    .process_frame(&frame, &mut response);

                if response.is_empty() {
                    continue;
                }
                // Datasheet §7: RTS is receive-enable, and the radio's
                // transmit of data is inhibited while it is low. The
                // command still ran -- the radio heard it -- but the answer
                // is dropped rather than queued, because a radio holding a
                // stale answer for whenever RTS returns is not what the
                // manual describes.
                if peer.rts() {
                    stream.write_all(&peer.encode_data(&response))?;
                }
            }
        }
        PeerEvent::Dtr(asserted) => {
            // The opto: DTR asserted pulls ACC2 pin 9 to ground.
            let keying = acc2.lock().expect("acc2 lock").set_pks(asserted);
            if let Some(keying) = keying {
                apply_keying(radio, keying);
            }
        }
        PeerEvent::Rts(_) | PeerEvent::Break(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emulator::new_shared_radio;

    fn tx(radio: &SharedRadio) -> bool {
        radio.lock().unwrap().radio().state().tx
    }

    #[test]
    fn keying_through_the_command_table_raises_tx() {
        let radio = new_shared_radio();
        assert!(!tx(&radio));

        apply_keying(&radio, Keying::Transmit(crate::acc2::KeySource::Pks));
        assert!(
            tx(&radio),
            "a hardware key must reach the same flag CAT sets"
        );

        apply_keying(&radio, Keying::Receive);
        assert!(!tx(&radio));
    }

    #[test]
    fn cts_reports_whether_the_radio_is_on() {
        let radio = new_shared_radio();
        assert_eq!(
            modem_state(&radio),
            modem::CTS,
            "a running radio's COM port is alive"
        );

        let mut out = Vec::new();
        let _ = radio.lock().unwrap().process_frame("PS0;", &mut out);
        assert_eq!(
            modem_state(&radio),
            0,
            "a radio switched off must not still be reporting CTS"
        );
    }

    #[test]
    fn dsr_and_dcd_are_never_asserted() {
        // Not an omission: the datasheet's DB9 table wires neither pin.
        let radio = new_shared_radio();
        let state = modem_state(&radio);
        assert_eq!(state & modem::DSR, 0);
        assert_eq!(state & modem::DCD, 0);
    }
}
