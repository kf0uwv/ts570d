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

//! Keying the transmitter from a serial handshake line.
//!
//! A great many PTT interfaces are not CAT at all: they hang an opto-isolator
//! off DTR (or RTS) and drop it onto the radio's keying pin — for this station,
//! ACC2 pin 9 (PKS) through an LTV4N35, described in
//! `docs/adr/0009-acc2-if-virtual-hardware.md`. The line is a property of the
//! *port*, not of the protocol, so nothing in the CAT command table can reach
//! it and [`crate::Radio`] is the wrong place to put it.
//!
//! [`PttLine`] is therefore an **optional capability**, in the sense ADR 0010
//! uses the word: its methods have default bodies that report the capability
//! absent, and a session with no modem-control lines (a TCP client, a
//! pseudo-terminal) keeps those defaults. A console asks
//! [`PttLine::ptt_line_available`] and hides the control rather than offering
//! one that always errors.
//!
//! # Why the availability probe is a probe
//!
//! `ptt_line_available` does not answer "is this type one that has lines"; it
//! asks the port. `cat_transport_core::NoModemControlLines` implements
//! `ModemControlLines` by returning an error from every method, and a Linux
//! pseudo-terminal implements the ioctls not at all (`ENOTTY` on both ends) —
//! both satisfy every bound and neither has a line. Reading CTS is the cheapest
//! question whose answer distinguishes them.
//!
//! # Idle is not "everything low"
//!
//! DTR's idle is deasserted: asserting it *is* key-down. RTS's idle is
//! **asserted** — the TS-570D's RTS input is receive-enable and the radio
//! withholds its CAT responses while the line is low (instruction manual
//! p. 70). [`HandshakeState`] is what a console can read back; DTR and RTS are
//! outputs and cannot be read back at all, which is why a caller that moves
//! them must remember where it left them.

use cat_transport_core::{CatSession, ModemControlLines, TransportError};

use crate::ts570d::{SharedSession, Ts570d};
use crate::{RadioError, RadioResult};

/// Which handshake output a console is driving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PttLineKind {
    /// Data Terminal Ready — DB9 pin 4. The line the ACC2-IF keys from.
    Dtr,
    /// Request To Send — DB9 pin 7. Also the radio's receive-enable input, so
    /// keying from it and talking CAT over the same handle are in conflict.
    Rts,
}

impl PttLineKind {
    /// The name an operator knows the line by.
    pub fn name(self) -> &'static str {
        match self {
            PttLineKind::Dtr => "DTR",
            PttLineKind::Rts => "RTS",
        }
    }

    /// Where the line sits when nothing is being keyed.
    ///
    /// `false` for DTR (asserting it keys the transmitter) and `true` for RTS
    /// (the radio stops answering CAT while it is low). See the module doc.
    pub fn idle_level(self) -> bool {
        match self {
            PttLineKind::Dtr => false,
            PttLineKind::Rts => true,
        }
    }
}

/// The handshake *inputs*, as the port reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HandshakeState {
    /// Clear To Send — DB9 pin 8. On this radio, asserted means its COM port
    /// is alive (it drops when the radio is switched off).
    pub cts: bool,
    /// Data Set Ready — DB9 pin 6. Not wired by the TS-570D; expected low.
    pub dsr: bool,
    /// Data Carrier Detect — DB9 pin 1. Not wired by the TS-570D; expected low.
    pub dcd: bool,
}

/// Optional capability: drive the port's PTT line and read its handshake.
///
/// Every method has a default body that reports the capability absent, so
/// implementing this trait for a transport that has no lines is
/// `impl PttLine for MySession {}` and costs nothing.
pub trait PttLine {
    /// Assert or release `line`.
    ///
    /// Asserting the line an interface is wired to **keys the transmitter**.
    fn set_ptt_line(&self, line: PttLineKind, asserted: bool) -> RadioResult<()> {
        let _ = (line, asserted);
        Err(RadioError::Unsupported)
    }

    /// Read CTS/DSR/DCD.
    fn read_handshake(&self) -> RadioResult<HandshakeState> {
        Err(RadioError::Unsupported)
    }

    /// Whether this port actually has handshake lines.
    ///
    /// A console shows its PTT-line control only when this is `true`.
    fn ptt_line_available(&self) -> bool {
        false
    }
}

/// A [`PttLine`] over a shared session, usable while something else owns the
/// radio.
///
/// The console's radio task owns `Ts570d<S>` and holds it across `.await` for
/// the length of every CAT command; the key that unkeys the transmitter must
/// not have to wait for that. [`Ts570d::ptt_line_handle`] hands out one of
/// these — a clone of the same `Rc`-backed session — so the line can be moved
/// from wherever the operator's keystroke is handled.
pub struct PttLineHandle<S: CatSession<Error = TransportError>> {
    session: SharedSession<S>,
}

impl<S> PttLine for PttLineHandle<S>
where
    S: CatSession<Error = TransportError> + ModemControlLines,
{
    fn set_ptt_line(&self, line: PttLineKind, asserted: bool) -> RadioResult<()> {
        set_line(&self.session, line, asserted)
    }

    fn read_handshake(&self) -> RadioResult<HandshakeState> {
        read_handshake(&self.session)
    }

    fn ptt_line_available(&self) -> bool {
        probe(&self.session)
    }
}

impl<S> PttLine for Ts570d<S>
where
    S: CatSession<Error = TransportError> + ModemControlLines,
{
    fn set_ptt_line(&self, line: PttLineKind, asserted: bool) -> RadioResult<()> {
        set_line(self.shared_session(), line, asserted)
    }

    fn read_handshake(&self) -> RadioResult<HandshakeState> {
        read_handshake(self.shared_session())
    }

    fn ptt_line_available(&self) -> bool {
        probe(self.shared_session())
    }
}

/// A radio whose port has no PTT line, or whose station does not key from
/// one.
///
/// Every method is the trait's default, which is the honest answer already:
/// `ptt_line_available()` is false and the two operations report
/// [`RadioError::Unsupported`]. A console handed one hides the `[P]` item
/// rather than offering a control that can only ever fail.
///
/// Used for two different situations that want the same behaviour: a
/// transport with no modem lines at all (a TCP client), and a **station
/// that has them and does not key with them** — `ts570d --cat-only`. The
/// second is a statement about the wiring, not about the hardware.
pub struct NoPttLine;

impl PttLine for NoPttLine {}

impl<S> Ts570d<S>
where
    S: CatSession<Error = TransportError> + ModemControlLines + 'static,
{
    /// A handle to this radio's PTT line that does not borrow the radio.
    ///
    /// See [`PttLineHandle`] for why a console needs one.
    pub fn ptt_line_handle(&self) -> Box<dyn PttLine> {
        Box::new(PttLineHandle {
            session: self.shared_session().clone(),
        })
    }
}

// ---------------------------------------------------------------------------
// The three operations, written once
// ---------------------------------------------------------------------------

fn set_line<S>(session: &SharedSession<S>, line: PttLineKind, asserted: bool) -> RadioResult<()>
where
    S: CatSession<Error = TransportError> + ModemControlLines,
{
    session
        .try_with(|s| match line {
            PttLineKind::Dtr => s.set_dtr(asserted),
            PttLineKind::Rts => s.set_rts(asserted),
        })
        .ok_or(RadioError::Busy)?
        .map_err(RadioError::Transport)
}

fn read_handshake<S>(session: &SharedSession<S>) -> RadioResult<HandshakeState>
where
    S: CatSession<Error = TransportError> + ModemControlLines,
{
    session
        .try_with(|s| {
            Ok(HandshakeState {
                cts: s.read_cts()?,
                dsr: s.read_dsr()?,
                dcd: s.read_dcd()?,
            })
        })
        .ok_or(RadioError::Busy)?
        .map_err(RadioError::Transport)
}

fn probe<S>(session: &SharedSession<S>) -> bool
where
    S: CatSession<Error = TransportError> + ModemControlLines,
{
    matches!(session.try_with(|s| s.read_cts()), Some(Ok(_)))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use async_trait::async_trait;
    use cat_transport_core::ResponseDisposition;

    use super::*;

    /// A session with real handshake lines, remembered in `Cell`s because
    /// `ModemControlLines` is a `&self` interface (which is the whole reason
    /// this capability can be offered while something else owns the radio).
    #[derive(Default)]
    struct WiredSession {
        dtr: Cell<bool>,
        rts: Cell<bool>,
        cts: Cell<bool>,
    }

    #[async_trait(?Send)]
    impl CatSession for WiredSession {
        type Error = TransportError;

        async fn execute(
            &mut self,
            _request: &[u8],
            _response: &mut Vec<u8>,
        ) -> Result<ResponseDisposition, Self::Error> {
            Ok(ResponseDisposition::NoResponse)
        }

        async fn send(&mut self, _request: &[u8]) -> Result<(), Self::Error> {
            Ok(())
        }

        fn flush_rx(&mut self) {}
    }

    impl ModemControlLines for WiredSession {
        fn set_rts(&self, asserted: bool) -> Result<(), TransportError> {
            self.rts.set(asserted);
            Ok(())
        }
        fn set_dtr(&self, asserted: bool) -> Result<(), TransportError> {
            self.dtr.set(asserted);
            Ok(())
        }
        fn read_cts(&self) -> Result<bool, TransportError> {
            Ok(self.cts.get())
        }
        fn read_dsr(&self) -> Result<bool, TransportError> {
            Ok(false)
        }
        fn read_dcd(&self) -> Result<bool, TransportError> {
            Ok(true)
        }
    }

    /// The shape `cat_transport_core::NoModemControlLines` and a Linux
    /// pseudo-terminal both have: satisfies the bound, has no lines.
    #[derive(Default)]
    struct UnwiredSession;

    #[async_trait(?Send)]
    impl CatSession for UnwiredSession {
        type Error = TransportError;

        async fn execute(
            &mut self,
            _request: &[u8],
            _response: &mut Vec<u8>,
        ) -> Result<ResponseDisposition, Self::Error> {
            Ok(ResponseDisposition::NoResponse)
        }

        async fn send(&mut self, _request: &[u8]) -> Result<(), Self::Error> {
            Ok(())
        }

        fn flush_rx(&mut self) {}
    }

    fn no_lines() -> TransportError {
        TransportError::Other("this transport has no modem control lines".to_string())
    }

    impl ModemControlLines for UnwiredSession {
        fn set_rts(&self, _asserted: bool) -> Result<(), TransportError> {
            Err(no_lines())
        }
        fn set_dtr(&self, _asserted: bool) -> Result<(), TransportError> {
            Err(no_lines())
        }
        fn read_cts(&self) -> Result<bool, TransportError> {
            Err(no_lines())
        }
        fn read_dsr(&self) -> Result<bool, TransportError> {
            Err(no_lines())
        }
        fn read_dcd(&self) -> Result<bool, TransportError> {
            Err(no_lines())
        }
    }

    /// A radio whose session has no lines *and* no `ModemControlLines` impl
    /// at all — the `--server` case. It keeps the trait's default bodies.
    struct SessionlessRadio;
    impl PttLine for SessionlessRadio {}

    #[test]
    fn keying_dtr_reaches_the_port() {
        // The one behaviour the ACC2-IF depends on: assert DTR, and the pin
        // moves. `docs/adr/0009` §1 wires that pin to PKS.
        let radio = Ts570d::new(WiredSession::default());
        radio
            .set_ptt_line(PttLineKind::Dtr, true)
            .expect("assert DTR");
        assert!(radio.shared_session().try_with(|s| s.dtr.get()).unwrap());

        radio
            .set_ptt_line(PttLineKind::Dtr, false)
            .expect("release DTR");
        assert!(!radio.shared_session().try_with(|s| s.dtr.get()).unwrap());
    }

    #[test]
    fn dtr_and_rts_are_not_the_same_line() {
        let radio = Ts570d::new(WiredSession::default());
        radio.set_ptt_line(PttLineKind::Rts, true).unwrap();
        assert!(radio.shared_session().try_with(|s| s.rts.get()).unwrap());
        assert!(
            !radio.shared_session().try_with(|s| s.dtr.get()).unwrap(),
            "keying RTS must not also key DTR"
        );
    }

    #[test]
    fn the_handshake_is_read_from_the_port_not_remembered() {
        let radio = Ts570d::new(WiredSession::default());
        assert_eq!(
            radio.read_handshake().unwrap(),
            HandshakeState {
                cts: false,
                dsr: false,
                dcd: true
            }
        );
        radio.shared_session().try_with(|s| s.cts.set(true));
        assert!(radio.read_handshake().unwrap().cts);
    }

    #[test]
    fn a_port_with_lines_reports_the_capability_present() {
        let radio = Ts570d::new(WiredSession::default());
        assert!(radio.ptt_line_available());
    }

    #[test]
    fn a_port_whose_ioctls_fail_reports_the_capability_absent() {
        // This is why availability is a probe and not a type test: this
        // session satisfies `ModemControlLines` in full and has no lines.
        let radio = Ts570d::new(UnwiredSession);
        assert!(!radio.ptt_line_available());
        assert!(radio.set_ptt_line(PttLineKind::Dtr, true).is_err());
    }

    #[test]
    fn a_session_with_no_modem_lines_at_all_keeps_the_defaults() {
        assert!(!SessionlessRadio.ptt_line_available());
        assert!(matches!(
            SessionlessRadio.set_ptt_line(PttLineKind::Dtr, true),
            Err(RadioError::Unsupported)
        ));
        assert!(matches!(
            SessionlessRadio.read_handshake(),
            Err(RadioError::Unsupported)
        ));
    }

    #[test]
    fn the_handle_keys_the_same_port_as_the_radio() {
        // The console's radio task owns the `Ts570d`; the key that keys the
        // transmitter is handled somewhere else entirely.
        let radio = Ts570d::new(WiredSession::default());
        let handle = radio.ptt_line_handle();

        assert!(handle.ptt_line_available());
        handle.set_ptt_line(PttLineKind::Dtr, true).unwrap();
        assert!(radio.shared_session().try_with(|s| s.dtr.get()).unwrap());
    }

    #[test]
    fn a_line_op_during_a_cat_command_says_busy_and_does_not_panic() {
        // The radio task holds the session across `.await` for the whole
        // length of a CAT command. Before this, reaching for the line in
        // that window went through `SharedSession::take`, which `expect`s.
        let radio = Ts570d::new(WiredSession::default());
        let handle = radio.ptt_line_handle();
        let checked_out = radio.shared_session().take();

        assert!(matches!(
            handle.set_ptt_line(PttLineKind::Dtr, false),
            Err(RadioError::Busy)
        ));
        assert!(matches!(handle.read_handshake(), Err(RadioError::Busy)));
        assert!(!handle.ptt_line_available());

        // Put it back, the way the radio task does, and the line moves again.
        radio.shared_session().put_back(checked_out);
        handle.set_ptt_line(PttLineKind::Dtr, false).unwrap();
    }

    #[test]
    fn idle_is_dtr_low_and_rts_high() {
        // Not a style choice: the radio withholds its CAT responses while
        // RTS is low (manual p. 70), so "put everything back to zero" would
        // leave the console unable to talk to the radio.
        assert!(!PttLineKind::Dtr.idle_level());
        assert!(PttLineKind::Rts.idle_level());
    }
}
