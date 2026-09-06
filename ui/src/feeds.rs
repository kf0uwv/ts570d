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

//! Where the console's two signal panels get their data.
//!
//! The radio has three interfaces and this console was only ever plugged
//! into one of them. CAT gives it the dial and the meters; the **CN4 tap**
//! carries the band panorama and the **ACC2 audio pair** carries the
//! receive audio, and until now the TUI read neither — so its waterfall
//! said `NO SPECTRUM SOURCE` and its AF panels said `PENDING` against an
//! emulator that was serving both the whole time.
//!
//! # Why the spectrum needs a thread and the audio does not
//!
//! They look symmetrical and are not.
//!
//! `cat_signal::SpectrumSource::next_frame` is an `async fn`, but
//! `cat-signal-rtlsdr`'s implementation **blocks inside it** — it reads the
//! socket directly rather than handing off to a worker. On a paced source
//! that is around 21 ms per frame, and this console's UI task is a
//! single-threaded `monoio` runtime shared with key handling and the radio
//! poll. Awaiting it there would stall the console for a fifth of a second
//! at a time; a keypress would arrive whenever the SDR felt like it. So
//! [`SpectrumFeed`] owns a thread, and the UI task only ever looks at what
//! that thread has already produced.
//!
//! `cat-signal-audio` already made the opposite choice: it has its own
//! reader thread and offers `try_next_frame`, which never blocks. So
//! [`AudioFeed`] is a thin thing the UI task can poll directly, and adding
//! a second thread around it would buy nothing.
//!
//! # Both drop frames, and that is the correct behaviour
//!
//! Neither panel is a recorder. A waterfall row that arrives late is worse
//! than one that never arrives, and stale receive audio is worthless. Both
//! feeds keep only what is current and let the rest go — the same
//! latest-frame discipline `radio-cat-rs` ADR 0014 records for the SDR
//! itself, applied one layer up.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cat_signal::{SpectrumFrame, SpectrumSource};
use cat_signal_audio::AudioFrame;
use cat_ui_ratatui::af::AudioState;

/// How many spectrum frames the waterfall keeps.
///
/// The panel draws two history rows per text row, so this is roughly twice
/// the tallest waterfall a terminal will ask for. Older rows are dropped
/// rather than the buffer growing: this is a display, not a log.
const HISTORY: usize = 128;

/// The band panorama, fed by a thread.
pub struct SpectrumFeed {
    frames: Arc<Mutex<Vec<SpectrumFrame>>>,
    /// The dial the source should be centred on. Written by the UI task
    /// when the radio reports a new frequency, read by the thread.
    dial_hz: Arc<AtomicU64>,
    /// Set once the thread has stopped, with why.
    fault: Arc<Mutex<Option<String>>>,
    running: Arc<AtomicBool>,
}

impl SpectrumFeed {
    /// Start feeding from `source`.
    ///
    /// Generic rather than taking a concrete SDR type: naming
    /// `RtlSdrSource<RtlTcpSource>` is the wiring layer's job, and keeping
    /// it out of here is what lets a different source — a native bandscope,
    /// a file — be plugged in without touching the console.
    pub fn start<S>(mut source: S, dial_hz: u64) -> Self
    where
        S: SpectrumSource + Send + 'static,
        S::Error: std::fmt::Display,
    {
        let frames = Arc::new(Mutex::new(Vec::new()));
        let dial = Arc::new(AtomicU64::new(dial_hz));
        let fault = Arc::new(Mutex::new(None));
        let running = Arc::new(AtomicBool::new(true));

        let t_frames = Arc::clone(&frames);
        let t_dial = Arc::clone(&dial);
        let t_fault = Arc::clone(&fault);
        let t_running = Arc::clone(&running);

        std::thread::Builder::new()
            .name("spectrum-feed".to_string())
            .spawn(move || {
                let mut centred_on = dial_hz;
                source.retune(centred_on);
                while t_running.load(Ordering::Relaxed) {
                    // Retune before asking for a frame, so the frame that
                    // comes back is already on the new dial rather than one
                    // window behind it.
                    let wanted = t_dial.load(Ordering::Relaxed);
                    if wanted != centred_on {
                        source.retune(wanted);
                        centred_on = wanted;
                    }
                    // `next_frame` blocks inside its `async fn`, so this
                    // drives it to completion on this thread rather than on
                    // the console's runtime. See the module doc.
                    match futures::executor::block_on(source.next_frame()) {
                        Ok(frame) => {
                            let mut held = t_frames.lock().expect("spectrum lock");
                            held.insert(0, frame);
                            held.truncate(HISTORY);
                        }
                        Err(e) => {
                            *t_fault.lock().expect("spectrum fault lock") =
                                Some(format!("spectrum source stopped: {e}"));
                            return;
                        }
                    }
                }
            })
            .expect("spawn the spectrum feed");

        Self {
            frames,
            dial_hz: dial,
            fault,
            running,
        }
    }

    /// Tell the source the dial has moved.
    ///
    /// An IF tap is dial-centred by construction — the SDR is parked on the
    /// first IF and the radio's own oscillator does the tuning — so a
    /// console that did not pass this on would draw a window that no longer
    /// matches the frequency printed above it.
    pub fn retune(&self, hz: u64) {
        self.dial_hz.store(hz, Ordering::Relaxed);
    }

    /// The history, newest first.
    pub fn frames(&self) -> Vec<SpectrumFrame> {
        self.frames.lock().expect("spectrum lock").clone()
    }

    /// Why the feed stopped, if it has.
    pub fn fault(&self) -> Option<String> {
        self.fault.lock().expect("spectrum fault lock").clone()
    }
}

impl Drop for SpectrumFeed {
    fn drop(&mut self) {
        // The thread checks this between frames. It may sit in one more
        // blocking read before it notices, which is fine: nothing is
        // waiting on it and the process is going away.
        self.running.store(false, Ordering::Relaxed);
    }
}

// `AudioTap` moved into `cat-signal-audio`, beside the two types that
// implement it: a server capturing audio to publish over the protocol
// needs the same abstraction as a console drawing it, and it should not
// have to depend on a terminal UI crate to name it.
pub use cat_signal_audio::AudioTap;

/// The ACC2 receive audio, polled directly.
pub struct AudioFeed {
    tap: Box<dyn AudioTap>,
    latest: Option<AudioFrame>,
    fault: Option<String>,
}

impl AudioFeed {
    pub fn new(tap: impl AudioTap + 'static) -> Self {
        Self {
            tap: Box::new(tap),
            latest: None,
            fault: None,
        }
    }

    /// Take whatever has arrived since the last call. Never blocks.
    ///
    /// Drains rather than taking one, because the audio source produces
    /// about 47 frames a second and this is called at the console's redraw
    /// rate: taking one per redraw would fall progressively further behind
    /// and eventually display audio from minutes ago.
    pub fn poll(&mut self) {
        if self.fault.is_some() {
            return;
        }
        loop {
            match self.tap.try_next_frame() {
                Ok(Some(frame)) => self.latest = Some(frame),
                Ok(None) => return,
                Err(e) => {
                    self.fault = Some(format!("audio stopped: {e}"));
                    return;
                }
            }
        }
    }

    /// The most recent frame, if any has ever arrived.
    pub fn latest(&self) -> Option<&AudioFrame> {
        self.latest.as_ref()
    }

    pub fn fault(&self) -> Option<&str> {
        self.fault.as_deref()
    }

    /// What the console should say about the audio path.
    ///
    /// `Absent` is never returned: this station has the ACC2 pair wired
    /// whether or not this console is attached to it, and claiming
    /// otherwise would be a statement about the hardware that is not true.
    /// A console with no `--acc2-audio` has no `AudioFeed` at all and shows
    /// `Configured`, which is the same honest answer.
    pub fn state(&self) -> AudioState {
        if self.latest.is_some() && self.fault.is_none() {
            AudioState::Streaming
        } else {
            AudioState::Configured
        }
    }
}

/// A source the operator picked, now open.
pub enum Attached {
    Spectrum(SpectrumFeed),
    Audio(AudioFeed),
    /// The attach was asked for on another machine, and there is nothing
    /// to install here.
    ///
    /// Over the native protocol the *server* opens the device; its frames
    /// then arrive on the connection this console already has. Returning a
    /// feed would be wrong — nothing local was opened — and returning an
    /// error would be wronger still, since nothing failed.
    Remote,
}

/// Open a device the operator chose from the picker.
///
/// Supplied by the wiring layer, because `src/main.rs` is the only place
/// concrete source types may be named (Rule 5) — and because what "open
/// this" means differs per source in ways the console has no business
/// knowing. The console's part is to show a list and report what came back.
pub type AttachFn = Box<dyn Fn(&cat_signal::DeviceInfo) -> Result<Attached, String>>;

/// The signal sources a console was started with, and what it can attach.
#[derive(Default)]
pub struct ConsoleSources {
    pub spectrum: Option<SpectrumFeed>,
    pub audio: Option<AudioFeed>,
    /// What this machine can see, enumerated once at startup.
    ///
    /// Once, not per frame: enumerating sound cards talks to the sound
    /// server, and doing that at the console's redraw rate would be a
    /// steady drip of syscalls to answer a question whose answer changes
    /// when somebody plugs something in. Re-enumeration is an explicit
    /// action, not a side effect of drawing.
    pub devices: Vec<cat_signal::DeviceList>,
    pub attach: Option<AttachFn>,
    /// A device list that can change after the console has started.
    ///
    /// The local case is a snapshot: this machine's hardware, enumerated
    /// once. The remote case cannot be, because the answer comes from the
    /// radio's host one round trip after connecting, and a console that
    /// only read `devices` at startup would show an empty list forever.
    pub device_feed: Option<DeviceFeed>,
    /// Things the far end said, for a console to show when it next draws.
    ///
    /// A network link produces messages nobody asked for at a moment
    /// nobody chose — an attach refused, a source that went away. There is
    /// no getter whose return value they belong in, so they queue here.
    pub notices: Option<Notices>,
    /// What the radio said it is, when something else did the asking.
    ///
    /// The console needs a capability document to draw from, and in
    /// `--server` mode the authoritative one is the *server's* — it
    /// carries the layout that server authored. Building one locally from
    /// a static declaration would work for everything except the two
    /// things that only the far end knows: what it has wired, and how it
    /// wants its console arranged.
    ///
    /// `None` means nobody else has a better answer and the console
    /// should use the radio crate's own declaration.
    pub capabilities: Option<cat_native::CapabilitiesWire>,
    /// Take the device list again, on request.
    ///
    /// A closure rather than a method, for the same reason `attach` is
    /// one: what "enumerate" means belongs to the wiring layer -- this
    /// machine's drivers in one mode, a round trip to the radio's host in
    /// the other -- and the console's part is to ask.
    pub refresh_devices: Option<Box<dyn Fn()>>,
}

/// A device list some other thread keeps up to date.
pub type DeviceFeed = std::sync::Arc<std::sync::Mutex<Vec<cat_signal::DeviceList>>>;

/// Unsolicited messages from the far end of a link.
pub type Notices = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

impl ConsoleSources {
    /// Sources for a console that reaches its radio **over the network**.
    ///
    /// No local enumeration, deliberately, and this is the point rather
    /// than a limitation being worked around. A console in this mode is
    /// talking to a `ts570d server` that owns the radio, and that server
    /// may be in another room or another building. The sound cards and
    /// dongles on *this* machine are not the radio's: offering them would
    /// let an operator on a laptop pick their laptop's microphone as the
    /// radio's ACC2 receive audio, which would look entirely correct and be
    /// entirely wrong.
    ///
    /// So both groups report `unavailable` with the reason, rather than
    /// being absent. An empty list says "nothing is plugged in", which is a
    /// claim about the radio's host this console is in no position to make.
    pub fn remote() -> Self {
        let why = "the radio is on another host, and this console cannot ask it what \
                   it has attached yet";
        Self {
            devices: vec![
                cat_signal::DeviceList::unavailable(cat_signal::DeviceKind::AudioInput, why),
                cat_signal::DeviceList::unavailable(cat_signal::DeviceKind::Sdr, why),
            ],
            ..Self::default()
        }
    }

    /// Apply a freshly opened source, replacing whatever was there.
    pub fn accept(&mut self, attached: Attached) {
        match attached {
            Attached::Spectrum(feed) => self.spectrum = Some(feed),
            Attached::Audio(feed) => self.audio = Some(feed),
            // Nothing to install: the device was opened on the radio's
            // machine, and whatever it produces arrives on the connection
            // already feeding this console.
            Attached::Remote => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cat_signal::fake::FakeSpectrumSource;

    #[test]
    fn a_started_feed_collects_frames_and_keeps_the_newest_first() {
        let feed = SpectrumFeed::start(FakeSpectrumSource::new(), 14_074_000);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while feed.frames().len() < 3 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let frames = feed.frames();
        assert!(frames.len() >= 3, "the feed produced {}", frames.len());
        assert!(
            frames[0].sequence > frames[1].sequence,
            "newest first: {} then {}",
            frames[0].sequence,
            frames[1].sequence
        );
    }

    #[test]
    fn the_history_is_bounded_because_this_is_a_display_and_not_a_log() {
        let feed = SpectrumFeed::start(FakeSpectrumSource::new(), 14_074_000);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while feed.frames().len() < HISTORY && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(feed.frames().len() <= HISTORY);
    }

    #[test]
    fn a_retune_reaches_the_source() {
        let feed = SpectrumFeed::start(FakeSpectrumSource::new(), 14_074_000);
        feed.retune(7_074_000);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let frames = feed.frames();
            if frames.iter().any(|f| f.center_hz == 7_074_000) {
                return;
            }
            if std::time::Instant::now() > deadline {
                panic!(
                    "the dial moved and the window did not: newest centre {:?}",
                    frames.first().map(|f| f.center_hz)
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn dropping_the_feed_stops_its_thread() {
        let feed = SpectrumFeed::start(FakeSpectrumSource::new(), 14_074_000);
        let frames = Arc::clone(&feed.frames);
        drop(feed);
        std::thread::sleep(std::time::Duration::from_millis(100));
        let before = frames.lock().unwrap().len();
        std::thread::sleep(std::time::Duration::from_millis(150));
        let after = frames.lock().unwrap().len();
        assert_eq!(before, after, "the thread kept producing after the drop");
    }
}

#[cfg(test)]
mod remote_tests {
    use super::*;
    use cat_signal::DeviceKind;

    #[test]
    fn a_network_console_offers_no_local_devices() {
        // The bug this constructor exists to prevent: a console that does
        // not own the radio must not offer this machine's hardware as
        // though it were the radio's. An operator on a laptop picking their
        // laptop microphone as the radio's receive audio would look
        // entirely correct on screen.
        let sources = ConsoleSources::remote();
        for list in &sources.devices {
            assert!(
                list.devices.is_empty(),
                "{:?} offered local hardware",
                list.kind
            );
        }
    }

    #[test]
    fn it_says_it_cannot_ask_rather_than_that_there_is_nothing() {
        // An empty list is a claim about the radio's host that a console on
        // the far end of a socket is in no position to make.
        for list in &ConsoleSources::remote().devices {
            assert!(!list.is_available(), "{:?}", list.kind);
            let why = list.error.as_deref().unwrap_or_default();
            assert!(why.contains("another host"), "{why}");
        }
    }

    #[test]
    fn both_kinds_are_still_named() {
        // The groups stay on screen. A section that vanished would leave an
        // operator wondering whether the console supports that hardware at
        // all, rather than knowing it cannot ask about it from here.
        let kinds: Vec<DeviceKind> = ConsoleSources::remote()
            .devices
            .iter()
            .map(|l| l.kind)
            .collect();
        assert!(kinds.contains(&DeviceKind::AudioInput));
        assert!(kinds.contains(&DeviceKind::Sdr));
    }

    #[test]
    fn nothing_can_be_attached_from_a_network_console() {
        // Not merely "the list is empty": there is no attach function, so
        // even a stale selection cannot open local hardware.
        assert!(ConsoleSources::remote().attach.is_none());
    }
}
