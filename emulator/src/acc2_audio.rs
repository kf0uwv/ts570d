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

//! The ACC2 audio pair: pin 3 (ANO) out and pin 11 (PKD) in.
//!
//! In the box these two go to a USB sound device. Here they go to a socket,
//! for the same reason the CN4 tap serves `rtl_tcp` rather than pretending
//! to be a dongle on a USB bus: what matters is that the *audio* is real,
//! paced, and derived from the same band the rest of the radio is showing.
//!
//! # The wire
//!
//! One TCP connection, full duplex, carrying **48 kHz mono signed 16-bit
//! little-endian** PCM in both directions — a sound card's ordinary format,
//! in a sound card's ordinary rate. Server to client is ANO; client to
//! server is PKD. A duplex socket rather than two ports because that is
//! what the sound device is: one stream in each direction, opened together
//! and closed together.
//!
//! Paced to real time, exactly as [`crate::tap`] paces its IQ. Audio that
//! arrived as fast as the CPU allowed would make an AF scope's timebase a
//! decoration.
//!
//! # ANO carries the band the waterfall is showing
//!
//! This is the property worth having. The samples are built from the *same*
//! [`cat_signal::synthetic::Band`] the CN4 tap serves, at the same dial, so
//! a station visible as a trace in the panorama is audible as a tone at the
//! offset the panorama puts it at. A receiver whose audio was unrelated
//! noise would let a console pass every test while showing an operator two
//! views that contradict each other.
//!
//! Each emitter within the receive passband becomes audio at the frequency
//! a real receiver would beat it down to — `|emitter - dial|`, on the
//! appropriate sideband — with its own envelope, so a CW station keys, an
//! FT8 station appears and disappears on its 15-second slots, and SSB stays
//! restless. That is a receiver's product, arrived at directly rather than
//! by demodulating IQ, and the two agree because both ask the same emitter
//! the same question.
//!
//! # What ANO is not
//!
//! It is not the headphone jack. Datasheet §6, pin 3: "RX AF out, **fixed**
//! (Menu 34)". It does not follow the AF gain, which is precisely why an
//! interface takes receive audio from here — turning the volume down must
//! not change what a decoder hears. [`crate::acc2::ano_level`] holds that
//! rule and this module obeys it.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cat_signal::synthetic::{Band, Emission};
use radio::ts570d_radio::Ts570dState;

use crate::acc2;
use crate::emulator::SharedRadio;

/// What both directions of the ACC2 audio pair run at.
pub const SAMPLE_RATE_HZ: u32 = 48_000;

/// Samples per write. About 21 ms at 48 kHz — short enough that keying and
/// retuning show up promptly, long enough not to syscall per sample.
const BLOCK: usize = 1024;

/// The receiver's noise floor, as a sample amplitude.
///
/// Present on purpose. A receiver that went digitally silent between
/// signals would let an AF display show a noise floor it invented, and
/// would make "the audio path is connected but nothing is on the band"
/// indistinguishable from "the audio path is dead".
const NOISE_FLOOR: f32 = 0.012;

/// The lowest audio frequency the receiver passes, in Hz.
const AF_LOW_HZ: f64 = 300.0;

/// The measured level of what the client is sending into PKD, published as
/// a fraction of full scale in parts per million so it fits an atomic.
static PKD_LEVEL_PPM: AtomicU32 = AtomicU32::new(0);

/// The last measured PKD (transmit audio) level, 0.0-1.0.
///
/// Read by the TUI so an operator can see that the sound card is feeding
/// the radio, which is otherwise invisible until something is transmitted.
pub fn pkd_level() -> f32 {
    PKD_LEVEL_PPM.load(Ordering::Relaxed) as f32 / 1_000_000.0
}

/// The audio passband, as offsets from the dial in Hz.
///
/// Delegates to [`radio::audio_passband_hz`] rather than deciding here.
/// The console marks these same edges on its AF FFT, and if the two ever
/// disagreed an operator would hear a tone outside the marked passband --
/// which is one of them lying, with no way to tell which.
pub fn passband(state: &Ts570dState) -> (f64, f64) {
    let mode = radio::Mode::try_from(state.mode).unwrap_or(radio::Mode::Usb);
    let (lo, hi) = radio::audio_passband_hz(mode, state.cw_pitch);
    (f64::from(lo), f64::from(hi))
}

/// One block of receive audio, as samples in -1.0..=1.0.
///
/// `t` is seconds since the stream began, passed in rather than read from a
/// clock so a test can step it and so two renders of the same instant
/// agree — the same discipline `cat_signal`'s own renderers use.
pub fn ano_samples(band: &Band, state: &Ts570dState, t: f64, count: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; count];
    let rate = f64::from(SAMPLE_RATE_HZ);

    // A transmitting receiver is a muted receiver. Without this the AF
    // panels would show a band during transmit, which no radio does.
    if state.tx {
        return out;
    }

    let (low, high) = passband(state);
    let dial = state.vfo_a_hz as f64;
    let level = acc2::ano_level(state);

    for emitter in band.emitters() {
        let offset = emitter.frequency_hz as f64 - dial;
        if offset < low.min(high) || offset > low.max(high) {
            continue;
        }
        let envelope = emitter.envelope(t);
        if envelope <= 0.0 {
            continue;
        }

        // The audio frequency a receiver beats this down to. Always
        // positive: a signal 1 kHz below an LSB dial and one 1 kHz above a
        // USB dial both come out as a 1 kHz note.
        let audio_hz = offset.abs();
        let amplitude = amplitude_of(band, emitter.level_dbm) * envelope;

        match emitter.emission {
            // Broadband hiss rather than a tone: a noise source that
            // rendered as a note would be the one signal on the band a
            // listener could not identify.
            Emission::Noise => {
                for (n, sample) in out.iter_mut().enumerate() {
                    *sample += amplitude * pseudo_noise(emitter.frequency_hz, n);
                }
            }
            // Voice occupies its whole bandwidth, so it is built from
            // several partials rather than one, and comes out as something
            // an AF FFT shows as a band instead of a line.
            Emission::Ssb => {
                for partial in 0..6 {
                    let spread = f64::from(partial) * (emitter.emission.bandwidth_hz() / 6.0);
                    let hz = audio_hz + spread;
                    if !(AF_LOW_HZ..rate / 2.0).contains(&hz) {
                        continue;
                    }
                    add_tone(&mut out, hz, amplitude * 0.4, rate, t, partial as u64);
                }
            }
            _ => {
                if (AF_LOW_HZ..rate / 2.0).contains(&audio_hz) {
                    add_tone(&mut out, audio_hz, amplitude, rate, t, 0);
                }
            }
        }
    }

    for (n, sample) in out.iter_mut().enumerate() {
        *sample += NOISE_FLOOR * pseudo_noise(0xACC2, n);
        // ANO is fixed-level: Menu 34 scales it, the AF gain does not.
        *sample *= level;
        *sample = sample.clamp(-1.0, 1.0);
    }
    out
}

/// How loud a signal is, relative to the band's noise floor.
///
/// The 40 dB reference is `Band::iq_bytes`'s, so a signal that looks strong
/// in the panorama sounds strong here.
///
/// `SCALE` is set so that the whole range `populate_range` produces --
/// 12 to 57 dB above the floor -- lands inside full scale with room to sum.
/// It was originally 2.5x larger, which put every signal above about 50 dB
/// at the clamp: an AF display would have shown most of the band as one
/// identical pegged blob, and a scope would have been flat-topped
/// everywhere. Found by scanning the band against the running emulator.
///
/// There is deliberately **no AGC**. A real receiver's would compress this
/// range back out again, and signal strength would stop being audible at
/// all -- which is the wrong trade for a fixture whose job is to make the
/// band legible.
fn amplitude_of(band: &Band, level_dbm: f32) -> f32 {
    const SCALE: f32 = 0.02;
    let above_floor = level_dbm - band.floor_dbm;
    (10f32.powf(above_floor / 40.0) * SCALE).min(0.9)
}

fn add_tone(out: &mut [f32], hz: f64, amplitude: f32, rate: f64, t: f64, phase_seed: u64) {
    let radians_per_sample = std::f64::consts::TAU * hz / rate;
    let phase0 = t * std::f64::consts::TAU * hz + phase_seed as f64 * 0.7;
    for (n, sample) in out.iter_mut().enumerate() {
        *sample += amplitude * (phase0 + radians_per_sample * n as f64).sin() as f32;
    }
}

/// Deterministic noise. Not a good PRNG, and does not need to be: it needs
/// to be broadband, bounded, and the same every run.
fn pseudo_noise(seed: u64, n: usize) -> f32 {
    let mut x = seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(n as u64);
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 29;
    ((x >> 40) as f32 / 8_388_608.0) - 1.0
}

/// Encode samples as signed 16-bit little-endian.
pub fn encode_pcm(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Decode signed 16-bit little-endian samples.
pub fn decode_pcm(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])) / f32::from(i16::MAX))
        .collect()
}

/// Serve the ACC2 audio pair. Spawns a thread and returns the bound address.
pub fn serve(radio: SharedRadio, band: Band, addr: &str) -> std::io::Result<SocketAddr> {
    let listener = TcpListener::bind(addr)?;
    let bound = listener.local_addr()?;
    let band = Arc::new(band);

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let radio = radio.clone();
            let band = Arc::clone(&band);
            std::thread::spawn(move || {
                let _ = serve_one(stream, radio, band);
            });
        }
    });
    Ok(bound)
}

fn serve_one(stream: TcpStream, radio: SharedRadio, band: Arc<Band>) -> std::io::Result<()> {
    stream.set_nodelay(true)?;

    // PKD on its own thread. Transmit audio arrives whenever the client
    // feels like sending it, and a receiver that stopped producing audio
    // while waiting for some would be a duplex stream in name only.
    let pkd_stream = stream.try_clone()?;
    let pkd = std::thread::spawn(move || pkd_loop(pkd_stream));

    let result = ano_loop(stream, radio, band);
    let _ = pkd.join();
    result
}

/// Stream receive audio out, paced to the sample rate.
fn ano_loop(mut stream: TcpStream, radio: SharedRadio, band: Arc<Band>) -> std::io::Result<()> {
    let started = Instant::now();
    let mut sent: u64 = 0;

    loop {
        let state = radio.lock().expect("radio lock").radio().state().clone();
        let t = started.elapsed().as_secs_f64();
        let samples = ano_samples(&band, &state, t, BLOCK);
        stream.write_all(&encode_pcm(&samples))?;
        stream.flush()?;

        sent += BLOCK as u64;
        let due = std::time::Duration::from_secs_f64(sent as f64 / f64::from(SAMPLE_RATE_HZ));
        if let Some(sleep) = due.checked_sub(started.elapsed()) {
            std::thread::sleep(sleep);
        }
    }
}

/// Read transmit audio in, and measure it.
///
/// SN-6: "ACC2 pin 11 feeds the mic path; a floating line-out keys VOX into
/// rapid TX/RX chatter. This design keys via DTR only — VOX stays off."
/// So audio arriving here is measured and **never** keys the radio. The
/// only thing that keys this virtual radio through ACC2 is pin 9.
fn pkd_loop(mut stream: TcpStream) {
    let mut buf = [0u8; BLOCK * 2];
    loop {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => {
                PKD_LEVEL_PPM.store(0, Ordering::Relaxed);
                return;
            }
            Ok(n) => {
                let samples = decode_pcm(&buf[..n - n % 2]);
                let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
                PKD_LEVEL_PPM.store((peak * 1_000_000.0) as u32, Ordering::Relaxed);
            }
        }
    }
}

/// The shared band, so the tap and the audio pin describe one radio.
///
/// A second, separately seeded band would give a console a waterfall and an
/// AF display that disagreed about what is on the air — the exact failure
/// this module exists to avoid.
pub type SharedBand = Arc<Mutex<Band>>;

#[cfg(test)]
mod tests {
    use super::*;
    use cat_signal::synthetic::Emitter;

    fn state_at(hz: u64, mode: u8) -> Ts570dState {
        let mut s = Ts570dState {
            vfo_a_hz: hz,
            mode,
            ..Ts570dState::default()
        };
        // Menu 34 at full scale, so the level rule is not what is under
        // test here.
        s.menu_values[acc2::MENU_ANO_LEVEL] = 9;
        s
    }

    fn one_signal(hz: u64, emission: Emission) -> Band {
        let mut band = Band::empty(-110.0, 1);
        band.push(Emitter::new(hz, emission, -40.0));
        band
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// The strongest audio frequency present, by direct correlation. A
    /// full FFT would be more code and no more convincing at one bin.
    fn strongest_hz(samples: &[f32], candidates: &[f64]) -> f64 {
        let rate = f64::from(SAMPLE_RATE_HZ);
        let mut best = (0.0f64, f64::NEG_INFINITY);
        for &hz in candidates {
            let mut re = 0.0f64;
            let mut im = 0.0f64;
            for (n, &s) in samples.iter().enumerate() {
                let phase = std::f64::consts::TAU * hz * n as f64 / rate;
                re += f64::from(s) * phase.cos();
                im += f64::from(s) * phase.sin();
            }
            let power = re * re + im * im;
            if power > best.1 {
                best = (hz, power);
            }
        }
        best.0
    }

    #[test]
    fn a_signal_in_the_passband_comes_out_at_its_beat_note() {
        // The property the whole module exists for: a station 1 kHz above a
        // USB dial is a 1 kHz tone.
        let band = one_signal(14_075_000, Emission::Cw);
        let state = state_at(14_074_000, 2);
        let samples = ano_samples(&band, &state, 0.0, 4096);

        let candidates: Vec<f64> = (1..=30).map(|k| f64::from(k) * 100.0).collect();
        assert_eq!(strongest_hz(&samples, &candidates), 1_000.0);
    }

    #[test]
    fn usb_hears_above_the_dial_and_lsb_below() {
        // Get the sideband backwards and an AF display is confidently
        // wrong, which is worse than blank.
        let above = one_signal(14_075_000, Emission::Cw);
        let below = one_signal(14_073_000, Emission::Cw);

        let usb = state_at(14_074_000, 2);
        assert!(peak(&ano_samples(&above, &usb, 0.0, 2048)) > NOISE_FLOOR * 2.0);
        assert!(
            peak(&ano_samples(&below, &usb, 0.0, 2048)) <= NOISE_FLOOR * 2.0,
            "USB must not hear a station below the dial"
        );

        let lsb = state_at(14_074_000, 1);
        assert!(peak(&ano_samples(&below, &lsb, 0.0, 2048)) > NOISE_FLOOR * 2.0);
        assert!(
            peak(&ano_samples(&above, &lsb, 0.0, 2048)) <= NOISE_FLOOR * 2.0,
            "LSB must not hear a station above the dial"
        );
    }

    #[test]
    fn a_station_outside_the_passband_is_not_heard() {
        let band = one_signal(14_084_000, Emission::Cw);
        let state = state_at(14_074_000, 2);
        assert!(peak(&ano_samples(&band, &state, 0.0, 2048)) <= NOISE_FLOOR * 2.0);
    }

    #[test]
    fn retuning_moves_the_note_because_the_band_stands_still() {
        // The same property the tap has: signals live at absolute
        // frequencies and the dial moves over them.
        let band = one_signal(14_075_000, Emission::Cw);
        let candidates: Vec<f64> = (1..=30).map(|k| f64::from(k) * 100.0).collect();

        let at_74 = ano_samples(&band, &state_at(14_074_000, 2), 0.0, 4096);
        assert_eq!(strongest_hz(&at_74, &candidates), 1_000.0);

        let at_745 = ano_samples(&band, &state_at(14_074_500, 2), 0.0, 4096);
        assert_eq!(strongest_hz(&at_745, &candidates), 500.0);
    }

    #[test]
    fn a_transmitting_radio_produces_no_receive_audio() {
        let band = one_signal(14_075_000, Emission::Cw);
        let mut state = state_at(14_074_000, 2);
        state.tx = true;
        assert_eq!(peak(&ano_samples(&band, &state, 0.0, 2048)), 0.0);
    }

    #[test]
    fn an_empty_band_still_carries_a_noise_floor() {
        // "Connected but quiet" and "dead" must not look the same.
        let band = Band::empty(-110.0, 1);
        let samples = ano_samples(&band, &state_at(14_074_000, 2), 0.0, 2048);
        assert!(peak(&samples) > 0.0, "a receiver is never digitally silent");
        assert!(peak(&samples) < 0.1, "and its noise floor is not a signal");
    }

    #[test]
    fn menu_34_scales_ano_and_the_af_gain_does_not() {
        // Datasheet §6, pin 3: "RX AF out, fixed (Menu 34)".
        let band = one_signal(14_075_000, Emission::Cw);
        let mut state = state_at(14_074_000, 2);

        let loud = peak(&ano_samples(&band, &state, 0.0, 2048));

        state.af_gain = 0;
        assert_eq!(
            peak(&ano_samples(&band, &state, 0.0, 2048)),
            loud,
            "turning the volume down must not change what a decoder hears"
        );

        state.menu_values[acc2::MENU_ANO_LEVEL] = 0;
        assert_eq!(
            peak(&ano_samples(&band, &state, 0.0, 2048)),
            0.0,
            "Menu 34 is what sets the level"
        );
    }

    #[test]
    fn cw_listens_at_the_sidetone_pitch() {
        let band = one_signal(14_074_700, Emission::Cw);
        let mut state = state_at(14_074_000, 3);
        // Index 3 -> 700 Hz. The window comes from the radio crate, so
        // this asserts the emulator is asking it rather than deciding.
        state.cw_pitch = 3;
        let (lo, hi) = passband(&state);
        assert!(
            lo < 700.0 && hi > 700.0,
            "{lo}..{hi} must contain the pitch"
        );
        assert!(
            peak(&ano_samples(&band, &state, 0.0, 2048)) > NOISE_FLOOR * 2.0,
            "a station at the pitch offset must be inside the CW window"
        );

        // The same station with the pitch set elsewhere falls outside.
        state.cw_pitch = 0;
        assert!(peak(&ano_samples(&band, &state, 0.0, 2048)) <= NOISE_FLOOR * 2.0);
    }

    #[test]
    fn the_loudest_signal_the_band_generates_does_not_peg_full_scale() {
        // `populate_range` tops out at 57 dB above the floor. If that
        // clipped, every strong signal would sound and look identical and
        // an AF scope would be flat-topped across most of the band.
        // A steady carrier rather than a keyed one, so the envelope is not
        // quietly doing half the work of the assertion.
        let mut band = Band::empty(-110.0, 1);
        band.push(Emitter::new(14_075_000, Emission::Am, -110.0 + 57.0));
        let samples = ano_samples(&band, &state_at(14_074_000, 2), 0.0, 4096);
        let loudest = peak(&samples);

        assert!(
            loudest < 1.0,
            "the strongest signal must not clip: {loudest}"
        );
        assert!(
            loudest > 0.2,
            "and it must still be clearly louder than the floor: {loudest}"
        );
    }

    #[test]
    fn a_weak_signal_and_a_strong_one_are_distinguishable() {
        let weak = {
            let mut b = Band::empty(-110.0, 1);
            b.push(Emitter::new(14_075_000, Emission::Cw, -110.0 + 12.0));
            b
        };
        let strong = {
            let mut b = Band::empty(-110.0, 1);
            b.push(Emitter::new(14_075_000, Emission::Cw, -110.0 + 50.0));
            b
        };
        let state = state_at(14_074_000, 2);
        let quiet = peak(&ano_samples(&weak, &state, 0.0, 2048));
        let loud = peak(&ano_samples(&strong, &state, 0.0, 2048));
        assert!(
            loud > quiet * 4.0,
            "38 dB of RF must be audible as a real difference: {quiet} vs {loud}"
        );
    }

    #[test]
    fn pcm_round_trips() {
        let samples = vec![0.0, 0.5, -0.5, 1.0, -1.0];
        let decoded = decode_pcm(&encode_pcm(&samples));
        for (a, b) in samples.iter().zip(decoded.iter()) {
            assert!((a - b).abs() < 1e-3, "{a} != {b}");
        }
    }

    #[test]
    fn a_keyed_cw_station_goes_quiet_between_characters() {
        // The envelope is shared with the IQ path, so the audio and the
        // waterfall agree about when a station is on the air.
        let band = one_signal(14_075_000, Emission::Cw);
        let state = state_at(14_074_000, 2);
        let emitter = &band.emitters()[0];

        let loud = (0..40)
            .map(|k| f64::from(k) * 0.1)
            .find(|&t| emitter.envelope(t) > 0.9)
            .expect("a moment with the key down");
        let quiet = (0..40)
            .map(|k| f64::from(k) * 0.1)
            .find(|&t| emitter.envelope(t) < 0.5)
            .expect("a moment with the key up");

        let down = peak(&ano_samples(&band, &state, loud, 2048));
        let up = peak(&ano_samples(&band, &state, quiet, 2048));
        assert!(
            down > up,
            "key down ({down}) must be louder than key up ({up})"
        );
    }
}
