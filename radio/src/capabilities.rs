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
