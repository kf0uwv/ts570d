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

//! The two device/network classifiers must agree.
//!
//! This program has one (`src/endpoint.rs`, covering the CAT port, the IF
//! output and the audio pair) and `cat-signal-audio` ships its own for the
//! audio grammar it owns. Two implementations of "is this a device?" that
//! disagreed would mean a string the console accepted and the library
//! refused, or worse, one they both accepted and resolved differently.
//!
//! Neither is going away — the library must be usable by a console that is
//! not this one — so the guard is that they answer alike.

#[path = "../src/endpoint.rs"]
mod endpoint;

use cat_signal_audio::AudioEndpoint;

#[test]
fn the_console_and_the_library_classify_audio_endpoints_alike() {
    for spec in [
        "audio:default",
        "audio:pipewire",
        "audio:HDA Intel",
        // A card name that looks like a port, which is the case a naive
        // classifier gets wrong.
        "audio:1234",
        "127.0.0.1:4002",
        "radio.local:4002",
    ] {
        let console_says_device = matches!(endpoint::classify(spec), endpoint::Endpoint::Device(_));
        let library_says_device = AudioEndpoint::parse(spec).is_device();
        assert_eq!(
            console_says_device, library_says_device,
            "{spec:?}: console device={console_says_device}, library device={library_says_device}"
        );
    }
}

#[test]
fn the_scheme_constant_is_the_same_string_in_both() {
    assert_eq!(endpoint::AUDIO_SCHEME, cat_signal_audio::DEVICE_SPEC_PREFIX);
}
