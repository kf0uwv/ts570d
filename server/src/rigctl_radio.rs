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

//! The one radio-specific seam `cat_rigctl`'s otherwise fully generic
//! Hamlib rigctld bridge needs: an `impl cat_rigctl::RigctlRadio for
//! RigctlTs570d<S>`, delegating each method to `radio::Ts570d`'s
//! already-correct, already-tested typed methods
//! (`get_vfo_a`/`set_mode`/`transmit`/...) instead of hand-rolling
//! TS-570D wire frames a second time (that logic -- dispatch,
//! `\dump_state`, line framing -- now lives in `cat_rigctl`, shared with
//! `ft991a`).
//!
//! [`RigctlTs570d`] is a thin local newtype around `radio::Ts570d<S>`
//! rather than a direct `impl cat_rigctl::RigctlRadio for radio::Ts570d<S>`
//! -- both `RigctlRadio` (defined in the external `cat_rigctl` crate) and
//! `Ts570d` (defined in the sibling `radio` crate) are foreign to this
//! `server` crate, so a direct impl would violate Rust's orphan rule
//! (E0117: neither the trait nor the type originates in this crate). The
//! newtype makes the *type* local, which is sufficient; `Deref`/`DerefMut`
//! keep every other `Ts570d` method usable unchanged wherever this wrapper
//! is held (only `lib.rs::run`'s `make_radio` closure, today).
//!
//! Mode-name mapping and the frequency range are ported verbatim from this
//! crate's former `rigctl.rs` (deleted as part of this migration) -- these
//! are genuinely TS-570D-specific and were already correct, just relocated
//! here per `cat_rigctl::RigctlRadio`'s trait shape.
//!
//! `get_transmitting` reads the `tx_rx` field of the `IF` query's
//! `InformationResponse` -- `Ts570d` has no dedicated `get_tx_state`-shaped
//! method (unlike e.g. `ft991a`'s radio crate).

use std::ops::{Deref, DerefMut};

use async_trait::async_trait;
use cat_transport_core::{CatSession, TransportError};
use radio::{Frequency, Mode, Ts570d};

/// Local newtype around `radio::Ts570d<S>` -- see this module's doc
/// comment for why it exists (orphan rule).
pub struct RigctlTs570d<S: CatSession>(pub Ts570d<S>);

impl<S: CatSession> Deref for RigctlTs570d<S> {
    type Target = Ts570d<S>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S: CatSession> DerefMut for RigctlTs570d<S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[async_trait(?Send)]
impl<S> cat_rigctl::RigctlRadio for RigctlTs570d<S>
where
    S: CatSession<Error = TransportError>,
{
    type Mode = Mode;
    type Error = radio::RadioError;

    async fn get_vfo_a_hz(&mut self) -> Result<u64, Self::Error> {
        self.0.get_vfo_a().await.map(Frequency::hz)
    }

    async fn set_vfo_a_hz(&mut self, hz: u64) -> Result<(), Self::Error> {
        let freq = Frequency::new(hz)?;
        self.0.set_vfo_a(freq).await
    }

    async fn get_mode(&mut self) -> Result<Self::Mode, Self::Error> {
        self.0.get_mode().await
    }

    async fn set_mode(&mut self, mode: Self::Mode) -> Result<(), Self::Error> {
        self.0.set_mode(mode).await
    }

    async fn get_transmitting(&mut self) -> Result<bool, Self::Error> {
        self.0.get_information().await.map(|info| info.tx_rx)
    }

    async fn transmit(&mut self) -> Result<(), Self::Error> {
        self.0.transmit().await
    }

    async fn receive(&mut self) -> Result<(), Self::Error> {
        self.0.receive().await
    }

    /// Map a [`Mode`] to the Hamlib rig-mode name `m`/`M` exchange on the
    /// wire. TS-570D's 8 modes map 1:1 onto Hamlib names -- no
    /// fallback/best-effort arms are needed (unlike a radio with a
    /// C4FM/data-mode equivalent).
    fn hamlib_mode_name(mode: Self::Mode) -> &'static str {
        match mode {
            Mode::Lsb => "LSB",
            Mode::Usb => "USB",
            Mode::Cw => "CW",
            Mode::Fm => "FM",
            Mode::Am => "AM",
            Mode::Fsk => "RTTY",
            Mode::CwReverse => "CWR",
            Mode::FskReverse => "RTTYR",
        }
    }

    fn hamlib_mode_from_name(name: &str) -> Option<Self::Mode> {
        match name.to_ascii_uppercase().as_str() {
            "LSB" => Some(Mode::Lsb),
            "USB" => Some(Mode::Usb),
            "CW" => Some(Mode::Cw),
            "FM" => Some(Mode::Fm),
            "AM" => Some(Mode::Am),
            "RTTY" => Some(Mode::Fsk),
            "CWR" => Some(Mode::CwReverse),
            "RTTYR" => Some(Mode::FskReverse),
            _ => None,
        }
    }

    /// TS-570D's real documented receive range: 500 kHz-60 MHz.
    fn freq_range_hz() -> (u64, u64) {
        (Frequency::MIN_HZ, Frequency::MAX_HZ)
    }
}

// Gated to Linux: uses #[monoio::test] (see
// docs/adr/0006-windows-concurrency-model.md).
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use cat_rigctl::RigctlRadio;
    use cat_transport_core::test_support::{Exchange, ScriptedCatSession};

    fn radio_with(session: ScriptedCatSession) -> RigctlTs570d<ScriptedCatSession> {
        RigctlTs570d(Ts570d::new(session))
    }

    #[monoio::test(driver = "legacy")]
    async fn get_vfo_a_hz_reports_current_frequency() {
        let session = ScriptedCatSession::with_script(vec![Exchange::new("FA;", "FA00014250000;")]);
        let mut radio = radio_with(session);
        assert_eq!(radio.get_vfo_a_hz().await.unwrap(), 14_250_000);
    }

    #[monoio::test(driver = "legacy")]
    async fn set_vfo_a_hz_sets_frequency() {
        let session = ScriptedCatSession::with_script(vec![Exchange::new("FA00014250000;", "")]);
        let mut radio = radio_with(session);
        radio.set_vfo_a_hz(14_250_000).await.unwrap();
    }

    #[monoio::test(driver = "legacy")]
    async fn set_vfo_a_hz_rejects_out_of_range_frequency() {
        // Above Frequency::MAX_HZ (60 MHz) -- Frequency::new() itself
        // rejects it, never reaching the radio.
        let mut radio = radio_with(ScriptedCatSession::new());
        assert!(radio.set_vfo_a_hz(70_000_000).await.is_err());
    }

    #[monoio::test(driver = "legacy")]
    async fn get_mode_reports_mode() {
        let session = ScriptedCatSession::with_script(vec![Exchange::new("MD;", "MD2;")]);
        let mut radio = radio_with(session);
        // Unambiguously resolves to the `RigctlRadio` impl (in scope via
        // `use cat_rigctl::RigctlRadio` above) -- `RigctlTs570d` itself has
        // no inherent `get_mode`, only the wrapped `Ts570d` does, so method
        // lookup finds the trait method at the receiver's own type before
        // ever autoderef-ing to `Ts570d`'s inherent one.
        assert_eq!(radio.get_mode().await.unwrap(), Mode::Usb);
    }

    #[monoio::test(driver = "legacy")]
    async fn set_mode_sets_mode() {
        let session = ScriptedCatSession::with_script(vec![Exchange::new("MD2;", "")]);
        let mut radio = radio_with(session);
        cat_rigctl::RigctlRadio::set_mode(&mut radio, Mode::Usb)
            .await
            .unwrap();
    }

    #[monoio::test(driver = "legacy")]
    async fn get_transmitting_reports_ptt_off() {
        // IF response layout, ground-truthed against
        // radio/src/ts570d.rs's own test_get_information_frequency_parsed
        // (34-char payload: freq(11) step(5) rit/xit(5) rit(1) xit(1)
        // bank(1) channel(2) tx_rx(1) mode(1) vfo(1) scan(1) split(1)
        // ctcss(2) tone(1)) -- tx_rx digit at payload offset 26 is '0'.
        let session = ScriptedCatSession::with_script(vec![Exchange::new(
            "IF;",
            "IF0001423000001000+00000000102000000;",
        )]);
        let mut radio = radio_with(session);
        assert!(!radio.get_transmitting().await.unwrap());
    }

    #[monoio::test(driver = "legacy")]
    async fn get_transmitting_reports_ptt_on() {
        // Same layout, tx_rx digit at payload offset 26 changed to '1'.
        let session = ScriptedCatSession::with_script(vec![Exchange::new(
            "IF;",
            "IF0001423000001000+00000000112000000;",
        )]);
        let mut radio = radio_with(session);
        assert!(radio.get_transmitting().await.unwrap());
    }

    #[monoio::test(driver = "legacy")]
    async fn transmit_sends_tx() {
        let session = ScriptedCatSession::with_script(vec![Exchange::new("TX;", "")]);
        let mut radio = radio_with(session);
        cat_rigctl::RigctlRadio::transmit(&mut radio).await.unwrap();
    }

    #[monoio::test(driver = "legacy")]
    async fn receive_sends_rx() {
        let session = ScriptedCatSession::with_script(vec![Exchange::new("RX;", "")]);
        let mut radio = radio_with(session);
        cat_rigctl::RigctlRadio::receive(&mut radio).await.unwrap();
    }

    #[test]
    fn hamlib_mode_round_trips_for_every_supported_mode() {
        for mode in [
            Mode::Lsb,
            Mode::Usb,
            Mode::Cw,
            Mode::Fm,
            Mode::Am,
            Mode::Fsk,
            Mode::CwReverse,
            Mode::FskReverse,
        ] {
            let name =
                <RigctlTs570d<ScriptedCatSession> as cat_rigctl::RigctlRadio>::hamlib_mode_name(
                    mode,
                );
            assert_eq!(
                <RigctlTs570d<ScriptedCatSession> as cat_rigctl::RigctlRadio>::hamlib_mode_from_name(
                    name
                ),
                Some(mode),
                "mode {mode:?} -> {name} did not round-trip"
            );
        }
    }

    #[test]
    fn freq_range_hz_matches_frequency_constants() {
        assert_eq!(
            <RigctlTs570d<ScriptedCatSession> as cat_rigctl::RigctlRadio>::freq_range_hz(),
            (Frequency::MIN_HZ, Frequency::MAX_HZ)
        );
    }
}
