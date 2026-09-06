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

//! Reading the ACC2 receive audio, and publishing what it hears.
//!
//! The same shape as [`crate::spectrum`], and for the same reasons: a
//! thread of its own because capturing audio is blocking I/O and the FFT
//! is real work; newest-wins into a shared slot; a selection that a
//! console can change from the far end of a socket.
//!
//! # Why the *server* runs the DSP
//!
//! The samples are here — this is the machine with the sound card in it.
//! Sending raw PCM instead would move the FFT to a console that has no
//! audio hardware in the picture at all, and cost more bandwidth doing
//! it: 48 kHz of `f32` is about 190 KB/s against roughly 145 KB/s of
//! finished frames at the pump rate.
//!
//! # Both halves travel together
//!
//! A [`cat_signal::AudioFrame`] is a scope trace and a spectrum computed
//! from the *same* block, sharing a sequence number. That is the whole
//! point of the type, and it survives to the console intact.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cat_rigctl::native_bridge::NativeShared;
use cat_signal_audio::{AudioFrame, AudioTap};
use tracing::{info, warn};

/// A source of audio frames, whatever it is underneath.
///
/// Boxed rather than generic because the two kinds — a PCM server on a
/// socket and a local sound card — are chosen at runtime from a string an
/// operator typed, and a server that had to be generic over which one
/// would have to be generic all the way up to `main`.
pub type AudioSource = Box<dyn AudioTap + Send>;

/// Which audio source the capture thread should be reading.
///
/// The counterpart of [`crate::spectrum::IfSelection`], down to handing
/// over an already-opened source: a busy sound card refuses the attach
/// while the operator is still looking at the picker, rather than two
/// seconds later on a thread nobody is watching.
pub struct AudioSelection {
    pending: Mutex<Option<AudioSource>>,
    /// What the last selection was called, for logging. There is no
    /// reconnect-by-spec here: unlike an `rtl_tcp` socket, a sound card
    /// that goes away has usually been unplugged, and silently grabbing
    /// it again later is not obviously right.
    label: Mutex<Option<String>>,
    generation: AtomicU64,
}

impl AudioSelection {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            pending: Mutex::new(None),
            label: Mutex::new(None),
            generation: AtomicU64::new(0),
        })
    }

    /// Hand over an already-opened source, replacing whatever is running.
    pub fn select(&self, label: String, source: AudioSource) {
        if let Ok(mut slot) = self.pending.lock() {
            *slot = Some(source);
        }
        if let Ok(mut current) = self.label.lock() {
            *current = Some(label);
        }
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// Whether anything has ever been selected.
    pub fn is_set(&self) -> bool {
        self.label.lock().map(|l| l.is_some()).unwrap_or(false)
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    fn take_pending(&self) -> Option<AudioSource> {
        self.pending.lock().ok()?.take()
    }

    fn label(&self) -> String {
        self.label
            .lock()
            .ok()
            .and_then(|l| l.clone())
            .unwrap_or_else(|| "audio".to_string())
    }
}

/// Read the selected source forever, publishing frames into `shared`.
pub fn spawn(shared: Arc<NativeShared>, selection: Arc<AudioSelection>) {
    std::thread::spawn(move || loop {
        let generation = selection.generation();
        match selection.take_pending() {
            Some(mut source) => {
                info!("ACC2 audio: reading {}", selection.label());
                if let Err(e) = read_frames(&shared, &selection, generation, &mut source) {
                    warn!("ACC2 audio: {e}");
                }
            }
            None => {
                // Nothing selected. Normal for a server started without
                // `--acc2-audio` and never given a source.
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
        }
        // A replacement is already open and waiting, so it does not wait.
        if selection.generation() == generation {
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

fn read_frames(
    shared: &NativeShared,
    selection: &AudioSelection,
    generation: u64,
    source: &mut AudioSource,
) -> Result<(), String> {
    loop {
        if selection.generation() != generation {
            info!("ACC2 audio: switching to a new source");
            return Ok(());
        }
        match source.try_next_frame() {
            Ok(Some(frame)) => shared.publish_audio(frame),
            // Nothing ready. `try_next_frame` does not block, so without
            // this the loop would spin a core waiting for a sound card
            // that produces about 47 blocks a second.
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Open the audio source named by `spec`.
///
/// Public because the attach path opens through it too: an operator's
/// choice and an `--acc2-audio` flag must produce the same thing.
pub fn open_spec(spec: &str) -> Result<AudioSource, String> {
    match crate::endpoint::classify(spec) {
        crate::endpoint::Endpoint::Network(addr) => {
            let (stream, _tx) = cat_signal_audio::AudioStream::connect(
                addr.as_str(),
                cat_signal_audio::AudioPipelineConfig::default(),
            )
            .map_err(|e| format!("no ACC2 audio at {addr}: {e}"))?;
            // The transmit half is dropped: a server publishing receive
            // audio does not send any, and holding the handle open would
            // keep the PKD direction alive for no reason.
            Ok(Box::new(stream))
        }
        crate::endpoint::Endpoint::Device(device) => open_device(&device),
    }
}

#[cfg(feature = "audio-device")]
fn open_device(spec: &str) -> Result<AudioSource, String> {
    let capture =
        cat_signal_audio::AudioCapture::open(spec, cat_signal_audio::CaptureConfig::default())
            .map_err(|e| format!("could not open {spec}: {e}"))?;
    // The negotiated format, not the requested one: a card that only does
    // 44.1 kHz is used at 44.1 kHz, and saying so is the difference
    // between a console that is right and one that puts a 1000 Hz note
    // at 1088.
    info!(
        "ACC2 audio opened on {} ({} Hz, {} ch, using ch {})",
        capture.label(),
        capture.format().sample_rate_hz,
        capture.format().channels,
        capture.format().channel
    );
    Ok(Box::new(capture))
}

#[cfg(not(feature = "audio-device"))]
fn open_device(spec: &str) -> Result<AudioSource, String> {
    Err(format!(
        "this server cannot open the sound device {spec:?} -- rebuild it with \
         `--features audio-device` (which needs the platform's sound headers), or \
         point --acc2-audio at a PCM server instead"
    ))
}

/// One block of audio, as this crate hands it on.
///
/// A type alias in name only — it exists so the module's signatures read
/// in terms of what they carry rather than which crate declares it.
pub type Frame = AudioFrame;
