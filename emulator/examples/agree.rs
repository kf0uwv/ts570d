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

//! Does the emulator's S-meter agree with its spectrum?
//!
//! ```text
//! cargo run -p emulator --example agree
//! ```
//!
//! The question this emulator exists to let a console be tested against.
//! For each of the strongest signals on 20 m it prints what the band says
//! is there and what the meter would read tuned onto it, over several
//! moments — because a mode with an envelope is *supposed* to come and go,
//! and a reading that never moved would be the suspicious one.

fn main() {
    let band = emulator::tap::band_for(7);
    let mut emitters: Vec<_> = band
        .emitters()
        .iter()
        .filter(|e| (14_000_000..14_350_000).contains(&e.frequency_hz))
        .collect();
    emitters.sort_by(|a, b| b.level_dbm.partial_cmp(&a.level_dbm).unwrap());

    println!(
        "{:>12}  {:>8}  {:>10}  S-meter over 10 s",
        "frequency", "level", "mode"
    );
    for e in emitters.iter().take(8) {
        let readings: Vec<String> = (0..10)
            .map(|s| emulator::meter::reading(&band, e.frequency_hz, f64::from(s)).to_string())
            .collect();
        println!(
            "{:>9.3} MHz  {:>5.0} dBm  {:>10}  {}",
            e.frequency_hz as f64 / 1e6,
            e.level_dbm,
            format!("{:?}", e.emission),
            readings.join(" ")
        );
    }
}
