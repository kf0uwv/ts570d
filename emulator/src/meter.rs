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

//! An S-meter that agrees with the band the emulator is transmitting.
//!
//! # Why this exists
//!
//! The emulator served a synthetic band on the IF tap and answered `SM`
//! with the constant 10. So a console could show a strong carrier filling
//! its waterfall while the S-meter sat at S5, and nothing anywhere would
//! notice — which makes the emulator useless for the one thing a console
//! most needs testing: that its instruments agree with each other.
//!
//! A real radio's S-meter reads what is inside its passband. So does this
//! one, from the same [`Band`] the tap and the ACC2 audio are rendering.
//! Tune onto a signal and the needle comes up; tune off it and the needle
//! falls, because it is the same signal.
//!
//! # The mapping is this radio's, not a formula
//!
//! S9 is −73 dBm by convention and each unit below it is 6 dB, but where
//! those land on a *raw* meter reading is a property of the meter circuit.
//! The TS-570D's table gives S0 three raw counts and every other unit two,
//! which no clean formula reproduces. So this inverts that radio's own
//! published scale rather than inventing one, and a test walks every
//! S-unit to keep the two from drifting.

use cat_signal::synthetic::Band;

/// The receive passband the meter integrates over.
///
/// An S-meter reads what got through the filter, so a signal 5 kHz away
/// does not move it. 3 kHz is the SSB passband this radio's capability set
/// declares; using it here is what makes tuning *onto* a signal the thing
/// that raises the needle.
pub const PASSBAND_HZ: u32 = 3_000;

/// What −73 dBm is: S9, by convention on HF.
const S9_DBM: f32 = -73.0;

/// Decibels per S-unit below S9.
const DB_PER_S_UNIT: f32 = 6.0;

/// Decibels per division above S9, where the scale is marked +10, +20, +30.
const DB_PER_OVER: f32 = 10.0;

/// This radio's raw reading for S9.
///
/// From `SUnitScale::TS570D`'s own table: raw 19–20 is S9. The top of the
/// band is taken, so a reading that maps exactly to S9 labels as S9 rather
/// than falling into S8.
const RAW_S9: f32 = 20.0;

/// Raw counts per S-unit below S9, from the same table.
const RAW_PER_S_UNIT: f32 = 2.0;

/// Raw counts per +10 dB division above S9: 20→24→28.
const RAW_PER_OVER: f32 = 4.0;

/// The top of this radio's meter.
const RAW_MAX: f32 = 30.0;

/// The strongest thing in the passband at `dial_hz`, in dBm.
///
/// Peak rather than mean: a meter reads the signal an operator is
/// listening to, and averaging it with the silence either side would make
/// a strong carrier look like a weak one.
pub fn passband_peak_dbm(band: &Band, dial_hz: u64, t: f64) -> f32 {
    // Nine bins across the passband. Enough that a narrow carrier lands
    // inside one rather than between two, few enough to be free.
    let bins = band.render(dial_hz, PASSBAND_HZ, 9, t);
    bins.into_iter().fold(f32::NEG_INFINITY, f32::max)
}

/// This radio's raw S-meter reading for a signal of `dbm`.
///
/// Inverts `SUnitScale::TS570D`. Below S1 the meter rests at zero, as a
/// real one does — there is no negative S.
pub fn raw_for_dbm(dbm: f32) -> u16 {
    // NaN reads zero; an infinity pegs. Folding both to zero -- which
    // this did -- would have an impossibly strong signal read as a dead
    // band, which is the wrong way round for anything that might be a
    // fault.
    if dbm.is_nan() {
        return 0;
    }
    if dbm == f32::INFINITY {
        return RAW_MAX as u16;
    }
    if dbm == f32::NEG_INFINITY {
        return 0;
    }
    let raw = if dbm <= S9_DBM {
        // Below S9: 6 dB per unit, 2 raw counts per unit.
        RAW_S9 - (S9_DBM - dbm) / DB_PER_S_UNIT * RAW_PER_S_UNIT
    } else {
        // Above S9: 10 dB per division, 4 raw counts per division.
        RAW_S9 + (dbm - S9_DBM) / DB_PER_OVER * RAW_PER_OVER
    };
    raw.clamp(0.0, RAW_MAX).round() as u16
}

/// The reading this radio should show, tuned to `dial_hz`.
pub fn reading(band: &Band, dial_hz: u64, t: f64) -> u16 {
    raw_for_dbm(passband_peak_dbm(band, dial_hz, t))
}

/// Keep a radio's S-meter agreeing with the band it is transmitting.
///
/// Its own thread, and running whether or not anything is connected to the
/// IF tap: a radio's meter is a property of the radio, not of somebody
/// watching its IF output. Putting this in the tap's client loop — which
/// is where it went first — meant the needle only moved for an operator
/// who happened to have a waterfall open.
///
/// Ten times a second, which is about the rate `SM` is polled at. A
/// faster loop would render a passband to answer a question nobody asked.
pub fn spawn(radio: crate::emulator::SharedRadio, band: Band) {
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        loop {
            let dial_hz = {
                let guard = radio.lock().expect("radio lock");
                guard.radio().state().vfo_a_hz
            };
            let raw = reading(&band, dial_hz, started.elapsed().as_secs_f64());
            radio
                .lock()
                .expect("radio lock")
                .radio_mut()
                .set_smeter(raw);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use cat_framework::capabilities::SUnitScale;
    use cat_signal::synthetic::{Emission, Emitter};

    fn band_with_carrier_at(hz: u64, dbm: f32) -> Band {
        Band::empty(-120.0, 1).with(Emitter::new(hz, Emission::Cw, dbm))
    }

    #[test]
    fn the_mapping_agrees_with_the_scale_the_console_draws() {
        // The whole point: a console labelling raw 20 "S9" and an emulator
        // that thought S9 was raw 15 would disagree about the same signal,
        // and every test between them would still pass.
        let scale = SUnitScale::TS570D;
        for (dbm, want) in [
            (-73.0, "S9"),
            (-79.0, "S8"),
            (-85.0, "S7"),
            (-121.0, "S1"),
            (-63.0, "S9+10"),
            (-53.0, "S9+20"),
        ] {
            assert_eq!(scale.label(raw_for_dbm(dbm)), want, "{dbm} dBm");
        }
    }

    #[test]
    fn a_quiet_band_rests_near_the_bottom() {
        // Near, not at. A real S-meter shows its noise floor -- a needle
        // pinned at exactly zero on a live receiver would be the
        // suspicious reading. What matters is that a quiet band is
        // unmistakably quiet.
        let quiet = Band::empty(-130.0, 1);
        let raw = reading(&quiet, 14_100_000, 0.0);
        assert!(raw <= 3, "a dead band read {raw}");
    }

    #[test]
    fn tuning_onto_a_signal_raises_the_needle() {
        // The behaviour this module exists for, stated as the thing an
        // operator does.
        let band = band_with_carrier_at(14_100_000, -60.0);
        let on = reading(&band, 14_100_000, 0.0);
        let off = reading(&band, 14_200_000, 0.0);
        assert!(on > off, "on {on} vs off {off}");
        assert!(
            on >= 20,
            "a -60 dBm carrier should read at least S9, got {on}"
        );
    }

    #[test]
    fn a_signal_outside_the_passband_barely_moves_it() {
        // A meter that read signals it was not passing would rise on a
        // neighbour an operator cannot hear.
        //
        // "Barely", not "not at all": a carrier this strong 20 kHz away
        // does leak a little into a 3 kHz window, and a synthetic band
        // that pretended otherwise would be less realistic than the radio
        // it stands in for. What must hold is that tuning onto the signal
        // is dramatically different from tuning near it.
        let band = band_with_carrier_at(14_100_000, -50.0);
        let on = reading(&band, 14_100_000, 0.0);
        let far = reading(&band, 14_100_000 + 20_000, 0.0);
        assert!(
            far * 3 < on,
            "20 kHz off read {far} against {on} on frequency"
        );
    }

    #[test]
    fn a_stronger_signal_reads_higher() {
        let weak = band_with_carrier_at(14_100_000, -100.0);
        let strong = band_with_carrier_at(14_100_000, -60.0);
        assert!(reading(&strong, 14_100_000, 0.0) > reading(&weak, 14_100_000, 0.0));
    }

    #[test]
    fn it_never_reads_past_the_top_of_the_scale() {
        // `SM` is a two-digit field and the capability set says 0-30. A
        // reading above that would be a protocol violation as well as a
        // lie.
        let enormous = band_with_carrier_at(14_100_000, 40.0);
        assert!(reading(&enormous, 14_100_000, 0.0) <= 30);
        assert_eq!(raw_for_dbm(f32::INFINITY), 30);
        assert_eq!(raw_for_dbm(f32::NEG_INFINITY), 0);
    }

    #[test]
    fn the_reading_is_monotonic_in_signal_strength() {
        // No fold-back: a stronger signal must never read lower, which an
        // off-by-one in the two-slope mapping would produce right at S9.
        let mut last = 0;
        let mut dbm = -130.0;
        while dbm <= 0.0 {
            let raw = raw_for_dbm(dbm);
            assert!(raw >= last, "{dbm} dBm read {raw} after {last}");
            last = raw;
            dbm += 0.5;
        }
    }
}
