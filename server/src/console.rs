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

use cat_native::{Command, MeterKind, MeterSample, RadioState};
use cat_rigctl::native_bridge::NativeRadio;
use cat_transport_core::{CatSession, TransportError};
use radio::{Frequency, Ts570d};

/// The meter `RM;`'s selector digit names.
///
/// From the CAT reference's METER SWITCH parameter: `0` no selection,
/// `1` SWR, `2` COMP, `3` ALC. Anything else is a radio reporting
/// something this does not know about, and inventing a meter for it would
/// put a reading on a row it does not belong to.
fn meter_switch(selector: u8) -> Option<MeterKind> {
    match selector {
        1 => Some(MeterKind::Swr),
        2 => Some(MeterKind::Comp),
        3 => Some(MeterKind::Alc),
        _ => None,
    }
}

/// This radio, as the console protocol sees it.
pub struct ConsoleTs570d<S: CatSession>(pub Ts570d<S>);

// The mode mapping lives in `radio` (`capabilities::to_mode` /
// `from_mode`): the console's own native client needs the same one, and a
// radio that disagreed with itself about `CwLower` depending on which end
// of the socket asked would be a bad afternoon.
use radio::capabilities::{from_mode, to_mode};

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
        //
        // **What `SM;` means depends on whether the radio is keyed.** The
        // manual is explicit, both in the operating description -- "While
        // receiving, serves as an S-meter... While transmitting, serves as
        // a calibrated power meter" -- and in the CAT reference, whose
        // note against `SM` reads "In transmit mode: power meter reading".
        //
        // This published every reading as `MeterKind::S`. During a
        // transmission that put a power reading on the S bar with an
        // S-unit scale applied to it: `S9+20` for what was really a power
        // level, on the one meter an operator looks at to decide whether
        // the radio is doing what they asked. `info.tx_rx` is read in the
        // same `IF;` a few lines above, so the reading is labelled with
        // the state it was taken in.
        let kind = if info.tx_rx {
            MeterKind::Po
        } else {
            MeterKind::S
        };
        let mut meters = match self.0.get_smeter().await {
            Ok(raw) => vec![MeterSample { kind, raw }],
            Err(_) => Vec::new(),
        };

        // While keyed, a second meter is answering at the same moment.
        // `RM;` reports whichever of SWR, compression or ALC the operator
        // has selected on the front panel, and the ALC reading is the one
        // that says whether the radio is actually being driven -- the
        // question a whole evening went into answering by other means
        // (troubleshooting-plan.md item 36).
        //
        // Only while transmitting: all three are transmit meters, they
        // read zero the rest of the time, and this is a CAT round trip on
        // a link shared with whatever is keying.
        if info.tx_rx {
            if let Ok((selector, raw)) = self.0.get_meter_reading().await {
                if let Some(kind) = meter_switch(selector) {
                    meters.push(MeterSample { kind, raw });
                }
            }
        }

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
            // Filled in by the pump on its own slower clock, so this read
            // stays one `IF;` plus one `SM;`.
            levels: None,
        })
    }

    /// The fourteen settings the reference rail shows.
    ///
    /// Read here rather than in `state` because they belong on a slower
    /// clock -- fourteen CAT commands at the dial's rate would be most of
    /// a 9600-baud link. `cat_rigctl` calls this every few seconds.
    ///
    /// All or nothing: a partial answer would put a real value beside a
    /// default and there would be no way for a console to tell which was
    /// which. That is precisely the bug this exists to fix -- a network
    /// console showed `AF 200` at a radio reading `AG034`, because the
    /// protocol carried none of these and the console drew its own struct
    /// defaults with total confidence.
    async fn levels(&mut self) -> Option<cat_native::RadioLevels> {
        Some(cat_native::RadioLevels {
            af_gain: self.0.get_af_gain().await.ok()?,
            rf_gain: self.0.get_rf_gain().await.ok()?,
            squelch: self.0.get_squelch().await.ok()?,
            mic_gain: self.0.get_mic_gain().await.ok()?,
            power_pct: self.0.get_power().await.ok()?,
            agc: self.0.get_agc().await.ok()?,
            noise_reduction: self.0.get_noise_reduction().await.ok()?,
            antenna: self.0.get_antenna().await.ok()?,
            noise_blanker: self.0.get_noise_blanker().await.ok()?,
            preamp: self.0.get_preamp().await.ok()?,
            attenuator: self.0.get_attenuator().await.ok()?,
            speech_processor: self.0.get_speech_processor().await.ok()?,
            vox: self.0.get_vox().await.ok()?,
            freq_lock: self.0.get_frequency_lock().await.ok()?,
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
            Command::ReadMeter { .. } | Command::ReadState | Command::ReadDevices => return Ok(()),
            // Never reaches here: `NativeShared::apply` handles an attach
            // against its device directory and does not queue it. Kept
            // explicit rather than swept into a `_` arm, so the next
            // command added to the protocol fails to compile here instead
            // of being silently accepted and ignored.
            Command::AttachDevice { .. } => {
                return Err("a device attach is not a CAT command".to_string())
            }
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

    // The round-trip and unknown-mode tests moved with the mapping into
    // `radio::capabilities`, which is where the two functions now live.
    // What stays here is the question only this seam can ask: whether the
    // modes the capability set *offers a console* are the same ones this
    // adapter can actually apply.

    #[test]
    fn the_meter_switch_digits_are_the_ones_the_manual_gives() {
        // CAT reference, METER SWITCH: 0 no selection, 1 SWR, 2 COMP,
        // 3 ALC. A digit mapped to the wrong meter draws a reading on
        // the wrong row, which is worse than not drawing it -- an SWR
        // figure sitting on the ALC row reads as a radio that is being
        // driven correctly.
        assert_eq!(meter_switch(1), Some(MeterKind::Swr));
        assert_eq!(meter_switch(2), Some(MeterKind::Comp));
        assert_eq!(meter_switch(3), Some(MeterKind::Alc));
    }

    #[test]
    fn an_unselected_or_unknown_meter_is_not_invented() {
        // 0 is "no selection". Anything above 3 is a radio reporting
        // something this does not know about, and guessing would put a
        // reading on a row it does not belong to.
        assert_eq!(meter_switch(0), None);
        for unknown in [4u8, 5, 9, 255] {
            assert_eq!(meter_switch(unknown), None, "{unknown}");
        }
    }

    #[test]
    fn every_meter_a_switch_digit_names_is_declared_by_this_radio() {
        // A label the radio does not declare is a reading with nowhere to
        // go: the rails draw each reading on the row for its own meter,
        // so it would simply not appear.
        for digit in 1..=3u8 {
            let kind = meter_switch(digit).expect("mapped");
            assert!(
                radio::capabilities::TS570D.meters.has(kind),
                "{kind:?} is a switch target but not declared"
            );
        }
    }

    #[test]
    fn this_radio_declares_the_meter_a_transmit_reading_is_labelled_with() {
        // `state` labels a reading taken while keyed as `Po`, because
        // that is what `SM;` answers with then. A console draws each
        // reading on the row for its own meter, so a label the radio does
        // not declare is a reading with nowhere to go -- it would simply
        // not appear, which looks like a meter that stopped working.
        for kind in [MeterKind::S, MeterKind::Po] {
            assert!(
                radio::capabilities::TS570D.meters.has(kind),
                "{kind:?} is used as a label but not declared"
            );
        }
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
