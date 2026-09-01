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

//! What this radio looks like to a console.
//!
//! The seam `cat_rigctl::run_with_native` asks for: read the radio's whole
//! state in one go, and apply a command. Both in this radio's own terms,
//! through the typed methods `radio::Ts570d` already has — no wire frames
//! are written here, exactly as the rigctl bridge writes none.
//!
//! A console asks for everything at once, which is why this is a separate
//! seam from `RigctlTs570d`: showing a frequency read at one moment beside
//! a mode read at another describes a radio that never existed.

use cat_native::{Command, MeterKind, MeterSample, ModeId, RadioState};
use cat_rigctl::native_bridge::NativeRadio;
use cat_transport_core::{CatSession, TransportError};
use radio::{Frequency, Mode, Ts570d};

/// This radio, as the console protocol sees it.
pub struct ConsoleTs570d<S: CatSession>(pub Ts570d<S>);

/// The TS-570D's `MD` digit for a mode, or `None` if it has no such mode.
///
/// `capabilities` already refuses a mode this radio lacks, so `None` here
/// is unreachable from the protocol — it exists so this mapping is total
/// rather than a panic waiting for a wider `ModeId`.
fn to_mode(id: ModeId) -> Option<Mode> {
    Some(match id {
        ModeId::Lsb => Mode::Lsb,
        ModeId::Usb => Mode::Usb,
        ModeId::CwUpper => Mode::Cw,
        ModeId::Fm => Mode::Fm,
        ModeId::Am => Mode::Am,
        ModeId::RttyLsb => Mode::Fsk,
        ModeId::CwLower => Mode::CwReverse,
        ModeId::RttyUsb => Mode::FskReverse,
        _ => return None,
    })
}

fn from_mode(mode: Mode) -> ModeId {
    match mode {
        Mode::Lsb => ModeId::Lsb,
        Mode::Usb => ModeId::Usb,
        Mode::Cw => ModeId::CwUpper,
        Mode::Fm => ModeId::Fm,
        Mode::Am => ModeId::Am,
        Mode::Fsk => ModeId::RttyLsb,
        Mode::CwReverse => ModeId::CwLower,
        Mode::FskReverse => ModeId::RttyUsb,
    }
}

#[async_trait::async_trait(?Send)]
impl<S> NativeRadio for ConsoleTs570d<S>
where
    S: CatSession<Error = TransportError>,
{
    async fn state(&mut self) -> Option<RadioState> {
        // One `IF;` carries the dial, the mode, TX, split and the memory
        // channel together. That is the whole reason to prefer it over
        // five separate reads: they would be five different moments.
        let info = self.0.get_information().await.ok()?;

        // The S-meter is its own command, and it is the one field that
        // moves fast enough to be worth a second round trip. A failed read
        // drops the meter rather than the whole state -- a console can
        // draw a dash for one meter, and cannot do anything useful with a
        // frequency it did not get.
        let meters = match self.0.get_smeter().await {
            Ok(raw) => vec![MeterSample {
                kind: MeterKind::S,
                raw,
            }],
            Err(_) => Vec::new(),
        };

        Some(RadioState {
            vfo_a_hz: info.frequency.hz(),
            // The `IF` response carries one frequency: whichever VFO is
            // active. Reporting it as VFO B as well would be inventing a
            // reading, so B is reported as A until there is a real read
            // for it.
            vfo_b_hz: info.frequency.hz(),
            mode: from_mode(info.mode),
            split: info.split,
            transmitting: info.tx_rx,
            memory_channel: Some(u16::from(info.memory_channel)),
            if_shift_hz: None,
            // No CAT-selectable width on this radio, which `capabilities`
            // already declares. Reporting one would contradict it.
            filter_width_hz: None,
            meters,
        })
    }

    async fn apply(&mut self, command: &Command) -> Result<(), String> {
        let result = match command {
            Command::SetFrequency { vfo: 0, hz } | Command::Retune { hz } => {
                match Frequency::new(*hz) {
                    Ok(f) => self.0.set_vfo_a(f).await,
                    Err(e) => return Err(e.to_string()),
                }
            }
            Command::SetFrequency { hz, .. } => match Frequency::new(*hz) {
                Ok(f) => self.0.set_vfo_b(f).await,
                Err(e) => return Err(e.to_string()),
            },
            Command::SetMode { mode } => match to_mode(*mode) {
                Some(m) => self.0.set_mode(m).await,
                None => return Err("this radio has no such mode".to_string()),
            },
            // This radio has no split flag: split IS which VFO transmits.
            // `FT1` puts TX on VFO B, `FT0` returns it to A.
            Command::SetSplit { enabled } => self.0.set_tx_vfo(u8::from(*enabled)).await,
            Command::SetMemoryChannel { channel } => match u8::try_from(*channel) {
                Ok(c) => self.0.set_memory_channel(c).await,
                Err(_) => return Err("memory channel out of range".to_string()),
            },
            // Reads are answered from the published state, never sent.
            Command::ReadMeter { .. } | Command::ReadState => return Ok(()),
            Command::SetIfShift { .. } | Command::SetFilterWidth { .. } => {
                return Err("not wired to CAT on this radio yet".to_string())
            }
        };
        result.map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_this_radio_has_round_trips() {
        // The two mappings are written out separately, so a mode added to
        // one and forgotten in the other is exactly the drift to catch.
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
            assert_eq!(
                to_mode(from_mode(mode)),
                Some(mode),
                "{mode:?} did not survive the round trip"
            );
        }
    }

    #[test]
    fn a_mode_this_radio_lacks_maps_to_nothing_rather_than_to_something_wrong() {
        // C4FM is an FT-991A mode. Silently mapping it onto USB would put
        // the radio in a mode nobody asked for.
        assert_eq!(to_mode(ModeId::C4fm), None);
        assert_eq!(to_mode(ModeId::DataUsb), None);
    }

    #[test]
    fn the_declared_modes_are_exactly_the_ones_this_seam_accepts() {
        // If capabilities offers a console a mode this cannot apply, the
        // console shows a control that fails when used.
        for descriptor in radio::capabilities::TS570D.modes {
            assert!(
                to_mode(descriptor.id).is_some(),
                "{} is offered to consoles but cannot be applied",
                descriptor.label
            );
        }
    }
}
