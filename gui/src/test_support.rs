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

//! Capability sets to render against, without a radio or a socket.
//!
//! These are `CapabilitiesWire` — the shape that arrives from a server —
//! rather than the `radio` crate's declaration, because that is what this
//! crate can see. `gui` never depends on `radio`: it is network-only, and
//! the radio on the other end might not be a TS-570D at all.
//!
//! `caps_bare` is the more useful of the two. A console tested only
//! against a well-equipped radio quietly grows assumptions that it has a
//! memory, a menu, a spectrum source — and then draws a control that can
//! never work the first time somebody points it at something simpler.

#![cfg(test)]

use cat_native::{
    CapabilitiesWire, FilterWire, FrequencyRange, Installation, MemoryCapability, MenuCapability,
    MeterDescriptorWire, MeterKind, ModeId, ModeKind, ModeWire, RawRange, SUnitScale, Sideband,
    SignalSupport, VfoCapability,
};

fn mode(id: ModeId, label: &str, kind: ModeKind, sideband: Option<Sideband>, bw: u32) -> ModeWire {
    ModeWire {
        id,
        label: label.to_string(),
        kind,
        sideband,
        default_bandwidth_hz: bw,
    }
}

/// A TS-570D as a server describes it.
pub fn caps_ts570d() -> CapabilitiesWire {
    CapabilitiesWire {
        model: "Kenwood TS-570D".to_string(),
        endpoints: Vec::new(),
        vfos: VfoCapability {
            count: 2,
            split: true,
            rit_hz: Some(9999),
            xit_hz: Some(9999),
        },
        modes: vec![
            mode(
                ModeId::Lsb,
                "LSB",
                ModeKind::Ssb,
                Some(Sideband::Lower),
                2400,
            ),
            mode(
                ModeId::Usb,
                "USB",
                ModeKind::Ssb,
                Some(Sideband::Upper),
                2400,
            ),
            mode(
                ModeId::CwUpper,
                "CW",
                ModeKind::Cw,
                Some(Sideband::Upper),
                500,
            ),
            mode(ModeId::Fm, "FM", ModeKind::Fm, None, 12000),
            mode(ModeId::Am, "AM", ModeKind::Am, None, 6000),
        ],
        tuning_steps_hz: vec![10, 100, 1_000, 5_000, 9_000, 10_000],
        rx_range: FrequencyRange::new(500_000, 60_000_000),
        filters: FilterWire {
            if_shift_hz: Some(1_000),
            // No CAT-selectable widths. This is why the quick bar shows no
            // FILTER control for this radio, and it is a real property
            // rather than a gap in the fixture.
            widths_hz: None,
            notch: false,
        },
        meters: vec![MeterDescriptorWire {
            kind: MeterKind::S,
            raw_range: RawRange::new(0, 30),
            active_on_transmit: false,
            s_units: Some(SUnitScale::TS570D),
        }],
        memory: Some(MemoryCapability {
            channels: RawRange::new(0, 99),
            named: false,
            stores_mode: true,
            scan: true,
        }),
        menu: Some(MenuCapability {
            item_count: 52,
            writable: true,
        }),
        signal: SignalSupport::IfTapPoint {
            if_center_hz: 73_050_000,
            inverted: true,
        },
        installation: Installation::default(),
    }
}

/// A radio with nothing but a dial.
///
/// No memory, no menu, no spectrum, no shift, no split. Every panel has to
/// survive this, and the console has to say what is missing rather than
/// offer a control that cannot work.
pub fn caps_bare() -> CapabilitiesWire {
    CapabilitiesWire {
        model: "Bare Radio".to_string(),
        endpoints: Vec::new(),
        vfos: VfoCapability {
            count: 1,
            split: false,
            rit_hz: None,
            xit_hz: None,
        },
        modes: Vec::new(),
        tuning_steps_hz: Vec::new(),
        rx_range: FrequencyRange::new(1_800_000, 30_000_000),
        filters: FilterWire {
            if_shift_hz: None,
            widths_hz: None,
            notch: false,
        },
        meters: Vec::new(),
        memory: None,
        menu: None,
        signal: SignalSupport::None,
        installation: Installation::default(),
    }
}
