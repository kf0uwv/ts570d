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

//! The terminal console, speaking the native protocol.
//!
//! # Why this exists
//!
//! `--server` used to reach a `ts570d server` over raw CAT: a Kenwood byte
//! pipe. It carried frequency and mode and nothing else — no capabilities,
//! no spectrum, and no way to ask what the radio's host has attached. The
//! SOURCE tab could only report, honestly, that it could not ask.
//!
//! The graphical console has had all of that since it existed, over the
//! same protocol on the same port. The gap was never a missing feature; it
//! was the terminal console pointed at the wrong protocol.
//!
//! # A cache, not a round trip
//!
//! Every getter here answers from the last `State` the server sent, and
//! the state is refreshed once per poll cycle. The console asks for about
//! forty fields per redraw; forty round trips at ten redraws a second
//! would be four hundred requests a second to render a screen that changes
//! a few times a minute.
//!
//! `ReadState` exists precisely so a console can have the dial, the mode
//! and the meters **from one moment**. Reading them field by field would
//! let a readout show a frequency from one instant beside a mode from
//! another, describing a radio that never existed.
//!
//! # Most of the `Radio` trait is deliberately not implemented
//!
//! A TS-570D has keyer speed, CTCSS tones, a speech processor and dozens
//! more controls, and the native protocol carries none of them. Those
//! methods keep the trait's default `NotImplemented`, which the console
//! skips rather than reporting (see `ui`'s `poll!`). The alternative —
//! guessing, or reporting a default as though it had been read — would put
//! numbers on screen that no radio ever sent.

use std::sync::Arc;

use cat_native::{Client, Command, Event, MeterKind, ServerMessage, Streams};
use radio::{Frequency, InformationResponse, Mode, Radio, RadioError, RadioResult};
use ui::feeds::{Attached, AudioFeed, AudioTap, ConsoleSources, SpectrumFeed};

/// How often the console asks the server what the radio is doing.
///
/// Ten times a second: fast enough that a dial turned elsewhere shows up
/// without a visible lag, slow enough that state traffic never crowds out
/// the spectrum frames sharing the socket. This is ADR 0011's two-rate
/// discipline, from the console's side.
const STATE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);

/// A [`Radio`] backed by a native-protocol connection.
pub struct NativeConsoleRadio {
    client: Client,
    /// The last state the server sent, or `None` before the first one.
    ///
    /// `None` is a real condition, not an error: a console that has just
    /// connected has not been told anything yet, and every getter reports
    /// that as `NotReady` so the screen shows em dashes rather than zeros.
    state: Option<cat_native::RadioState>,
    /// What the radio's host says it can see, once the server answers.
    ///
    /// Shared with the console's `ConsoleSources`, because the answer
    /// arrives a round trip after the console has already started drawing
    /// and a snapshot taken at startup would be empty forever.
    devices: ui::feeds::DeviceFeed,
    /// Whether an answer has been received at all, as against an answer
    /// that was empty. Kept here rather than inferred from the shared
    /// list, which cannot tell "not yet" from "nothing".
    devices_answered: bool,
    /// Whether a device question is outstanding.
    ///
    /// Replies are not correlated to requests on this protocol, so an
    /// `Unsupported` can only be attributed to the device question by
    /// knowing one is in flight. This console asks once per connection.
    devices_pending: bool,
    /// Set when the server declines device selection, so the console can
    /// say so instead of showing an empty list.
    devices_declined: bool,
    /// When state was last asked for, so a redraw's worth of getters
    /// costs one round trip rather than one each.
    last_request: std::time::Instant,
    /// The last error the server sent, surfaced through the next poll.
    fault: Option<String>,
    /// Where unsolicited messages from the server go, for the console to
    /// show. An attach refused two seconds after the fact has no getter's
    /// return value to travel in.
    notices: ui::feeds::Notices,
}

impl NativeConsoleRadio {
    /// Connect and handshake.
    pub fn connect(addr: &str) -> Result<Self, String> {
        // Spectrum is requested up front: a console's waterfall is one of
        // the two reasons to prefer this protocol, and a client that
        // declined would have to reconnect to change its mind.
        let client = Client::connect(addr, Streams::all())
            .map_err(|e| format!("could not reach the console protocol at {addr}: {e}"))?;
        Ok(Self {
            client,
            state: None,
            devices: ui::feeds::DeviceFeed::default(),
            devices_answered: false,
            devices_pending: false,
            devices_declined: false,
            // Far enough in the past that the first getter asks
            // immediately, rather than showing em dashes for the first
            // tenth of a second after connecting.
            last_request: std::time::Instant::now() - STATE_INTERVAL,
            fault: None,
            notices: ui::feeds::Notices::default(),
        })
    }

    /// What the radio is, as the server published it at handshake.
    pub fn capabilities(&self) -> &cat_native::CapabilitiesWire {
        self.client.capabilities()
    }

    /// The slot audio frames land in, for a feed on another thread.
    pub fn audio_slot(&self) -> Arc<std::sync::Mutex<Option<cat_signal::AudioFrame>>> {
        self.client.audio_slot()
    }

    /// The slot spectrum frames land in, for a feed on another thread.
    pub fn spectrum_slot(&self) -> Arc<std::sync::Mutex<Option<cat_signal::SpectrumFrame>>> {
        self.client.spectrum_slot()
    }

    /// Ask the radio's host what it has, once.
    pub fn ask_for_devices(&mut self) {
        if self.devices_pending || self.devices_answered || self.devices_declined {
            return;
        }
        if self.client.send(Command::ReadDevices) {
            self.devices_pending = true;
        }
    }

    /// Drain the reader thread, and ask for a fresh state if it is time.
    ///
    /// Called at the top of every getter rather than from a hook, because
    /// the `Radio` trait has none: it is a set of questions, and a console
    /// asks them in whatever order it draws. Draining on each is cheap and
    /// idempotent; the *request* is rate-limited, so forty getters in one
    /// redraw produce one round trip and not forty.
    ///
    /// This is what makes a whole redraw come from one moment. The state
    /// arrives as a single `State` message, so every field a console reads
    /// between two of them agrees with every other -- no readout showing a
    /// frequency from one instant beside a mode from another.
    fn tick(&mut self) {
        self.drain();
        if self.last_request.elapsed() >= STATE_INTERVAL {
            self.last_request = std::time::Instant::now();
            self.client.request_state();
            self.ask_for_devices();
        }
    }

    /// Take everything the reader thread has, without blocking.
    fn drain(&mut self) {
        while let Some(event) = self.client.try_event() {
            match event {
                Event::Reply(ServerMessage::State(state)) => self.state = Some(*state),
                Event::Reply(ServerMessage::Devices { lists }) => {
                    if let Ok(mut slot) = self.devices.lock() {
                        *slot = lists;
                    }
                    self.devices_answered = true;
                    self.devices_pending = false;
                }
                Event::Reply(ServerMessage::Error { code, message }) => {
                    // A server that declines device selection, or one too
                    // old to parse the command, is answering the question
                    // rather than failing. It belongs in the SOURCE tab,
                    // not in the console's error line.
                    if self.devices_pending
                        && matches!(
                            code,
                            cat_native::ErrorCode::Unsupported | cat_native::ErrorCode::Malformed
                        )
                    {
                        self.devices_declined = true;
                        self.devices_pending = false;
                        // Said, not left blank. An empty SOURCE tab would
                        // read as "this radio has nothing attached",
                        // which is a claim about the radio's host that a
                        // server declining the question has not made.
                        self.note(
                            "this server does not offer device selection -- set its \
                             sources with --if-out on the radio's machine",
                        );
                    } else if !matches!(code, cat_native::ErrorCode::NotReady) {
                        // The host's own words reach the operator: "device
                        // or resource busy" names the program holding the
                        // dongle, which a tidied message would not.
                        self.note(message.clone());
                        // `NotReady` is the ordinary answer to asking for
                        // a TX meter during receive, and to asking for
                        // state before the server has heard from the
                        // radio. Neither is worth an error line.
                        self.fault = Some(format!("{code:?}: {message}"));
                    }
                }
                Event::Reply(_) => {}
                Event::Disconnected(why) => {
                    self.fault = Some(format!("connection lost: {why}"));
                    self.state = None;
                    return;
                }
            }
        }
    }

    /// The last published state, or the reason there isn't one.
    ///
    /// Every getter goes through here, which is also what drives the
    /// connection -- see [`NativeConsoleRadio::tick`].
    fn state(&mut self) -> RadioResult<&cat_native::RadioState> {
        self.tick();
        self.published()
    }

    fn published(&self) -> RadioResult<&cat_native::RadioState> {
        match (&self.state, &self.fault) {
            (Some(state), _) => Ok(state),
            (None, Some(why)) => Err(RadioError::Link(why.clone())),
            (None, None) => Err(RadioError::Link(
                "the server has not reported the radio's state yet".to_string(),
            )),
        }
    }
}

impl NativeConsoleRadio {
    /// The occasional settings, if the server has read them yet.
    fn levels(&self) -> RadioResult<cat_native::RadioLevels> {
        self.state
            .as_ref()
            .and_then(|s| s.levels)
            .ok_or(RadioError::NotImplemented)
    }
}

#[async_trait::async_trait(?Send)]
impl Radio for NativeConsoleRadio {
    async fn get_vfo_a(&mut self) -> RadioResult<Frequency> {
        Frequency::new(self.state()?.vfo_a_hz)
    }

    async fn set_vfo_a(&mut self, freq: Frequency) -> RadioResult<()> {
        self.send(Command::SetFrequency {
            vfo: 0,
            hz: freq.hz(),
        })
    }

    async fn get_vfo_b(&mut self) -> RadioResult<Frequency> {
        Frequency::new(self.state()?.vfo_b_hz)
    }

    async fn set_vfo_b(&mut self, freq: Frequency) -> RadioResult<()> {
        self.send(Command::SetFrequency {
            vfo: 1,
            hz: freq.hz(),
        })
    }

    async fn get_mode(&mut self) -> RadioResult<Mode> {
        let id = self.state()?.mode;
        radio::capabilities::to_mode(id).ok_or_else(|| {
            RadioError::Link(format!(
                "the server reported a {id:?} mode, which this radio does not have"
            ))
        })
    }

    async fn set_mode(&mut self, mode: Mode) -> RadioResult<()> {
        self.send(Command::SetMode {
            mode: radio::capabilities::from_mode(mode),
        })
    }

    async fn get_meter_reading(&mut self) -> RadioResult<(u8, u16)> {
        // Served from the published state rather than asked for: the
        // server reads `RM;` while the radio is keyed and sends the
        // sample along with everything else. Reported as the selector the
        // sample's kind implies, so the console can label the row the
        // same way whichever end read it.
        let state = self.state()?;
        for (selector, kind) in [
            (1u8, MeterKind::Swr),
            (2, MeterKind::Comp),
            (3, MeterKind::Alc),
        ] {
            if let Some(raw) = state.meter(kind) {
                return Ok((selector, raw));
            }
        }
        Err(RadioError::NotImplemented)
    }

    async fn get_smeter(&mut self) -> RadioResult<u16> {
        // Whichever meter `SM;` was answering when the server read it.
        //
        // On this radio that command is two meters: the S-meter while
        // receiving, and -- per the manual -- "a calibrated power meter"
        // while transmitting. The server labels the sample with the state
        // it was taken in. Asking only for `MeterKind::S` here would make
        // this read fail for the whole length of a transmission, and the
        // console would hold its last receive reading and go on drawing
        // it as though the radio were still listening.
        //
        // The console labels the row from its own `tx` flag, read in the
        // same cycle, so the value and its meaning stay together.
        let state = self.state()?;
        state
            .meter(MeterKind::S)
            .or_else(|| state.meter(MeterKind::Po))
            .ok_or(RadioError::NotImplemented)
    }

    async fn get_information(&mut self) -> RadioResult<InformationResponse> {
        let state = self.state()?;
        let mode = radio::capabilities::to_mode(state.mode).ok_or_else(|| {
            RadioError::Link(format!(
                "the server reported a {:?} mode, which this radio does not have",
                state.mode
            ))
        })?;
        Ok(InformationResponse {
            frequency: Frequency::new(state.vfo_a_hz)?,
            mode,
            tx_rx: state.transmitting,
            split: state.split,
            memory_channel: state.memory_channel.unwrap_or(0) as u8,
            vfo_memory: u8::from(state.memory_channel.is_some()),
            // Not carried by the protocol. Reported as the neutral value
            // rather than guessed: an `IF` response is a fixed-width
            // record and every field has to say something, so the honest
            // choice is the one that reads as "off" rather than one that
            // invents an offset the radio never had.
            step: 0,
            rit_xit_offset: 0,
            rit_enabled: false,
            xit_enabled: false,
            memory_bank: 0,
            scan_status: 0,
            ctcss_tone: 0,
            tone_number: 0,
        })
    }

    // ── the occasional settings ────────────────────────────────────
    //
    // These used to report `NotImplemented`, because `RadioState` did not
    // carry them. The console skipped the failed poll and kept whatever
    // `RadioDisplay::default()` held -- so a network console displayed
    // `AF 200` at a radio reading `AG034`, and `PRE off` at a radio with
    // its preamp on, confidently and indistinguishably from a reading.
    //
    // The protocol carries them now, read on the server's slow clock.
    // Still `NotImplemented` until the first slow poll has landed, which
    // is the honest answer for "nobody has read this yet".

    async fn get_af_gain(&mut self) -> RadioResult<u8> {
        self.levels().map(|l| l.af_gain)
    }

    async fn get_rf_gain(&mut self) -> RadioResult<u8> {
        self.levels().map(|l| l.rf_gain)
    }

    async fn get_squelch(&mut self) -> RadioResult<u8> {
        self.levels().map(|l| l.squelch)
    }

    async fn get_mic_gain(&mut self) -> RadioResult<u8> {
        self.levels().map(|l| l.mic_gain)
    }

    async fn get_power(&mut self) -> RadioResult<u8> {
        self.levels().map(|l| l.power_pct)
    }

    async fn get_agc(&mut self) -> RadioResult<u8> {
        self.levels().map(|l| l.agc)
    }

    async fn get_noise_reduction(&mut self) -> RadioResult<u8> {
        self.levels().map(|l| l.noise_reduction)
    }

    async fn get_antenna(&mut self) -> RadioResult<u8> {
        self.levels().map(|l| l.antenna)
    }

    async fn get_noise_blanker(&mut self) -> RadioResult<bool> {
        self.levels().map(|l| l.noise_blanker)
    }

    async fn get_preamp(&mut self) -> RadioResult<bool> {
        self.levels().map(|l| l.preamp)
    }

    async fn get_attenuator(&mut self) -> RadioResult<bool> {
        self.levels().map(|l| l.attenuator)
    }

    async fn get_speech_processor(&mut self) -> RadioResult<bool> {
        self.levels().map(|l| l.speech_processor)
    }

    async fn get_vox(&mut self) -> RadioResult<bool> {
        self.levels().map(|l| l.vox)
    }

    async fn get_frequency_lock(&mut self) -> RadioResult<bool> {
        self.levels().map(|l| l.freq_lock)
    }

    /// This radio reports IF shift as a direction and a magnitude, where
    /// the protocol carries one signed number. Splitting it here rather
    /// than teaching the protocol about `char` keeps the wire format
    /// radio-independent, which is the whole point of it.
    async fn get_if_shift(&mut self) -> RadioResult<(char, u16)> {
        let hz = self
            .state()?
            .if_shift_hz
            .ok_or(RadioError::NotImplemented)?;
        let direction = if hz < 0 { 'D' } else { 'U' };
        let magnitude = u16::try_from(hz.unsigned_abs())
            .map_err(|_| RadioError::Link(format!("{hz} Hz is not an IF shift this radio has")))?;
        Ok((direction, magnitude))
    }

    async fn set_if_shift(&mut self, direction: char, freq: u16) -> RadioResult<()> {
        let magnitude = i32::from(freq);
        let hz = match direction {
            'D' | 'd' => -magnitude,
            'U' | 'u' => magnitude,
            other => {
                return Err(RadioError::Link(format!(
                    "unknown IF shift direction {other:?}"
                )))
            }
        };
        self.send(Command::SetIfShift { hz })
    }

    async fn get_tx_vfo(&mut self) -> RadioResult<u8> {
        Ok(u8::from(self.state()?.split))
    }

    async fn set_tx_vfo(&mut self, vfo: u8) -> RadioResult<()> {
        // On this radio split *is* which VFO transmits, which is exactly
        // what the protocol's `SetSplit` means.
        self.send(Command::SetSplit { enabled: vfo != 0 })
    }

    async fn get_memory_channel(&mut self) -> RadioResult<u8> {
        self.state()?
            .memory_channel
            .map(|c| c as u8)
            .ok_or(RadioError::NotImplemented)
    }

    async fn set_memory_channel(&mut self, channel: u8) -> RadioResult<()> {
        self.send(Command::SetMemoryChannel {
            channel: u16::from(channel),
        })
    }
}

impl NativeConsoleRadio {
    /// Queue a command, reporting a dead link rather than dropping it.
    ///
    /// Fire and forget by design: the server answers with `Ack` or an
    /// `Error`, both of which arrive on the next `refresh`. Blocking here
    /// for the reply would stall the console's redraw on the network for
    /// every knob turn.
    fn send(&mut self, command: Command) -> RadioResult<()> {
        if self.client.send(command) {
            Ok(())
        } else {
            Err(RadioError::Link(
                "the connection to the radio is gone".to_string(),
            ))
        }
    }
}

impl NativeConsoleRadio {
    /// Queue something for the console to show next time it draws.
    fn note(&self, message: impl Into<String>) {
        if let Ok(mut queue) = self.notices.lock() {
            queue.push(message.into());
        }
    }
}

/// The console's sources, for a radio on the far end of a socket.
///
/// Everything here comes from the **server**. This console never looks at
/// its own machine: the sound card in this laptop is not the radio's ACC2
/// audio, and offering it would look entirely correct and be wrong.
pub fn sources(radio: &mut NativeConsoleRadio) -> ConsoleSources {
    let sink = radio.client.sink();
    ConsoleSources {
        // Frames arrive on the connection the radio poll is already using,
        // and are relayed from its slot by a feed thread. The FFT was
        // computed on the radio's machine, where the dongle is.
        spectrum: Some(SpectrumFeed::start(
            RemoteSpectrum::new(
                radio.spectrum_slot(),
                remote_capability(radio.capabilities()),
            ),
            0,
        )),
        // The ACC2 pair, computed on the radio's machine and relayed
        // whole: the scope trace and the AF spectrum arrive together with
        // one sequence number, so the two panels cannot disagree about
        // what the radio was doing.
        audio: Some(AudioFeed::new(RemoteAudio::new(radio.audio_slot()))),
        // Empty only until the server answers; `device_feed` is what the
        // console actually reads after that.
        devices: Vec::new(),
        device_feed: Some(Arc::clone(&radio.devices)),
        notices: Some(Arc::clone(&radio.notices)),
        // The server's, not this machine's: it carries the layout that
        // server authored for its radio, which is the whole reason a
        // console asks rather than assuming.
        capabilities: Some(radio.capabilities().clone()),
        refresh_devices: {
            let sink = radio.client.sink();
            Some(Box::new(move || {
                // Straight down the sink, not through the adapter's
                // ask-once guard: the guard exists to stop a redraw
                // asking forty times, and this is an operator asking
                // deliberately.
                sink.send(Command::ReadDevices);
            }))
        },
        attach: Some(Box::new(move |device: &cat_signal::DeviceInfo| {
            if sink.send(Command::AttachDevice {
                kind: device.kind,
                spec: device.spec.clone(),
            }) {
                // `Remote`: the device is opened on the radio's machine,
                // so there is nothing to install here. Whether it worked
                // is the server's to say, and its answer arrives as a
                // notice rather than as this function's return value.
                Ok(Attached::Remote)
            } else {
                Err("the connection to the radio is gone".to_string())
            }
        })),
    }
}

/// Spectrum frames that were computed somewhere else.
///
/// A [`cat_signal::SpectrumSource`] that does no signal processing: the
/// FFT ran on the radio's machine, where the dongle is, and this relays
/// the finished frames into the console's feed so that a remote waterfall
/// and a local one take the same path through the console.
struct RemoteSpectrum {
    slot: Arc<std::sync::Mutex<Option<cat_signal::SpectrumFrame>>>,
    /// What the server said its source is.
    capability: cat_signal::SignalCapability,
    /// The last frame handed on, so a source slower than this poll loop
    /// is not reported repeatedly as though it were live.
    last_sequence: Option<u64>,
}

impl RemoteSpectrum {
    fn new(
        slot: Arc<std::sync::Mutex<Option<cat_signal::SpectrumFrame>>>,
        capability: cat_signal::SignalCapability,
    ) -> Self {
        Self {
            slot,
            capability,
            last_sequence: None,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl cat_signal::SpectrumSource for RemoteSpectrum {
    type Error = String;

    async fn next_frame(&mut self) -> Result<cat_signal::SpectrumFrame, Self::Error> {
        loop {
            let frame = self.slot.lock().ok().and_then(|mut g| g.take());
            if let Some(frame) = frame {
                if self.last_sequence != Some(frame.sequence) {
                    self.last_sequence = Some(frame.sequence);
                    return Ok(frame);
                }
            }
            // Polling, because the slot is filled by the client's reader
            // thread and there is nothing here to await on. 10 ms is well
            // under the server's 30 ms pump, so no frame waits on this.
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn capability(&self) -> cat_signal::SignalCapability {
        self.capability
    }

    fn settings(&self) -> cat_signal::SpectrumSettings {
        cat_signal::SpectrumSettings::default()
    }

    fn apply(&mut self, key: &str, _value: cat_signal::SettingValue) -> Result<(), Self::Error> {
        Err(format!(
            "{key} belongs to the source on the radio's machine, and this protocol has no \
             way to set it from here yet"
        ))
    }

    /// Deliberately nothing.
    ///
    /// The server centres its own frames on its own dial — it reads the
    /// same state this console does. Re-centring here would apply the
    /// correction twice and put every signal at double its true offset.
    fn retune(&mut self, _dial_hz: u64) {}
}

/// What the server says its spectrum source is.
///
/// Read from the server's declaration rather than assumed, because this
/// console cannot see the hardware and the answer decides whether a
/// waterfall may be drawn at all. `AudioDerived` is a source and is *not*
/// a band panorama, and a console that guessed `IfTap` would draw one
/// across a few kHz of audio as though it were a band.
///
/// Falling back to the radio *model*'s declared tap point is the honest
/// second-best: the server is streaming frames, so it has a source, and
/// the model says what a source on this radio is. It is not a guess about
/// hardware — it is this radio's published circuitry (ADR 0015).
fn remote_capability(caps: &cat_native::CapabilitiesWire) -> cat_signal::SignalCapability {
    if let Some(source) = caps.installation.band_panorama() {
        return source.capability;
    }
    match caps.signal {
        radio::capabilities::SignalSupport::IfTapPoint {
            if_center_hz,
            inverted,
        } => cat_signal::SignalCapability::IfTap(cat_signal::IfTapConfig {
            if_center_hz,
            inverted,
            // The station's crystal trim belongs to whoever owns the
            // dongle, and that is the server. Its frames are already
            // corrected; applying a trim again here would move a picture
            // that is already right.
            trim_hz: 0,
        }),
        _ => cat_signal::SignalCapability::None,
    }
}

/// Audio frames that were computed somewhere else.
///
/// The counterpart of [`RemoteSpectrum`]: the DSP ran on the machine with
/// the sound card in it, and this hands the finished pair to the console's
/// own [`AudioFeed`] so that remote audio and local audio take the same
/// path through the console.
struct RemoteAudio {
    slot: Arc<std::sync::Mutex<Option<cat_signal::AudioFrame>>>,
    /// The last sequence handed on. A capture that has stopped must not
    /// be redrawn as though it were live — a repeated scope trace reads
    /// as a steady tone, which is the opposite of what has happened.
    last_sequence: Option<u64>,
}

impl RemoteAudio {
    fn new(slot: Arc<std::sync::Mutex<Option<cat_signal::AudioFrame>>>) -> Self {
        Self {
            slot,
            last_sequence: None,
        }
    }
}

impl AudioTap for RemoteAudio {
    fn try_next_frame(
        &mut self,
    ) -> Result<Option<cat_signal::AudioFrame>, cat_signal_audio::AudioError> {
        // Never blocks, which is the whole contract: `AudioFeed::poll`
        // drains this at the console's redraw rate and stops at the first
        // `None`.
        let Some(frame) = self.slot.lock().ok().and_then(|mut g| g.take()) else {
            return Ok(None);
        };
        if self.last_sequence == Some(frame.sequence()) {
            return Ok(None);
        }
        self.last_sequence = Some(frame.sequence());
        Ok(Some(frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cat_native::testing::{serve_stub, StubHost};
    use cat_signal::{DeviceInfo, DeviceKind, DeviceList, SpectrumSource};

    fn radio_side_devices() -> Vec<DeviceList> {
        vec![DeviceList::found(
            DeviceKind::Sdr,
            vec![DeviceInfo {
                kind: DeviceKind::Sdr,
                spec: "rtl:in-the-shack".to_string(),
                label: "the dongle on the radio's IF tap".to_string(),
                detail: None,
                is_default: false,
            }],
        )]
    }

    fn connected(host: Arc<StubHost>) -> NativeConsoleRadio {
        NativeConsoleRadio::connect(&serve_stub(host)).expect("the stub server should accept")
    }

    /// Poll until `done`, the way the console's loop does.
    fn settle(radio: &mut NativeConsoleRadio, done: impl Fn(&NativeConsoleRadio) -> bool) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            radio.tick();
            if done(radio) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        futures::executor::block_on(f)
    }

    #[test]
    fn a_whole_redraw_reads_one_moment_of_the_radio() {
        // The reason `ReadState` exists. Reading field by field would let
        // a readout show a frequency from one instant beside a mode from
        // another, describing a radio that never existed.
        let mut radio = connected(StubHost::new());
        assert!(settle(&mut radio, |r| r.published().is_ok()));

        let info = block_on(radio.get_information()).expect("state has arrived");
        let dial = block_on(radio.get_vfo_a()).expect("same state");
        let mode = block_on(radio.get_mode()).expect("same state");

        assert_eq!(info.frequency, dial);
        assert_eq!(info.mode, mode);
    }

    #[test]
    fn a_field_the_protocol_does_not_carry_is_not_an_error() {
        // It reports `NotImplemented`, which the console skips rather than
        // showing. Anything else would put a permanent error banner over a
        // console that is working exactly as it can.
        let mut radio = connected(StubHost::new());
        assert!(settle(&mut radio, |r| r.published().is_ok()));

        for result in [
            block_on(radio.get_af_gain()),
            block_on(radio.get_rf_gain()),
            block_on(radio.get_keyer_speed()),
        ] {
            assert!(
                matches!(result, Err(RadioError::NotImplemented)),
                "expected NotImplemented, got {result:?}"
            );
        }
    }

    #[test]
    fn before_the_first_state_the_console_is_told_why_not_shown_zeros() {
        // A dial reading 0.000.000 is a claim about the radio. "Nothing
        // reported yet" is the truth, and the console draws em dashes.
        let radio = NativeConsoleRadio {
            client: connected(StubHost::new()).client,
            state: None,
            devices: ui::feeds::DeviceFeed::default(),
            devices_answered: false,
            devices_pending: false,
            devices_declined: false,
            last_request: std::time::Instant::now(),
            fault: None,
            notices: ui::feeds::Notices::default(),
        };
        match radio.published() {
            Err(RadioError::Link(why)) => assert!(why.contains("not reported"), "{why}"),
            other => panic!("expected a link-level reason, got {other:?}"),
        }
    }

    #[test]
    fn if_shift_survives_the_trip_through_a_signed_number() {
        // This radio says direction-and-magnitude; the protocol says one
        // signed number. A sign lost in the middle would move the passband
        // the wrong way, which looks plausible and is wrong.
        let mut radio = connected(StubHost::new());
        assert!(settle(&mut radio, |r| r.published().is_ok()));

        for (direction, magnitude) in [('U', 300u16), ('D', 300)] {
            block_on(radio.set_if_shift(direction, magnitude)).expect("queued");
        }

        // The stub reports 0, so read back the mapping itself rather than
        // the stub's echo: the direction for a negative shift must be down.
        radio.state = Some(cat_native::RadioState {
            if_shift_hz: Some(-300),
            ..radio.published().unwrap().clone()
        });
        assert_eq!(block_on(radio.get_if_shift()).unwrap(), ('D', 300));

        radio.state = Some(cat_native::RadioState {
            if_shift_hz: Some(300),
            ..radio.published().unwrap().clone()
        });
        assert_eq!(block_on(radio.get_if_shift()).unwrap(), ('U', 300));
    }

    #[test]
    fn the_device_list_is_the_servers_and_reaches_the_console() {
        let mut radio = connected(StubHost::offering(radio_side_devices()));
        assert!(
            settle(&mut radio, |r| r.devices_answered),
            "the server's device list never arrived"
        );

        let specs: Vec<String> = radio
            .devices
            .lock()
            .unwrap()
            .iter()
            .flat_map(|l| l.devices.iter().map(|d| d.spec.clone()))
            .collect();
        assert_eq!(specs, vec!["rtl:in-the-shack".to_string()]);
    }

    #[test]
    fn a_server_that_declines_says_so_rather_than_leaving_the_tab_blank() {
        // An empty SOURCE tab reads as "this radio has nothing attached",
        // which is a claim a declining server never made.
        let mut radio = connected(StubHost::new());
        assert!(settle(&mut radio, |r| r.devices_declined));

        let notices = radio.notices.lock().unwrap().clone();
        assert!(
            notices.iter().any(|n| n.contains("does not offer")),
            "{notices:?}"
        );
        assert!(
            radio.devices.lock().unwrap().is_empty(),
            "a decline must not be dressed up as a list"
        );
    }

    #[test]
    fn the_console_asks_for_state_at_its_own_rate_not_per_getter() {
        // Forty getters in one redraw must be one round trip. The guard is
        // the clock, not the call site, because the trait has no hook and
        // a console asks in whatever order it draws.
        let mut radio = connected(StubHost::new());
        assert!(settle(&mut radio, |r| r.published().is_ok()));

        let before = radio.last_request;
        for _ in 0..40 {
            let _ = block_on(radio.get_vfo_a());
        }
        assert_eq!(
            radio.last_request, before,
            "a redraw's worth of getters asked the server more than once"
        );
    }

    #[test]
    fn a_remote_waterfall_is_not_recentred_a_second_time() {
        // The server centres its frames on the same dial this console
        // reads. Re-centring here would apply the correction twice and put
        // every signal at double its true offset -- a plausible-looking
        // picture that is wrong everywhere except at the centre.
        let slot = Arc::new(std::sync::Mutex::new(None));
        let mut source = RemoteSpectrum::new(Arc::clone(&slot), cat_signal::SignalCapability::None);

        *slot.lock().unwrap() = Some(cat_signal::SpectrumFrame {
            center_hz: 14_074_000,
            span_hz: 240_000,
            ref_level_dbm: -40.0,
            bins: vec![-100.0; 8],
            sequence: 1,
        });
        source.retune(21_074_000);

        let frame = block_on(source.next_frame()).unwrap();
        assert_eq!(
            frame.center_hz, 14_074_000,
            "the frame was re-centred on this side as well as the server's, so every \
             signal would sit at double its true offset"
        );
        assert_eq!(frame.span_hz, 240_000);
    }

    #[test]
    fn a_frame_is_handed_on_once() {
        // A source slower than the console's poll must not be reported
        // repeatedly: a stalled dongle would look live.
        let slot = Arc::new(std::sync::Mutex::new(None));
        let mut source = RemoteSpectrum::new(Arc::clone(&slot), cat_signal::SignalCapability::None);

        let frame = cat_signal::SpectrumFrame {
            center_hz: 14_074_000,
            span_hz: 240_000,
            ref_level_dbm: -40.0,
            bins: vec![-100.0; 8],
            sequence: 7,
        };
        *slot.lock().unwrap() = Some(frame.clone());
        assert_eq!(block_on(source.next_frame()).unwrap().sequence, 7);

        // The same frame put back must not be handed on again.
        *slot.lock().unwrap() = Some(frame);
        let second = std::thread::spawn(move || {
            futures::executor::block_on(async {
                futures::future::select(
                    Box::pin(source.next_frame()),
                    Box::pin(async {
                        std::thread::sleep(std::time::Duration::from_millis(150));
                    }),
                )
                .await
            });
        });
        std::thread::sleep(std::time::Duration::from_millis(250));
        assert!(
            !second.is_finished(),
            "the same frame was handed on twice, so a stalled source would look live"
        );
    }

    fn audio_frame(sequence: u64) -> cat_signal::AudioFrame {
        cat_signal::AudioFrame {
            scope: cat_signal::AudioScopeFrame {
                sample_rate_hz: 48_000,
                samples: vec![0.1, -0.2, 0.3],
                sequence,
            },
            spectrum: cat_signal::AudioSpectrumFrame {
                start_hz: 0,
                span_hz: 4_000,
                bins: vec![-90.0, -60.0, -80.0],
                sequence,
            },
        }
    }

    #[test]
    fn a_stalled_remote_capture_is_not_redrawn_as_though_it_were_live() {
        // The one thing an AF scope must not do. An operator watches it
        // precisely to see whether audio is moving, so a repeated trace
        // reads as a steady tone -- the opposite of what has happened.
        let slot = Arc::new(std::sync::Mutex::new(None));
        let mut tap = RemoteAudio::new(Arc::clone(&slot));

        *slot.lock().unwrap() = Some(audio_frame(3));
        assert_eq!(tap.try_next_frame().unwrap().map(|f| f.sequence()), Some(3));

        *slot.lock().unwrap() = Some(audio_frame(3));
        assert_eq!(
            tap.try_next_frame().unwrap(),
            None,
            "the same block was handed on twice"
        );

        *slot.lock().unwrap() = Some(audio_frame(4));
        assert_eq!(tap.try_next_frame().unwrap().map(|f| f.sequence()), Some(4));
    }

    #[test]
    fn an_empty_slot_is_not_an_error() {
        // `AudioFeed::poll` drains until the first `None` and treats an
        // `Err` as the stream having stopped for good. Reporting "nothing
        // yet" as a fault would put the AF panels into a terminal error
        // state on the first idle redraw.
        let mut tap = RemoteAudio::new(Arc::new(std::sync::Mutex::new(None)));
        assert!(matches!(tap.try_next_frame(), Ok(None)));
    }

    #[test]
    fn the_two_halves_of_a_remote_frame_stay_together() {
        // What the pair is for: a console checks the trace against the
        // spectrum by this number, and nothing on the way here may split
        // them.
        let slot = Arc::new(std::sync::Mutex::new(Some(audio_frame(9))));
        let mut tap = RemoteAudio::new(slot);
        let frame = tap.try_next_frame().unwrap().unwrap();
        assert_eq!(frame.scope.sequence, frame.spectrum.sequence);
    }

    #[test]
    fn the_remote_source_capability_comes_from_the_server_not_a_guess() {
        // Whether a waterfall may be drawn at all turns on this. An
        // `AudioDerived` source rendered as a band panorama would draw a
        // few kHz of audio across a whole band's worth of screen.
        let caps = cat_native::testing::stub_capabilities();
        match remote_capability(&caps) {
            cat_signal::SignalCapability::IfTap(tap) => {
                assert_eq!(tap.if_center_hz, 73_050_000);
                assert!(tap.inverted);
                assert_eq!(tap.trim_hz, 0, "the station's trim belongs to the server");
            }
            other => panic!("expected the model's declared tap, got {other:?}"),
        }
    }
}
