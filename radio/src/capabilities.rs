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

//! What a TS-570D is, as data.
//!
//! Every number here is a **model fact** — true of any TS-570D on any
//! bench, independent of what is plugged into it. What is plugged in is an
//! `Installation` and lives elsewhere; radio-cat-rs ADR 0015 draws that
//! line, and the CN4 tap below is exactly the case that motivated it.
//!
//! The values were validated as a `#[cfg(test)]` fixture in
//! `cat-framework` while the capability model was being designed (ADR 0010
//! task 13). This is the same data, now where it belongs: a radio
//! describes itself, and `cat-framework` stays free of any radio.
//!
//! Each field cites the code in this crate it agrees with, so a change
//! there that contradicts one here has somewhere to be caught.

use cat_framework::capabilities::*;

// Re-exported because a consumer that reads this radio's declaration has
// to name the type it is matching on, and `radio` is the crate that owns
// the declaration. Without this, every caller would need its own
// dependency on `cat-framework` to ask what `TS570D.signal` is — which the
// dependency rules deliberately do not allow the wiring layer or the
// renderers to have.
pub use cat_framework::capabilities::{EndpointRole, ModeId, SignalSupport};
// The installation model too (ADR 0015): the wiring layer has to say what
// this bench has attached, and it names these types to do it.
pub use cat_framework::installation::{AudioOrigin, Installation, InstalledSource, SourceState};
// `SignalCapability` is `cat-signal`'s, re-exported here so the wiring
// layer names one crate rather than three to describe one bench.
pub use cat_signal::SignalCapability;

/// The single RS-232C port carries CAT **and** keying at once.
///
/// This is the case `shareable_with` exists for: one handle, two roles.
const ENDPOINTS: &[EndpointDescriptor] = &[EndpointDescriptor {
    role: EndpointRole::Cat,
    required: true,
    shareable_with: &[EndpointRole::Keying],
}];

/// The eight modes, in wire order.
///
/// Discriminants 1-7 and 9; 8 is unused on this radio. Mirrors
/// [`crate::radio_trait::Mode`] and its `TryFrom<u8>`, including the labels
/// its `name` returns.
const MODES: &[ModeDescriptor] = &[
    ModeDescriptor {
        id: ModeId::Lsb,
        label: "LSB",
        kind: ModeKind::Ssb,
        sideband: Some(Sideband::Lower),
        default_bandwidth_hz: 2400,
    },
    ModeDescriptor {
        id: ModeId::Usb,
        label: "USB",
        kind: ModeKind::Ssb,
        sideband: Some(Sideband::Upper),
        default_bandwidth_hz: 2400,
    },
    ModeDescriptor {
        id: ModeId::CwUpper,
        label: "CW",
        kind: ModeKind::Cw,
        sideband: Some(Sideband::Upper),
        default_bandwidth_hz: 500,
    },
    ModeDescriptor {
        id: ModeId::Fm,
        label: "FM",
        kind: ModeKind::Fm,
        sideband: None,
        default_bandwidth_hz: 12000,
    },
    ModeDescriptor {
        id: ModeId::Am,
        label: "AM",
        kind: ModeKind::Am,
        sideband: None,
        default_bandwidth_hz: 6000,
    },
    ModeDescriptor {
        id: ModeId::RttyLsb,
        label: "FSK",
        kind: ModeKind::Data,
        sideband: Some(Sideband::Lower),
        default_bandwidth_hz: 500,
    },
    ModeDescriptor {
        id: ModeId::CwLower,
        label: "CW-R",
        kind: ModeKind::Cw,
        sideband: Some(Sideband::Lower),
        default_bandwidth_hz: 500,
    },
    ModeDescriptor {
        id: ModeId::RttyUsb,
        label: "FSK-R",
        kind: ModeKind::Data,
        sideband: Some(Sideband::Upper),
        default_bandwidth_hz: 500,
    },
];

/// Every meter reports over **0-30**, unlike the FT-991A's 0-255.
///
/// The S-meter carries its own S-unit table. Where those boundaries fall
/// is a property of the meter circuit, not a display choice: this radio
/// gives S0 three raw counts and every other unit two, which no clean
/// formula reproduces — an interpolated scale disagrees at 8 of the 31
/// values the meter can report. Publishing it here is what stops a console
/// from having to know.
const METERS: &[MeterDescriptor] = &[
    MeterDescriptor {
        kind: MeterKind::S,
        // **Disputed.** The CAT reference's parameter table gives the
        // `SM` command a range of `0000~0015`, half of this, and adds
        // "Relative values are output" -- so the manual declines to
        // define what the number means as well as disagreeing about how
        // far it goes.
        //
        // Thirty is kept because it is what this radio's console has
        // always drawn and because a controlled measurement supports it:
        // three preamp steps, each sized on the IF tap rather than
        // assumed, put a raw count at 2.95 dB, which is the 3.0 dB a
        // count that a 0-30 scale spanning S0..S9+30 implies. Fifteen
        // would make it 6.0.
        //
        // Against that, the operator's own reading of the front panel --
        // S7 to S9+10 while `SM;` returned 6 to 10 -- works out at 5.5 dB
        // a count, which is the manual's number. One is a measurement
        // against a known step and the other a glance at a bouncing
        // needle mid-FT8-burst, which would settle it if the manual did
        // not agree with the glance.
        //
        // The test is one strong signal: if `SM;` ever answers above 15
        // this is right, and if it pins at 15 while the panel climbs past
        // S9 then this and `SUnitScale::TS570D` both want halving. A
        // sweep of eight broadcast bands on 2026-09-08 reached raw 7, so
        // it is still open. See troubleshooting-plan.md item 48.
        raw_range: RawRange::new(0, 30),
        active_on_transmit: false,
        s_units: Some(SUnitScale::TS570D),
    },
    MeterDescriptor {
        kind: MeterKind::Po,
        raw_range: RawRange::new(0, 30),
        active_on_transmit: true,
        s_units: None,
    },
    MeterDescriptor {
        kind: MeterKind::Swr,
        raw_range: RawRange::new(0, 30),
        active_on_transmit: true,
        s_units: None,
    },
    MeterDescriptor {
        kind: MeterKind::Alc,
        raw_range: RawRange::new(0, 30),
        active_on_transmit: true,
        s_units: None,
    },
];

/// The Kenwood TS-570D.
pub const TS570D: RadioCapabilities = RadioCapabilities {
    model: "Kenwood TS-570D",
    endpoints: EndpointSet::new(ENDPOINTS),
    vfos: VfoCapability {
        count: 2,
        split: true,
        // RIT/XIT offset -9999..+9999 Hz — the IF response layout in
        // `radio_trait.rs`, byte 15.
        rit_hz: Some(9999),
        xit_hz: Some(9999),
    },
    modes: MODES,
    tuning_steps_hz: &[10, 100, 1_000, 5_000, 9_000, 10_000],
    // `Frequency::MIN_HZ` / `MAX_HZ`.
    rx_range: FrequencyRange::new(500_000, 60_000_000),
    filters: FilterCapability {
        // `get_if_shift` returns a direction and an offset. The radio has
        // IF shift, and exposes no selectable width list over CAT — which
        // is why `widths_hz` is None rather than a guess.
        if_shift_hz: Some(1_000),
        widths_hz: None,
        notch: false,
    },
    meters: MeterSet::new(METERS),
    memory: Some(MemoryCapability {
        // "memory channel (00-99)" — the IF layout, byte 24.
        channels: RawRange::new(0, 99),
        named: false,
        stores_mode: true,
        scan: true,
    }),
    menu: Some(MenuCapability {
        // `Ts570dState::menu_values: [u16; 52]`.
        item_count: 52,
        writable: true,
    }),
    // A model fact: every TS-570D has a CN4 header on its TX-RX unit at a
    // 73.05 MHz first IF, spectrum-reversed because LO1 is high-side
    // injection (73.05-103.05 MHz). Whether a dongle is hanging off it is
    // *not* a fact about the model and belongs in an `Installation`.
    signal: SignalSupport::IfTapPoint {
        if_center_hz: 73_050_000,
        inverted: true,
    },
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::radio_trait::{Frequency, Mode};

    #[test]
    fn the_declared_modes_are_the_modes_this_crate_can_parse() {
        // If someone adds a mode to `Mode` and forgets this table, a
        // console driven by capabilities would silently never offer it.
        for code in 1..=9u8 {
            let Ok(mode) = Mode::try_from(code) else {
                continue;
            };
            assert!(
                TS570D.modes.iter().any(|m| m.label == mode.name()),
                "{} is parseable but not declared",
                mode.name()
            );
        }
        assert_eq!(TS570D.modes.len(), 8);
    }

    #[test]
    fn the_declared_coverage_is_the_coverage_this_crate_enforces() {
        assert_eq!(TS570D.rx_range.min_hz, Frequency::MIN_HZ);
        assert_eq!(TS570D.rx_range.max_hz, Frequency::MAX_HZ);
    }

    #[test]
    fn the_s_meter_publishes_the_table_its_console_has_always_drawn() {
        let s = TS570D.meters.find(MeterKind::S).expect("has an S meter");
        let scale = s.s_units.expect("publishes its S-unit table");
        // The four values the shipped table and an interpolated one
        // disagree about at the top of the scale.
        assert_eq!(scale.label(20), "S9");
        assert_eq!(scale.label(24), "S9+10");
        assert_eq!(scale.label(28), "S9+20");
        assert_eq!(scale.label(30), "S9+30");
        // S0 gets three raw counts; every other unit gets two.
        assert_eq!(scale.label(0), "S0");
        assert_eq!(scale.label(2), "S0");
        assert_eq!(scale.label(3), "S1");
    }

    #[test]
    fn the_if_tap_is_declared_as_inverted() {
        // LO1 is high-side, so the tapped spectrum is mirrored. A console
        // that misses this draws every signal on the wrong side of the
        // dial -- and it looks plausible until you tune.
        let SignalSupport::IfTapPoint {
            if_center_hz,
            inverted,
        } = TS570D.signal
        else {
            panic!("the CN4 tap is a model fact");
        };
        assert_eq!(if_center_hz, 73_050_000);
        assert!(inverted);
    }

    #[test]
    fn the_cat_port_is_declared_shareable_with_keying() {
        // One RS-232C handle, two roles. A supervisor that opened a second
        // handle for keying would fail on real hardware.
        let cat = TS570D
            .endpoints
            .endpoints
            .iter()
            .find(|e| e.role == EndpointRole::Cat)
            .expect("has a CAT endpoint");
        assert!(cat.shareable_with.contains(&EndpointRole::Keying));
    }
}

// ---------------------------------------------------------------------------
// Mode identity across the two vocabularies
// ---------------------------------------------------------------------------

/// This radio's mode for a protocol [`ModeId`], or `None` if it has none.
///
/// One mapping, here, because there are now two callers -- the server's
/// console adapter and the console's own native client -- and a radio that
/// disagreed with itself about what `CwLower` means depending on which end
/// of the socket you asked would be a genuinely confusing bug to chase.
///
/// `None` is unreachable from the protocol, since `capabilities` refuses a
/// mode this radio lacks before dispatch. It exists so the mapping is
/// total rather than a panic waiting for a wider `ModeId`.
pub fn to_mode(id: ModeId) -> Option<crate::Mode> {
    use crate::Mode;
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

/// The protocol [`ModeId`] for one of this radio's modes.
pub fn from_mode(mode: crate::Mode) -> ModeId {
    use crate::Mode;
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

#[cfg(test)]
mod mode_mapping_tests {
    use super::*;

    #[test]
    fn every_mode_this_radio_has_survives_a_round_trip() {
        // The property that matters: a mode set over the protocol and read
        // back must be the same mode. A mapping that lost `CwReverse` on
        // the way out would silently put an operator on the wrong sideband.
        for mode in [
            crate::Mode::Lsb,
            crate::Mode::Usb,
            crate::Mode::Cw,
            crate::Mode::Fm,
            crate::Mode::Am,
            crate::Mode::Fsk,
            crate::Mode::CwReverse,
            crate::Mode::FskReverse,
        ] {
            assert_eq!(to_mode(from_mode(mode)), Some(mode), "{mode:?}");
        }
    }

    #[test]
    fn the_two_cw_modes_stay_distinct() {
        // The pair most easily collapsed, and the one where collapsing it
        // puts the operator's injection on the wrong side.
        assert_ne!(
            from_mode(crate::Mode::Cw),
            from_mode(crate::Mode::CwReverse)
        );
        assert_eq!(to_mode(ModeId::CwLower), Some(crate::Mode::CwReverse));
    }

    #[test]
    fn a_mode_this_radio_does_not_have_maps_to_nothing() {
        assert_eq!(to_mode(ModeId::C4fm), None);
        assert_eq!(to_mode(ModeId::DataUsb), None);
    }
}
