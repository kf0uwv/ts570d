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

//! Swapping the ACC2 audio source under a running server.
//!
//! Same failure to guard against as the IF tap's: the attach is accepted,
//! the picker highlights the new device, and the capture thread keeps
//! reading the old one. So the two sources here are *distinguishable* —
//! different sample rates — rather than two of the same thing, which
//! would pass whether or not the swap happened.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cat_native::RadioHost;
use cat_rigctl::native_bridge::NativeShared;
use cat_signal::{AudioFrame, AudioScopeFrame, AudioSpectrumFrame};
use cat_signal_audio::{AudioError, AudioTap};
use server::audio::{spawn, AudioSelection, AudioSource};

/// A tap that produces frames stamped with a rate, so a test can tell one
/// source from another.
struct Fixed {
    sample_rate_hz: u32,
    sequence: Arc<Mutex<u64>>,
}

impl AudioTap for Fixed {
    fn try_next_frame(&mut self) -> Result<Option<AudioFrame>, AudioError> {
        let mut n = self.sequence.lock().unwrap();
        *n += 1;
        Ok(Some(AudioFrame {
            scope: AudioScopeFrame {
                sample_rate_hz: self.sample_rate_hz,
                samples: vec![0.0; 8],
                sequence: *n,
            },
            spectrum: AudioSpectrumFrame {
                start_hz: 0,
                span_hz: 4_000,
                bins: vec![-90.0; 4],
                sequence: *n,
            },
        }))
    }
}

fn source(sample_rate_hz: u32) -> AudioSource {
    Box::new(Fixed {
        sample_rate_hz,
        sequence: Arc::new(Mutex::new(0)),
    })
}

fn wait_for_rate(shared: &NativeShared, want: u32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(frame) = shared.audio() {
            if frame.scope.sample_rate_hz == want {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn a_console_picking_a_new_source_changes_what_is_published() {
    let shared = NativeShared::new(&radio::capabilities::TS570D);
    let selection = AudioSelection::new();
    selection.select("first".to_string(), source(48_000));
    spawn(Arc::clone(&shared), Arc::clone(&selection));

    assert!(
        wait_for_rate(&shared, 48_000),
        "the first source never published"
    );

    selection.select("second".to_string(), source(44_100));

    assert!(
        wait_for_rate(&shared, 44_100),
        "the capture thread kept reading the first source after a new one was chosen"
    );
}

#[test]
fn a_server_given_no_audio_publishes_none_rather_than_silence() {
    // Silence is a signal — a quiet band sounds exactly like it. "No
    // source" has to be a different answer, or a console would draw a
    // flat trace and an operator would go looking for their cable.
    let shared = NativeShared::new(&radio::capabilities::TS570D);
    let selection = AudioSelection::new();
    spawn(Arc::clone(&shared), Arc::clone(&selection));

    assert!(!selection.is_set());
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        shared.audio().is_none(),
        "a server with nothing attached published audio anyway"
    );
}

#[test]
fn a_source_that_will_not_open_leaves_the_running_one_alone() {
    // Why the attach opens the source before handing it over: a bad
    // choice fails while the operator is still looking at the picker, and
    // what was playing keeps playing.
    let shared = NativeShared::new(&radio::capabilities::TS570D);
    let selection = AudioSelection::new();
    selection.select("good".to_string(), source(48_000));
    spawn(Arc::clone(&shared), Arc::clone(&selection));
    assert!(wait_for_rate(&shared, 48_000));

    let err = match server::audio::open_spec("127.0.0.1:1") {
        Err(e) => e,
        Ok(_) => panic!("nothing listens on port 1, so opening it must fail"),
    };
    assert!(!err.is_empty(), "a refusal must say something");

    assert!(
        wait_for_rate(&shared, 48_000),
        "a failed attach took down the running source"
    );
}
