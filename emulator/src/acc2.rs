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

//! The ACC2 connector, as the radio presents it.
//!
//! This is the DIN-13 socket an ACC2-IF plugs into — the radio's side of
//! the ACC2-IF datasheet (Rev A, 2026-08-31, §6). The emulator's job is to
//! be a radio, so what is modelled here is what the *radio* does with each
//! pin, not what the interface box does with it.
//!
//! # The pin map is data, and that is the point
//!
//! [`ACC2`] carries all thirteen pins, including the ones nothing is wired
//! to and the one that has been physically cut. A connector modelled as
//! "the four signals we use" cannot represent service note SN-1 (a cable
//! that bonded pin 13 to the braid and keyed the radio on plug-in) or SN-2
//! (every wire landed one mirror position away, so the box measured
//! perfect and keyed nothing). Both are things that actually happened to
//! this station, and both are only expressible if the pins that are
//! *supposed* to do nothing are present and doing nothing on purpose.
//!
//! # Keying goes through the command table, not around it
//!
//! When PKS is pulled to ground the emulator raises TX by feeding `TX;`
//! through the same [`radio::TS570D_COMMAND_TABLE`] a CAT client would
//! reach it by, rather than by writing the `tx` flag directly. Two reasons.
//! A radio that is off, or otherwise not in a state that accepts transmit,
//! does not key just because a pin went low — going through the state
//! machine gets that for free. And a console must not be able to tell how
//! the radio was keyed, which is only true if both routes end in the same
//! place.
//!
//! What the two routes do *not* share is the microphone, and that
//! difference is the whole reason the TS-570D has two PTT pins:
//!
//! | | pin | mic |
//! |---|---|---|
//! | PKS | 9 | **muted** — the data path, what an interface should use |
//! | SS | 13 | **live** — parallel with the mic jack's own PTT |

use radio::ts570d_radio::Ts570dState;

/// Whether the radio has anything wired to a pin, and if not, why not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wiring {
    /// The ACC2-IF lands a wire here.
    Wired,
    /// No internal connection on the radio side.
    NotConnected,
    /// A signal the radio provides that this station deliberately does not
    /// use.
    Unused,
    /// Physically cut on this station's cable. See SN-1.
    Cut,
}

/// One pin of the DIN-13.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Acc2Pin {
    pub number: u8,
    /// Kenwood's name for the signal (instruction manual p. 62).
    pub name: &'static str,
    pub function: &'static str,
    pub wiring: Wiring,
}

/// The ACC2 socket, pin 1 through pin 13, in **solder-side / rear-panel**
/// numbering — the view the datasheet's Fig. 3 uses and the only one its
/// wiring table is written against.
///
/// Names and functions are the datasheet's §6 table, which is itself the
/// Kenwood instruction manual p. 62.
pub const ACC2: [Acc2Pin; 13] = [
    Acc2Pin {
        number: 1,
        name: "NC",
        function: "no connection",
        wiring: Wiring::NotConnected,
    },
    Acc2Pin {
        number: 2,
        name: "RTK",
        function: "RTTY key in (not a CW key)",
        wiring: Wiring::Unused,
    },
    Acc2Pin {
        number: 3,
        name: "ANO",
        function: "RX AF out, fixed level (Menu 34)",
        wiring: Wiring::Wired,
    },
    Acc2Pin {
        number: 4,
        name: "GND",
        function: "shield for pin 3",
        wiring: Wiring::Wired,
    },
    Acc2Pin {
        number: 5,
        name: "PSQ",
        function: "squelch status out",
        wiring: Wiring::Unused,
    },
    Acc2Pin {
        number: 6,
        name: "SMET",
        function: "S-meter out (>=1 MOhm load)",
        wiring: Wiring::Unused,
    },
    Acc2Pin {
        number: 7,
        name: "NC",
        function: "no connection",
        wiring: Wiring::NotConnected,
    },
    Acc2Pin {
        number: 8,
        name: "GND",
        function: "chassis ground",
        wiring: Wiring::Wired,
    },
    Acc2Pin {
        number: 9,
        name: "PKS",
        function: "PTT in - ground to transmit, mic muted",
        wiring: Wiring::Wired,
    },
    Acc2Pin {
        number: 10,
        name: "NC",
        function: "no connection",
        wiring: Wiring::NotConnected,
    },
    Acc2Pin {
        number: 11,
        name: "PKD",
        function: "mic/data audio in",
        wiring: Wiring::Wired,
    },
    Acc2Pin {
        number: 12,
        name: "GND",
        function: "shield for pin 11",
        wiring: Wiring::Wired,
    },
    Acc2Pin {
        number: 13,
        name: "SS",
        function: "PTT in, parallel with the mic jack - mic stays live",
        wiring: Wiring::Cut,
    },
];

/// Look up a pin by its solder-side number.
pub fn pin(number: u8) -> Option<&'static Acc2Pin> {
    ACC2.iter().find(|p| p.number == number)
}

/// The same physical contact's number when read from the **mating face**
/// instead of the solder side.
///
/// SN-2, "the mirror trap": the two views are mirrored left-to-right, so
/// 9↔12, 8↔5, 11↔10, 3↔2 and 4↔1 swap, while 6, 7 and 13 sit on the
/// mirror line and keep their numbers. A board wired from the wrong view
/// measures perfect and keys nothing — the PKS wire lands on a ground, the
/// opto emitter lands on PSQ, and the TX audio lands on an NC pin.
///
/// This exists as a function so a test can state the trap rather than a
/// comment merely warning about it.
pub fn mating_face_number(solder_side: u8) -> u8 {
    match solder_side {
        9 => 12,
        12 => 9,
        8 => 5,
        5 => 8,
        11 => 10,
        10 => 11,
        3 => 2,
        2 => 3,
        4 => 1,
        1 => 4,
        // On the mirror line.
        other => other,
    }
}

/// How the transmitter came to be keyed.
///
/// The distinction is not bookkeeping: it decides whether the microphone
/// is live, which is the difference between a data transmission and one
/// that also puts the shack on the air.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    /// `TX;` over CAT.
    Cat,
    /// Pin 9 pulled to ground — the ACC2-IF's opto. Mic muted.
    Pks,
    /// Pin 13 pulled to ground. Mic live. Only reachable on this station
    /// via the SN-1 fault, because the pin is cut.
    Ss,
}

/// Faults the real station has hit, reproducible on demand.
///
/// All off by default: a virtual radio that misbehaved out of the box
/// would be a worse radio, not a more honest one. They exist so the
/// software that is supposed to survive them can be tested against them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Acc2Faults {
    /// SN-1. A cheap DIN lead with pin 13 bonded to the shield, so seating
    /// the plug grounds SS and keys the radio immediately — with the mic
    /// live. The datasheet's new-cable rule exists because of this.
    pub pin13_bonded_to_braid: bool,
    /// SN-3. Radio COM CTS back-feeds an unpowered adapter, DTR floats
    /// positive, and PKS is keyed *marginally* — which presents as TX/RX
    /// chatter rather than as a clean key-down.
    pub phantom_keying: bool,
}

/// What the radio should do about a change on the connector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keying {
    /// Raise TX, keyed by this source.
    Transmit(KeySource),
    /// Drop back to receive.
    Receive,
}

/// The ACC2 socket's live state.
#[derive(Debug, Clone, Default)]
pub struct Acc2 {
    /// Pin 9 held to ground.
    pks_grounded: bool,
    /// Pin 13 held to ground. Only ever true under [`Acc2Faults`].
    ss_grounded: bool,
    /// Whether a plug is in the socket at all.
    seated: bool,
    /// Set while [`Acc2Faults::phantom_keying`] is producing chatter, so
    /// the emulator can toggle rather than key cleanly.
    phantom_high: bool,
    faults: Acc2Faults,
}

impl Acc2 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn faults(&self) -> Acc2Faults {
        self.faults
    }

    pub fn set_faults(&mut self, faults: Acc2Faults) {
        self.faults = faults;
    }

    /// Whether a plug is seated in the socket.
    pub fn seated(&self) -> bool {
        self.seated
    }

    /// Pin 9's state: `true` while it is held to ground, i.e. keyed.
    pub fn pks_grounded(&self) -> bool {
        self.pks_grounded
    }

    /// Pin 13's state. `true` only under SN-1.
    pub fn ss_grounded(&self) -> bool {
        self.ss_grounded
    }

    /// Whichever source currently has the radio keyed through this
    /// connector, if any. CAT keying is not visible here — it does not
    /// come through ACC2.
    pub fn key_source(&self) -> Option<KeySource> {
        // SS wins when both are down: it is the one with the mic live, and
        // reporting the quieter of two simultaneous faults would describe
        // a safer radio than the one on the bench.
        if self.ss_grounded {
            Some(KeySource::Ss)
        } else if self.pks_grounded {
            Some(KeySource::Pks)
        } else {
            None
        }
    }

    /// Whether the microphone is muted, given how the radio is keyed.
    ///
    /// Datasheet §6: PKS mutes the mic, SS leaves it live.
    pub fn mic_muted(&self) -> bool {
        matches!(self.key_source(), Some(KeySource::Pks))
    }

    /// Drive pin 9 — what the ACC2-IF's opto does when DTR is asserted.
    ///
    /// Returns the keying change to apply, or `None` if nothing changed.
    pub fn set_pks(&mut self, grounded: bool) -> Option<Keying> {
        if self.pks_grounded == grounded {
            return None;
        }
        self.pks_grounded = grounded;
        Some(self.keying())
    }

    /// Seat or unseat a plug in the socket.
    ///
    /// Under SN-1 this is where the damage happens: pin 13 is bonded to the
    /// braid inside the cable, so the act of plugging in grounds SS.
    pub fn seat(&mut self, seated: bool) -> Option<Keying> {
        self.seated = seated;
        let ss = seated && self.faults.pin13_bonded_to_braid;
        if self.ss_grounded == ss {
            return None;
        }
        self.ss_grounded = ss;
        Some(self.keying())
    }

    /// Advance the SN-3 phantom-keying fault by one step.
    ///
    /// Marginal keying is not a steady state and modelling it as one would
    /// miss the symptom entirely: what the operator sees is TX/RX chatter,
    /// so what this produces is TX/RX chatter.
    pub fn tick_phantom(&mut self) -> Option<Keying> {
        if !self.faults.phantom_keying {
            // Leaving the fault behind must not leave the key stuck down.
            if self.phantom_high {
                self.phantom_high = false;
                return self.set_pks(false);
            }
            return None;
        }
        self.phantom_high = !self.phantom_high;
        self.set_pks(self.phantom_high)
    }

    /// The keying state implied by the pins as they now stand.
    fn keying(&self) -> Keying {
        match self.key_source() {
            Some(source) => Keying::Transmit(source),
            None => Keying::Receive,
        }
    }
}

// ── Analogue outputs ────────────────────────────────────────────────────
//
// Pins 3, 5 and 6 are outputs the radio drives and this station does not
// wire. They are modelled anyway: "the radio provides this and we chose not
// to use it" is a different statement from "this does not exist", and a
// console or a future interface that did use them should find them here
// already behaving.

/// Full-scale output on the analogue pins, in volts.
///
/// The TS-570D's ACC2 analogue outputs swing over roughly 0-5 V into a high
/// impedance; the datasheet's only electrical statement about them is that
/// SMET wants at least a 1 MΩ load.
pub const ANALOGUE_FULL_SCALE_V: f32 = 5.0;

/// The raw S-meter value that corresponds to full scale.
///
/// `radio::capabilities::TS570D` publishes `RawRange::new(0, 30)` for every
/// meter on this radio, and `SM` reports that same raw number. Taking the
/// figure from one place keeps the pin and the CAT command describing the
/// same meter.
const SMETER_FULL_SCALE: f32 = 30.0;

/// Pin 6, SMET: the S-meter as a voltage.
pub fn smet_volts(state: &Ts570dState) -> f32 {
    let fraction = (f32::from(state.smeter) / SMETER_FULL_SCALE).clamp(0.0, 1.0);
    fraction * ANALOGUE_FULL_SCALE_V
}

/// Pin 5, PSQ: squelch status.
///
/// `true` means the squelch is **open** — there is audio. The pin itself is
/// active-low on the real radio (it pulls to ground when the squelch
/// opens); this returns the logical state rather than the electrical one,
/// because every consumer wants "is there audio" and none of them wants to
/// remember the inversion.
pub fn psq_open(state: &Ts570dState) -> bool {
    // The squelch control is 0-255 (`SQ` takes three digits) and the meter
    // is 0-30, so the comparison has to be made in one common scale rather
    // than between the two raw numbers.
    let threshold = f32::from(state.squelch) / 255.0;
    let signal = (f32::from(state.smeter) / SMETER_FULL_SCALE).clamp(0.0, 1.0);
    signal >= threshold
}

/// Menu 34 selects the fixed level ANO puts out.
pub const MENU_ANO_LEVEL: usize = 34;

/// The menu value [`ano_level`] treats as full scale.
///
/// **Assumed, not verified.** `EX` carries four digits, so the wire permits
/// 0-9999; this takes Menu 34's own range to be a single digit, as the
/// TS-570D's level menus are. The instruction manual is the authority and
/// this repository does not carry it (`docs/*.pdf` is gitignored, see
/// `docs/README`). Correct this against the manual before trusting an
/// absolute level; nothing here depends on the number beyond scaling.
pub const MENU_ANO_FULL_SCALE: f32 = 9.0;

/// Pin 3, ANO: the receive audio output level, as a fraction of full scale.
///
/// **Fixed**, in the sense the datasheet means: it is set by Menu 34 and
/// does **not** follow the front-panel AF gain. That is exactly why an
/// interface takes its receive audio from here rather than from the
/// headphone jack — turning the volume down must not change what the
/// decoder hears.
pub fn ano_level(state: &Ts570dState) -> f32 {
    let raw = state.menu_values[MENU_ANO_LEVEL];
    // Menu values are four digits on the wire; this one is a level, so it
    // is read as a fraction of its own full scale rather than as volts.
    (f32::from(raw) / MENU_ANO_FULL_SCALE).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Ts570dState {
        Ts570dState::default()
    }

    #[test]
    fn every_pin_of_the_din_is_present() {
        // A connector modelled as only its wired pins cannot express SN-1
        // or SN-2 at all. See the module doc.
        assert_eq!(ACC2.len(), 13);
        for (index, p) in ACC2.iter().enumerate() {
            assert_eq!(p.number as usize, index + 1, "pins must be in order");
        }
    }

    #[test]
    fn pin_13_is_cut_not_merely_unused() {
        // SN-1. "Unused" would still be a pin that could be driven; this
        // one has been physically removed and must never key.
        assert_eq!(pin(13).unwrap().wiring, Wiring::Cut);
        assert_eq!(pin(13).unwrap().name, "SS");
    }

    #[test]
    fn the_datasheets_wired_pins_are_the_wired_pins() {
        // Bold in datasheet §6: 3 ANO, 4 GND, 8 GND, 9 PKS, 11 PKD, 12 GND.
        let wired: Vec<u8> = ACC2
            .iter()
            .filter(|p| p.wiring == Wiring::Wired)
            .map(|p| p.number)
            .collect();
        assert_eq!(wired, vec![3, 4, 8, 9, 11, 12]);
    }

    #[test]
    fn the_mirror_swaps_exactly_the_pairs_sn2_names() {
        // SN-2 lists them: 9<->12, 8<->5, 11<->10, 3<->2, 4<->1.
        for (a, b) in [(9, 12), (8, 5), (11, 10), (3, 2), (4, 1)] {
            assert_eq!(mating_face_number(a), b, "pin {a} mirrors to {b}");
            assert_eq!(mating_face_number(b), a, "the mirror is symmetric");
        }
    }

    #[test]
    fn the_mirror_trap_lands_pks_on_a_ground() {
        // The specific failure SN-2 describes: wired from the mating face,
        // the PKS wire goes to pin 12, which is a ground. Perfect
        // measurements, no keying.
        let landed = pin(mating_face_number(9)).unwrap();
        assert_eq!(landed.name, "GND");
    }

    #[test]
    fn pins_on_the_mirror_line_keep_their_number() {
        for p in [6, 7, 13] {
            assert_eq!(mating_face_number(p), p);
        }
    }

    #[test]
    fn grounding_pks_transmits_with_the_mic_muted() {
        let mut acc2 = Acc2::new();
        assert_eq!(acc2.set_pks(true), Some(Keying::Transmit(KeySource::Pks)));
        assert!(
            acc2.mic_muted(),
            "PKS is the data path: the mic must be muted"
        );
        assert_eq!(acc2.set_pks(false), Some(Keying::Receive));
        assert!(!acc2.mic_muted());
    }

    #[test]
    fn driving_pks_to_the_state_it_is_already_in_is_not_a_change() {
        let mut acc2 = Acc2::new();
        acc2.set_pks(true);
        assert_eq!(acc2.set_pks(true), None);
    }

    #[test]
    fn a_healthy_cable_does_not_key_when_it_seats() {
        let mut acc2 = Acc2::new();
        assert_eq!(acc2.seat(true), None);
        assert!(acc2.seated());
        assert_eq!(acc2.key_source(), None);
    }

    #[test]
    fn sn1_keys_on_plug_in_with_the_mic_live() {
        // The fault the pin was cut for. Seating the plug is enough.
        let mut acc2 = Acc2::new();
        acc2.set_faults(Acc2Faults {
            pin13_bonded_to_braid: true,
            ..Acc2Faults::default()
        });

        assert_eq!(acc2.seat(true), Some(Keying::Transmit(KeySource::Ss)));
        assert!(
            !acc2.mic_muted(),
            "SS is parallel with the mic jack -- this is the dangerous one"
        );

        assert_eq!(acc2.seat(false), Some(Keying::Receive));
    }

    #[test]
    fn ss_outranks_pks_when_both_are_down() {
        let mut acc2 = Acc2::new();
        acc2.set_faults(Acc2Faults {
            pin13_bonded_to_braid: true,
            ..Acc2Faults::default()
        });
        acc2.seat(true);
        acc2.set_pks(true);
        assert_eq!(acc2.key_source(), Some(KeySource::Ss));
        assert!(
            !acc2.mic_muted(),
            "the mic is live if either route leaves it live"
        );
    }

    #[test]
    fn sn3_chatters_rather_than_keying_cleanly() {
        // Marginal keying presents as TX/RX chatter. A fault modelled as a
        // steady key-down would not reproduce the symptom that sent the
        // operator looking.
        let mut acc2 = Acc2::new();
        acc2.set_faults(Acc2Faults {
            phantom_keying: true,
            ..Acc2Faults::default()
        });

        assert_eq!(acc2.tick_phantom(), Some(Keying::Transmit(KeySource::Pks)));
        assert_eq!(acc2.tick_phantom(), Some(Keying::Receive));
        assert_eq!(acc2.tick_phantom(), Some(Keying::Transmit(KeySource::Pks)));
    }

    #[test]
    fn clearing_sn3_mid_chatter_does_not_leave_the_key_down() {
        let mut acc2 = Acc2::new();
        acc2.set_faults(Acc2Faults {
            phantom_keying: true,
            ..Acc2Faults::default()
        });
        assert_eq!(acc2.tick_phantom(), Some(Keying::Transmit(KeySource::Pks)));

        acc2.set_faults(Acc2Faults::default());
        assert_eq!(
            acc2.tick_phantom(),
            Some(Keying::Receive),
            "a transmitter left keyed by a cleared fault is worse than the fault"
        );
        assert_eq!(acc2.tick_phantom(), None);
    }

    #[test]
    fn smet_tracks_the_same_raw_meter_the_sm_command_reports() {
        let mut s = state();
        s.smeter = 0;
        assert_eq!(smet_volts(&s), 0.0);
        s.smeter = 30;
        assert_eq!(smet_volts(&s), ANALOGUE_FULL_SCALE_V);
        s.smeter = 15;
        assert!((smet_volts(&s) - 2.5).abs() < 1e-6);
    }

    #[test]
    fn smet_does_not_run_off_the_end_of_the_scale() {
        // `smeter` is a u16 and nothing stops a state from carrying a
        // larger number than the meter's range.
        let mut s = state();
        s.smeter = 9999;
        assert_eq!(smet_volts(&s), ANALOGUE_FULL_SCALE_V);
    }

    #[test]
    fn psq_opens_when_the_signal_passes_the_squelch() {
        let mut s = state();
        s.squelch = 0;
        s.smeter = 0;
        assert!(psq_open(&s), "squelch fully open passes even a dead band");

        s.squelch = 255;
        assert!(
            !psq_open(&s),
            "squelch fully closed holds against a dead band"
        );

        s.smeter = 30;
        assert!(
            psq_open(&s),
            "a full-scale signal opens a fully closed squelch"
        );
    }

    #[test]
    fn ano_follows_menu_34_and_nothing_else() {
        // The property that makes ANO the right tap for a decoder: the
        // front-panel volume control must not change it.
        let mut s = state();
        s.menu_values[MENU_ANO_LEVEL] = 0;
        assert_eq!(ano_level(&s), 0.0);
        s.menu_values[MENU_ANO_LEVEL] = 9;
        assert_eq!(ano_level(&s), 1.0);

        let quiet = ano_level(&s);
        s.af_gain = 0;
        assert_eq!(
            ano_level(&s),
            quiet,
            "ANO is fixed: turning the AF gain down must not change it"
        );
    }
}
