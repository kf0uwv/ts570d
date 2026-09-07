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

//! TS-570D Radio Control Application
//!
//! Main entry point: parse `--cat-only <port>`, `--cat-dtr <port>` or
//! `--server <host:port>` from argv, open the chosen transport, create a
//! typed Ts570d client, and run the ratatui UI.
//!
//! # The flag names the wiring, the argument names the endpoint
//!
//! `--cat-only` and `--cat-dtr` are the same transport choice and a
//! different **station**: whether this radio's PTT is keyed from the serial
//! DTR line. That is not something software can detect — a port has a DTR
//! pin either way, and whether anything is wired to it is a fact about the
//! shack — so the operator says which, once, on the command line.
//!
//! Either flag takes a local device (`/dev/ttyUSB0`, `COM3`) **or** a
//! `host:port` on an RFC 2217 device server (`ser2net`, a Moxa box, this
//! repo's emulator run with `--com`). The transport follows from the shape
//! of the argument — see [`endpoint`] — so the same flag reaches a radio on
//! the desk and a radio on the network, and the console above it cannot
//! tell the difference.
//!
//! The emulator (if needed) runs as a **separate process**:
//!   cargo run --bin emulator
//! It prints the PTY slave path to stdout; pass that path here via --port.
//!
//! # Platforms
//!
//! Linux uses `#[monoio::main]` (io_uring). Windows has no `monoio` at all
//! (io_uring is a Linux kernel interface) and drives the same `run_app()`
//! future via a hand-rolled thread-parking executor instead — see
//! `docs/adr/0006-windows-concurrency-model.md` and `win_runtime.rs`.

#[cfg(target_os = "windows")]
mod win_runtime;

mod calibrate;

#[path = "endpoint.rs"]
mod endpoint;

/// The console, speaking the native protocol to a `ts570d server`.
#[cfg(target_os = "linux")]
mod native_console;

use tracing::info;

use cat_signal::IfTapConfig;
use cat_signal_audio::{AudioPipelineConfig, AudioStream};
use cat_transport_core::{CatSession, ResponseDisposition, TransportError};
use cat_transport_rfc2217::{Rfc2217Config, Rfc2217Port};
use cat_transport_serial::{SerialCatSession, SerialConfig, SerialPort};
use cat_transport_tcp::{TcpCatSession, TcpSessionError};
use radio::Ts570d;
use ui::feeds::{Attached, AudioFeed, ConsoleSources, SpectrumFeed};

/// Which transport to open for the local TUI (`--port` or `--server`,
/// mutually exclusive — see [`parse_args`]).
enum Transport {
    /// `--cat-only <port>` or `--cat-dtr <port>`: own a serial port, local
    /// or on a device server. `keys_ptt` is the difference between the two
    /// flags, and it is a statement about the shack rather than about the
    /// hardware.
    Serial {
        port: String,
        baud: u32,
        stop_bits: u8,
        keys_ptt: bool,
    },
    /// `--server <host:port>`: attach to a remote `ts570d server` over the
    /// **native console protocol** — its `--console-port`.
    ///
    /// The protocol a console should speak. It carries the radio's
    /// capabilities, its whole state in one round trip, spectrum frames,
    /// and the question "what does your machine have attached?" — none of
    /// which raw CAT can express.
    Server { addr: String },
    /// `--server-raw <host:port>`: the same idea over raw CAT, against a
    /// server's `--raw-tcp-port`.
    ///
    /// Kept because it is the only way to reach a raw listener, and
    /// removing it would break setups that point at one. It is not the
    /// better choice for a console: over a Kenwood byte pipe there are no
    /// capabilities to read, no spectrum, and no way to ask what the
    /// radio's host has attached.
    ServerRaw { addr: String },
}

/// Parse an `--if-trim` value: signed Hz, calibrated against a known
/// carrier.
///
/// Its own function because it is the one CLI value with a sign that
/// matters and no natural bound. A trim is a small correction -- this
/// bench measures -115 Hz -- so a figure in the tens of kHz is a typo or
/// a units mix-up, and half a band away is not a calibration.
fn parse_trim_hz(value: &str) -> Result<i32, String> {
    let hz: i32 = value
        .parse()
        .map_err(|_| format!("--if-trim wants a whole number of Hz, got {value:?}"))?;
    if hz.abs() > MAX_TRIM_HZ {
        return Err(format!(
            "--if-trim {hz} Hz is beyond +/-{MAX_TRIM_HZ}; that is not a calibration"
        ));
    }
    Ok(hz)
}

/// As far from the IF as a station calibration can plausibly be.
///
/// The correction absorbs a dongle's crystal error and the radio's own
/// deviation from a nominal 73.05 MHz. Both are small. A wider bound
/// would accept a typo that silently mis-labels the whole waterfall.
const MAX_TRIM_HZ: i32 = 50_000;

/// Parsed command-line arguments for the local TUI.
struct Args {
    transport: Transport,
    /// `--if-out <endpoint>`: the radio's IF output -- on a TS-570D, the
    /// CN4 header. Either an `rtl_tcp` server (`host:port`) or a local
    /// dongle (`rtl:<index>`).
    if_out: Option<String>,
    /// `--acc2-audio <endpoint>`: the ACC2 receive-audio pair. Either a
    /// PCM server (`host:port`) or a local sound device.
    acc2_audio: Option<String>,
    /// `--if-trim <hz>`: this station's IF calibration. See
    /// [`parse_trim_hz`].
    if_trim: i32,
}

/// The TS-570D's first IF, and what its IF output needs corrected.
///
/// **The whole of this program's contribution to opening an SDR.** Sample
/// rate, bin count, how a dongle is named, how the pieces assemble --
/// `cat-signal-rtlsdr` owns all of it, because all of it is a fact about
/// the dongle. What only this program knows is the number below: a
/// TS-570D's first IF is 73.05 MHz, its LO1 is high-side so the tapped
/// spectrum arrives mirrored, and `trim_hz` is a per-station calibration
/// against a known carrier, left at zero until somebody measures it.
///
/// On this radio the IF output is the **CN4** header; `--if-out` names the
/// signal rather than the connector, because the designation is a TS-570D
/// fact and the signal is what every radio with one has.
fn if_tap(trim_hz: i32) -> IfTapConfig {
    IfTapConfig {
        if_center_hz: 73_050_000,
        inverted: true,
        trim_hz,
    }
}

/// Where the tap is pointed until the first CAT poll says otherwise.
///
/// The spectrum feed is started before the radio has been asked anything,
/// so it needs a dial to centre on; the very next poll corrects it. The
/// TS-570D's own power-on frequency is the least surprising guess.
const INITIAL_DIAL_HZ: u64 = 14_000_000;

/// Open the signal sources the console was asked for.
///
/// Failing to open one is reported and **not fatal**: a radio with an
/// unplugged dongle is still a radio, and a console that refused to start
/// because its waterfall had no source would be worse than one that starts
/// and says so.
fn open_sources(
    if_out: Option<String>,
    acc2_audio: Option<String>,
    dial_hz: u64,
    trim_hz: i32,
) -> ConsoleSources {
    let mut sources = ConsoleSources::default();

    if let Some(spec) = if_out {
        // One call, and this program's only say in it is `IF_TAP`. The
        // spec may name a dongle on this machine or an rtl_tcp server; the
        // library decides which and how, and pins the tuner to the IF.
        match cat_signal_rtlsdr::open(
            &spec,
            if_tap(trim_hz),
            cat_signal_rtlsdr::IfSourceConfig::default(),
        ) {
            Ok(source) => {
                info!("IF output attached at {spec}");
                sources.spectrum = Some(SpectrumFeed::start(source, dial_hz));
            }
            Err(e) => eprintln!("warning: {e}"),
        }
    }

    if let Some(spec) = acc2_audio {
        match endpoint::classify(&spec) {
            endpoint::Endpoint::Network(addr) => {
                match AudioStream::connect(addr.as_str(), AudioPipelineConfig::default()) {
                    // The transmit half is dropped: this console does not
                    // send audio, and holding a handle open would keep the
                    // PKD direction alive for no reason.
                    Ok((stream, _tx)) => {
                        info!("ACC2 audio attached at {addr}");
                        sources.audio = Some(AudioFeed::new(stream));
                    }
                    Err(e) => eprintln!("warning: no ACC2 audio at {addr}: {e}"),
                }
            }
            endpoint::Endpoint::Device(dev) => match open_local_audio(&dev) {
                Ok(Attached::Audio(feed)) => sources.audio = Some(feed),
                // `open_local_audio` only ever produces audio; the arm
                // exists because `Attached` is shared with the SDR path.
                Ok(_) => eprintln!("warning: {dev:?} did not open as audio"),
                Err(e) => eprintln!("warning: {e}"),
            },
        }
    }

    // What this machine can see, asked once. The picker offers these; the
    // flags above remain the way to say it up front.
    sources.devices = enumerate_devices();
    // The same list, in the shared slot the console re-reads, so that
    // `r` on the SOURCE tab can replace it. A dongle plugged in after the
    // console started should not need the console restarted.
    let feed: ui::feeds::DeviceFeed =
        std::sync::Arc::new(std::sync::Mutex::new(sources.devices.clone()));
    sources.device_feed = Some(std::sync::Arc::clone(&feed));
    sources.refresh_devices = Some(Box::new(move || {
        if let Ok(mut slot) = feed.lock() {
            *slot = enumerate_devices();
        }
    }));
    sources.attach = Some(Box::new(move |device| attach(device, dial_hz, trim_hz)));

    sources
}

/// What this machine can see, in the order a picker should show it.
///
/// Each source crate enumerates its own kind, because doing so needs that
/// kind's driver and nothing else should have to link one. A kind whose
/// backend is not in this build reports *why* rather than reporting an
/// empty list -- "nothing plugged in" and "this build cannot look" send an
/// operator to opposite ends of the shack.
fn enumerate_devices() -> Vec<cat_signal::DeviceList> {
    vec![audio_devices(), sdr_devices()]
}

/// The sound cards this machine can capture from.
///
/// Not `cfg`-gated: `cat_signal_audio::input_devices()` exists in every
/// build and reports *why* it is empty when the backend is absent. A picker
/// is compiled once and has to be able to explain an empty list.
fn audio_devices() -> cat_signal::DeviceList {
    cat_signal_audio::input_devices()
}

/// Open a local sound card.
#[cfg(feature = "audio-device")]
fn open_local_audio(spec: &str) -> Result<Attached, String> {
    let capture =
        cat_signal_audio::AudioCapture::open(spec, cat_signal_audio::CaptureConfig::default())
            .map_err(|e| format!("could not open {spec}: {e}"))?;
    // The negotiated format, not the requested one: a card that only does
    // 44.1 kHz is used at 44.1 kHz, and saying so is the difference between
    // a console that is right and one that puts a 1000 Hz note at 1088.
    info!(
        "ACC2 audio attached to {} ({} Hz, {} ch, using ch {})",
        capture.label(),
        capture.format().sample_rate_hz,
        capture.format().channels,
        capture.format().channel
    );
    Ok(Attached::Audio(AudioFeed::new(capture)))
}

/// The same in a build without the feature: say what to do instead.
#[cfg(not(feature = "audio-device"))]
fn open_local_audio(spec: &str) -> Result<Attached, String> {
    Err(format!(
        "this build cannot open the sound device {spec:?} -- rebuild with \
         `--features audio-device` (which needs the platform's sound headers), or \
         point --acc2-audio at a PCM server instead"
    ))
}

#[cfg(feature = "sdr-device")]
fn sdr_devices() -> cat_signal::DeviceList {
    cat_signal_rtlsdr::device::devices()
}

#[cfg(not(feature = "sdr-device"))]
fn sdr_devices() -> cat_signal::DeviceList {
    cat_signal::DeviceList::unavailable(
        cat_signal::DeviceKind::Sdr,
        "not in this build -- rebuild with --features sdr-device",
    )
}

/// What the *radio's* machine can see, offered to consoles elsewhere.
///
/// A console is not usually on the radio's machine, so enumerating locally
/// would offer an operator their own laptop's microphone as the radio's
/// receive audio -- which looks entirely correct and is wrong. This is the
/// server's side of asking properly.
///
/// It lives here rather than in `server` for the same reason
/// [`enumerate_devices`] does: looking for a sound card needs that card's
/// driver, and the server crate should not link one in order to answer
/// (Rule 5).
struct ServerDevices {
    if_source: std::sync::Arc<server::spectrum::IfSelection>,
    audio_source: std::sync::Arc<server::audio::AudioSelection>,
}

impl ServerDevices {
    /// What this bench has wired, as a console should be told it.
    ///
    /// Assembled from what was actually opened rather than from the
    /// flags: a `--if-out` that failed to connect is not a wired source,
    /// and saying it was would have a console draw a waterfall panel that
    /// never fills.
    ///
    /// `state` is `Configured` throughout, not `Streaming`. Whether frames
    /// are moving is something a console can see for itself, and claiming
    /// otherwise from here would be guessing about a thread that may have
    /// just lost its dongle.
    fn installation(&self) -> radio::capabilities::Installation {
        let mut installation =
            radio::capabilities::Installation::bare(vec![radio::capabilities::EndpointRole::Cat]);
        if self.if_source.is_set() {
            if let Some(source) = radio::capabilities::Installation::if_tap_from(
                &radio::capabilities::TS570D,
                // The station's crystal trim is measured once against a
                // known carrier and nobody here has measured this one. A
                // wrong non-zero default would be worse than none,
                // because it would look calibrated.
                0,
                radio::capabilities::SourceState::Configured,
                "IF tap on CN4",
            ) {
                installation = installation.with_source(source);
            }
        }
        if self.audio_source.is_set() {
            installation = installation.with_source(
                radio::capabilities::InstalledSource::new(
                    radio::capabilities::SignalCapability::AudioDerived {
                        // The ACC2 pair is a communications-audio pair;
                        // 4 kHz is what the pipeline reports and what a
                        // console may draw. Declaring it is what stops a
                        // consumer rendering audio as a band panorama.
                        max_bandwidth_hz: 4_000,
                    },
                    radio::capabilities::SourceState::Configured,
                    "ACC2 receive audio",
                )
                // Post-DSP: this is what the operator is hearing, so a
                // console may honestly draw the radio's filter passband
                // over its FFT.
                .from_origin(radio::capabilities::AudioOrigin::RadioOutput),
            );
        }
        installation
    }
}

impl cat_signal::DeviceDirectory for ServerDevices {
    fn list(&self) -> Vec<cat_signal::DeviceList> {
        // The same enumeration the local console does, because it is the
        // same machine's hardware and an operator should not see two
        // different answers depending on where they sat down.
        enumerate_devices()
    }

    fn attach(&self, kind: cat_signal::DeviceKind, spec: &str) -> Result<(), String> {
        match kind {
            cat_signal::DeviceKind::Sdr => {
                // Opened here, so a busy dongle refuses the attach with
                // the driver's own words while the operator is still
                // looking at the picker -- rather than two seconds later
                // on a background thread with nobody listening.
                let source = server::spectrum::open_spec(spec, self.if_source.trim_hz())?;
                self.if_source.select(spec.to_string(), source);
                info!("console attached the IF source {spec}");
                Ok(())
            }
            cat_signal::DeviceKind::AudioInput => {
                // Opened here for the same reason as the SDR: a card
                // another program holds refuses the attach with the
                // driver's own words while the operator is still looking
                // at the picker.
                let source = server::audio::open_spec(spec)?;
                self.audio_source.select(spec.to_string(), source);
                info!("console attached the ACC2 audio source {spec}");
                Ok(())
            }
            other => Err(format!("this server cannot attach a {other:?}")),
        }
    }
}

/// Open a device the operator picked.
///
/// The console never names a concrete source type; this is the wiring
/// layer, which is the only place that may (Rule 5).
fn attach(device: &cat_signal::DeviceInfo, dial_hz: u64, trim_hz: i32) -> Result<Attached, String> {
    match device.kind {
        cat_signal::DeviceKind::Sdr => cat_signal_rtlsdr::open(
            &device.spec,
            if_tap(trim_hz),
            cat_signal_rtlsdr::IfSourceConfig::default(),
        )
        .map(|source| Attached::Spectrum(SpectrumFeed::start(source, dial_hz)))
        .map_err(|e| e.to_string()),
        cat_signal::DeviceKind::AudioInput => open_local_audio(&device.spec),
        // `DeviceKind` is `#[non_exhaustive]`: a kind added upstream should
        // reach an operator as "this console does not know that yet", not
        // as a compile error here and not as silence.
        other => Err(format!("this console cannot attach a {other:?}")),
    }
}

/// Print usage and exit with code 1.
fn usage_exit() -> ! {
    eprintln!(
        "Usage: ts570d --cat-only <port>  [--baud <rate>] [--stop-bits <n>] [sources]\n       \
                ts570d --cat-dtr  <port>  [--baud <rate>] [--stop-bits <n>] [sources]\n       \
                ts570d --server   <host:port>\n       \
                ts570d --server-raw <host:port>\n\
         \n\
           --cat-only  CAT only. The station does not key PTT from DTR, so\n\
                       the console does not offer the PTT-line control.\n\
           --cat-dtr   CAT, and PTT keyed from the serial DTR line (an ACC2\n\
                       opto interface, say). Offers the [P] PTT-line item.\n\
                       Mutually exclusive with --cat-only.\n\
         \n\
           <port> is a local device or a host:port on an RFC 2217 device\n\
           server -- ser2net, a Moxa/Digi box, or this repo's emulator run\n\
           with --com. Examples:\n\
                       /dev/ttyUSB0   COM3   127.0.0.1:4001   radio.local:4001\n\
         \n\
           --baud      Baud rate: 1200, 2400, 4800, 9600  (default: 9600)\n\
           --stop-bits Stop bits: 1 or 2                  (default: 1)\n\
         \n\
           --server    Attach to a remote `ts570d server` that already owns\n\
                       a radio and is sharing it, rather than owning a port\n\
                       of your own. Speaks the console protocol, so point it\n\
                       at the server's --console-port. Brings capabilities,\n\
                       the waterfall, and the radio host's device list.\n\
                       Example: --server 127.0.0.1:7400\n\
           --server-raw  The same, over raw CAT against --raw-tcp-port. No\n\
                       capabilities, no spectrum, and no way to ask what the\n\
                       radio's host has attached. Prefer --server.\n\
         \n\
         Signal sources (optional, either serial mode):\n\
           --if-out <endpoint>        the radio's IF output: an rtl_tcp\n\
                                      server (host:port), or a local dongle\n\
                                      as rtl:0, rtl:1, ...\n\
           --acc2-audio <endpoint>    the ACC2 receive-audio pair: a PCM\n\
                                      server (host:port), or a sound device\n\
           --calibration <file>       a snapshot from `ts570d calibrate`,\n\
                                      checked once at startup. Defaults to\n\
                                      ~/.config/ts570d/calibration.json.\n\
           --acc2-capture <n>         sound card capture gain, raw mixer\n\
                                      units. Asserted on open and re-asserted\n\
                                      after every USB re-enumeration, which\n\
                                      silently reverts it.\n\
           --acc2-playback <n>        sound card playback level (TX drive),\n\
                                      same treatment.\n\
           --if-trim <hz>             this station's IF calibration, signed\n\
                                      Hz (default 0). Park the radio on a\n\
                                      known-exact carrier such as WWV, see\n\
                                      how far the trace lands off, and pass\n\
                                      the negative of that."
    );
    std::process::exit(1);
}

/// Parse the local-TUI arguments. Unknown flags are silently ignored.
///
/// Exits with an error message and code 1 for missing or invalid values,
/// and for any two of `--cat-only`/`--cat-dtr`/`--server` together.
fn parse_args() -> Args {
    let mut args_iter = std::env::args().skip(1);
    let mut cat_only: Option<String> = None;
    let mut cat_dtr: Option<String> = None;
    let mut baud: u32 = 9600;
    let mut stop_bits: u8 = 1;
    let mut server: Option<String> = None;
    let mut server_raw: Option<String> = None;
    let mut if_out: Option<String> = None;
    let mut acc2_audio: Option<String> = None;
    let mut if_trim: i32 = 0;

    loop {
        match args_iter.next().as_deref() {
            Some("--if-trim") => match args_iter.next() {
                Some(val) => {
                    if_trim = parse_trim_hz(&val).unwrap_or_else(|e| {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    })
                }
                None => usage_exit(),
            },
            Some("--cat-only") => match args_iter.next() {
                Some(path) => cat_only = Some(path),
                None => usage_exit(),
            },
            Some("--cat-dtr") => match args_iter.next() {
                Some(path) => cat_dtr = Some(path),
                None => usage_exit(),
            },
            Some("--server") => match args_iter.next() {
                Some(addr) => server = Some(addr),
                None => usage_exit(),
            },
            Some("--server-raw") => match args_iter.next() {
                Some(addr) => server_raw = Some(addr),
                None => usage_exit(),
            },
            Some("--if-out") => match args_iter.next() {
                Some(addr) => if_out = Some(addr),
                None => usage_exit(),
            },
            Some("--acc2-audio") => match args_iter.next() {
                Some(addr) => acc2_audio = Some(addr),
                None => usage_exit(),
            },
            Some("--baud") => match args_iter.next() {
                Some(val) => {
                    let rate: u32 = val.parse().unwrap_or_else(|_| {
                        eprintln!("error: --baud value must be a number, got {:?}", val);
                        std::process::exit(1);
                    });
                    match rate {
                        1200 | 2400 | 4800 | 9600 => baud = rate,
                        _ => {
                            eprintln!(
                                "error: invalid baud rate {}; valid values: 1200, 2400, 4800, 9600",
                                rate
                            );
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    eprintln!("error: --baud requires a value");
                    std::process::exit(1);
                }
            },
            Some("--stop-bits") => match args_iter.next() {
                Some(val) => {
                    let n: u8 = val.parse().unwrap_or_else(|_| {
                        eprintln!("error: --stop-bits value must be a number, got {:?}", val);
                        std::process::exit(1);
                    });
                    match n {
                        1 | 2 => stop_bits = n,
                        _ => {
                            eprintln!("error: invalid stop bits {}; valid values: 1 or 2", n);
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    eprintln!("error: --stop-bits requires a value");
                    std::process::exit(1);
                }
            },
            Some(_) => {}
            None => break,
        }
    }

    // Three ways to reach a radio, and exactly one of them per run. Named
    // in the error rather than counted, so somebody who passed two is told
    // which two.
    let chosen: Vec<&str> = [
        cat_only.as_ref().map(|_| "--cat-only"),
        cat_dtr.as_ref().map(|_| "--cat-dtr"),
        server.as_ref().map(|_| "--server"),
        server_raw.as_ref().map(|_| "--server-raw"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if chosen.len() > 1 {
        eprintln!(
            "error: {} are mutually exclusive -- pick one",
            chosen.join(" and ")
        );
        std::process::exit(1);
    }

    let transport = match (cat_only, cat_dtr, server, server_raw) {
        (Some(port), _, _, _) => Transport::Serial {
            port,
            baud,
            stop_bits,
            keys_ptt: false,
        },
        (None, Some(port), _, _) => Transport::Serial {
            port,
            baud,
            stop_bits,
            keys_ptt: true,
        },
        (None, None, Some(addr), _) => Transport::Server { addr },
        (None, None, None, Some(addr)) => Transport::ServerRaw { addr },
        (None, None, None, None) => usage_exit(),
    };

    Args {
        transport,
        if_out,
        acc2_audio,
        if_trim,
    }
}

/// Command-line arguments specific to `ts570d server ...`: headless network
/// server mode, one process owning the serial port and exposing it over the
/// network to WSJT-X (via the rigctld-compatible listener) and/or other
/// `radio-cat-rs`-aware clients (via the raw `cat-server` TCP/UDP
/// listeners), instead of running the local TUI.
struct ServerArgs {
    port: String,
    /// This station's IF calibration in Hz, from `--if-trim`.
    if_trim: i32,
    /// `--calibration <file>`: a snapshot from `ts570d calibrate`, checked
    /// once at startup. `None` is itself reported — a radio whose menus
    /// nobody has recorded is worth one line.
    calibration: Option<String>,
    /// `--acc2-capture <n>` / `--acc2-playback <n>`: the sound card mixer
    /// values this server asserts and re-asserts. Station-specific, so
    /// flags rather than constants.
    acc2_capture: Option<i64>,
    acc2_playback: Option<i64>,
    baud: u32,
    stop_bits: u8,
    raw_tcp_port: Option<u16>,
    raw_udp_port: Option<u16>,
    rigctl_port: Option<u16>,
    /// The typed console protocol, for `ts570d-gui`.
    console_port: Option<u16>,
    /// The radio's IF output, as `host:port` speaking rtl_tcp. Either the
    /// emulator's `--if-out` or a real dongle behind `rtl_tcp`.
    if_out: Option<String>,
    /// The ACC2 receive-audio pair on *this* machine, for a console
    /// somewhere else: a PCM server (`host:port`) or a sound device.
    acc2_audio: Option<String>,
}

/// Print `ts570d server` usage and exit with code 1.
fn server_usage_exit() -> ! {
    eprintln!(
        "Usage: ts570d server --port <serial-port-path> [--baud <rate>] [--stop-bits <n>]\n\
                     [--raw-tcp-port <port>] [--raw-udp-port <port>] [--rigctl-port <port>]\n\
                     [--console-port <port>] [--if-out <endpoint>]\n\
                     [--acc2-audio <endpoint>]\n\
         \n\
           --port          Serial port path (required)\n\
           --baud          Baud rate: 1200, 2400, 4800, 9600  (default: 9600)\n\
           --stop-bits     Stop bits: 1 or 2                  (default: 1)\n\
           --raw-tcp-port  Bind cat-server's raw length-prefixed TCP protocol\n\
           --raw-udp-port  Bind cat-server's raw enveloped UDP protocol\n\
           --rigctl-port   Bind a Hamlib rigctld-compatible TCP listener\n\
                           (for WSJT-X's \"Hamlib NET rigctl\" rig type)\n\
           --console-port  Bind the typed console protocol -- what both\n\
                           `ts570d --server` and ts570d-gui speak\n\
           --if-out        The radio's IF output: an rtl_tcp server\n\
                           (host:port) or a local dongle as rtl:0, rtl:1.\n\
                           Needs --console-port to go anywhere.\n\
           --acc2-audio    The radio's ACC2 receive-audio pair, for a\n\
                           console on another machine: a PCM server\n\
                           (host:port) or a sound device. Needs\n\
                           --console-port to go anywhere.\n\
           At least one of --raw-tcp-port/--raw-udp-port/--rigctl-port/--console-port\n\
           is required."
    );
    std::process::exit(1);
}

/// Parse `ts570d server`'s own flags from `std::env::args()`, skipping both
/// the program name and the `server` subcommand word itself (positions 0
/// and 1).
fn parse_server_args() -> ServerArgs {
    let mut args_iter = std::env::args().skip(2);
    let mut port: Option<String> = None;
    let mut baud: u32 = 9600;
    let mut stop_bits: u8 = 1;
    let mut if_trim: i32 = 0;
    let mut calibration: Option<String> = None;
    let mut acc2_capture: Option<i64> = None;
    let mut acc2_playback: Option<i64> = None;
    let mut raw_tcp_port: Option<u16> = None;
    let mut raw_udp_port: Option<u16> = None;
    let mut rigctl_port: Option<u16> = None;
    let mut console_port: Option<u16> = None;
    let mut if_out: Option<String> = None;
    let mut acc2_audio: Option<String> = None;

    fn parse_port_number(val: Option<String>, flag: &str) -> u16 {
        match val.and_then(|v| v.parse::<u16>().ok()) {
            Some(p) => p,
            None => {
                eprintln!("error: {flag} requires a valid port number (0-65535)");
                std::process::exit(1);
            }
        }
    }

    loop {
        match args_iter.next().as_deref() {
            Some("--port") => match args_iter.next() {
                Some(path) => port = Some(path),
                None => server_usage_exit(),
            },
            Some("--baud") => match args_iter.next() {
                Some(val) => {
                    let rate: u32 = val.parse().unwrap_or_else(|_| {
                        eprintln!("error: --baud value must be a number, got {:?}", val);
                        std::process::exit(1);
                    });
                    match rate {
                        1200 | 2400 | 4800 | 9600 => baud = rate,
                        _ => {
                            eprintln!(
                                "error: invalid baud rate {}; valid values: 1200, 2400, 4800, 9600",
                                rate
                            );
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    eprintln!("error: --baud requires a value");
                    std::process::exit(1);
                }
            },
            Some("--stop-bits") => match args_iter.next() {
                Some(val) => {
                    let n: u8 = val.parse().unwrap_or_else(|_| {
                        eprintln!("error: --stop-bits value must be a number, got {:?}", val);
                        std::process::exit(1);
                    });
                    match n {
                        1 | 2 => stop_bits = n,
                        _ => {
                            eprintln!("error: invalid stop bits {}; valid values: 1 or 2", n);
                            std::process::exit(1);
                        }
                    }
                }
                None => {
                    eprintln!("error: --stop-bits requires a value");
                    std::process::exit(1);
                }
            },
            Some("--raw-tcp-port") => {
                raw_tcp_port = Some(parse_port_number(args_iter.next(), "--raw-tcp-port"))
            }
            Some("--raw-udp-port") => {
                raw_udp_port = Some(parse_port_number(args_iter.next(), "--raw-udp-port"))
            }
            Some("--rigctl-port") => {
                rigctl_port = Some(parse_port_number(args_iter.next(), "--rigctl-port"))
            }
            Some("--console-port") => {
                console_port = Some(parse_port_number(args_iter.next(), "--console-port"))
            }
            Some("--if-out") => if_out = args_iter.next(),
            Some("--acc2-audio") => acc2_audio = args_iter.next(),
            Some("--calibration") => calibration = args_iter.next(),
            Some("--acc2-capture") => {
                acc2_capture = Some(parse_mixer_level(args_iter.next(), "--acc2-capture"))
            }
            Some("--acc2-playback") => {
                acc2_playback = Some(parse_mixer_level(args_iter.next(), "--acc2-playback"))
            }
            Some("--if-trim") => match args_iter.next() {
                Some(val) => {
                    if_trim = parse_trim_hz(&val).unwrap_or_else(|e| {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    })
                }
                None => server_usage_exit(),
            },
            Some(_) => {}
            None => break,
        }
    }

    if raw_tcp_port.is_none()
        && raw_udp_port.is_none()
        && rigctl_port.is_none()
        && console_port.is_none()
    {
        eprintln!(
            "error: at least one of --raw-tcp-port/--raw-udp-port/--rigctl-port/--console-port is required"
        );
        std::process::exit(1);
    }

    // A tap with nothing to serve it to is a thread reading a socket for
    // no reason, and much more likely a mistyped invocation than an
    // intention.
    if if_out.is_some() && console_port.is_none() {
        eprintln!("error: --if-out needs --console-port; nothing else consumes the spectrum");
        std::process::exit(1);
    }
    // Same reasoning: only the console protocol carries audio, so
    // capturing it with nothing to send it to is a sound card held open
    // for nobody.
    if acc2_audio.is_some() && console_port.is_none() {
        eprintln!("error: --acc2-audio needs --console-port; nothing else consumes the audio");
        std::process::exit(1);
    }

    match port {
        Some(p) => ServerArgs {
            port: p,
            baud,
            stop_bits,
            raw_tcp_port,
            raw_udp_port,
            rigctl_port,
            console_port,
            if_out,
            acc2_audio,
            if_trim,
            calibration,
            acc2_capture,
            acc2_playback,
        },
        None => server_usage_exit(),
    }
}

/// Which kind of serial port the one serial arm actually opened.
///
/// An enum rather than a boxed `CatSession`, because `Ts570d<S>` is generic
/// over `S` and boxing it would mean a `dyn CatSession` that the `?Send`
/// async-trait shape makes awkward. Two short arms are cheaper than that,
/// and this is the wiring layer, which is where concrete types belong.
enum Session {
    Local(SerialCatSession<SerialPort>),
    Remote(SerialCatSession<Rfc2217Port>),
}

/// Adapts `cat_transport_tcp::TcpCatSession` (`CatSession<Error =
/// TcpSessionError>`) to `CatSession<Error = TransportError>`, the bound
/// `Ts570d<S>` requires. Mirrors `ft991a::main::TcpClientSession`'s shape
/// exactly (see `docs/adr/0006-windows-concurrency-model.md`'s
/// cross-reference note). Unlike `ft991a`, `ts570d` wraps this in nothing
/// further: `ui::run`'s `Radio` bound has no `ModemControlLines`-equivalent
/// requirement (confirmed by reading `radio/src/radio_trait.rs` — `ts570d`
/// has no CW-keying feature, per this repo's own CLAUDE.md), so no
/// `cat_transport_core::NoModemControlLines` wrapper is needed here.
struct TcpClientSession {
    inner: TcpCatSession,
}

impl TcpClientSession {
    fn new(inner: TcpCatSession) -> Self {
        Self { inner }
    }
}

/// `TcpSessionError` -> `TransportError`: the orthogonal error-mapping step
/// every network-transport consumer needs regardless of modem-line concerns
/// (see `cat_transport_core::NoModemControlLines`'s doc comment for why this
/// mapping can only be written here, not in a shared crate).
fn map_tcp_err(err: TcpSessionError) -> TransportError {
    match err {
        TcpSessionError::Io(e) => TransportError::Io(e),
        TcpSessionError::FrameTooLarge { len, max } => TransportError::Other(format!(
            "frame length {len} exceeds max frame size {max} bytes"
        )),
    }
}

#[async_trait::async_trait(?Send)]
impl CatSession for TcpClientSession {
    type Error = TransportError;

    async fn execute(
        &mut self,
        request: &[u8],
        response: &mut Vec<u8>,
    ) -> Result<ResponseDisposition, Self::Error> {
        self.inner
            .execute(request, response)
            .await
            .map_err(map_tcp_err)
    }

    async fn send(&mut self, request: &[u8]) -> Result<(), Self::Error> {
        self.inner.send(request).await.map_err(map_tcp_err)
    }

    fn flush_rx(&mut self) {
        self.inner.flush_rx();
    }
}

/// `ts570d server ...` -- headless network server mode: one process owns
/// the serial port, exposed over the network to WSJT-X (via the new
/// rigctld-compatible listener) and/or other `radio-cat-rs`-aware clients
/// (via the existing raw `cat-server` TCP/UDP listeners), instead of
/// running the local TUI.
async fn run_server_mode() {
    let args = parse_server_args();

    let port = SerialPort::open(
        &args.port,
        SerialConfig {
            baud_rate: args.baud,
            stop_bits: args.stop_bits,
            // Never assert DTR at open — see the TUI-mode open above:
            // DTR may be wired as a PTT key line. (planning/ptt-line)
            initial_dtr: false,
            ..SerialConfig::default()
        },
    )
    .expect("serial open failed");

    info!(
        "Serial port opened (server mode): {} @ {} baud {} stop bit(s)",
        args.port, args.baud, args.stop_bits
    );

    // Checked before the listeners open, while nothing else is talking to
    // the radio. The session is borrowed through a typed client and handed
    // straight back: a serial port opens once, and `server::run` needs it.
    let session = {
        let mut probe = radio::Ts570d::new(SerialCatSession::new(port));
        // The flag wins; otherwise the default location, but only if a
        // file is actually there. A default path that does not exist is
        // "no snapshot", not "a snapshot that will not read" — the second
        // reads like a fault and this is just a station that has never
        // captured one.
        let default = calibrate::default_path().filter(|p| p.exists());
        let path = args
            .calibration
            .clone()
            .or_else(|| default.map(|p| p.display().to_string()));
        let status = radio::calibration::check(&mut probe, path.as_deref()).await;
        report_calibration(&status);
        probe.into_session()
    };
    // Built here, not inside the server: a source named by `--if-out` and
    // one a console picks later must be opened by the same code, and this
    // is the layer allowed to name it (Rule 5).
    let if_source = server::spectrum::IfSelection::new(args.if_out.clone(), args.if_trim);
    // The audio pair, opened up front if one was named. Opening here
    // rather than inside the server means a mistyped device fails while
    // the operator is still looking at the terminal, not two seconds
    // later on a background thread.
    // Built with the mixer values up front, so the flag, a console's
    // attach and the capture thread's own reconnect all assert the same
    // thing. See `server::mixer` for why the server owns this at all.
    #[cfg(all(target_os = "linux", feature = "audio-device"))]
    let audio_source = server::audio::AudioSelection::with_mixer(server::mixer::MixerSettings {
        capture: args.acc2_capture,
        playback: args.acc2_playback,
    });
    #[cfg(not(all(target_os = "linux", feature = "audio-device")))]
    let audio_source = server::audio::AudioSelection::new();
    if let Some(spec) = args.acc2_audio.clone() {
        // Recorded before the open is attempted, so the mixer is still
        // asserted when the PCM belongs to PipeWire or WSJT-X.
        audio_source.set_requested(&spec);
        match server::audio::open_spec(&spec) {
            Ok(source) => {
                info!("ACC2 audio attached at {spec}");
                audio_source.select(spec, source);
            }
            // Not fatal, for the same reason a missing dongle is not: a
            // radio with nothing on its audio pair is still a radio.
            Err(e) => eprintln!("warning: {e}"),
        }
    }
    let devices = std::sync::Arc::new(ServerDevices {
        if_source: std::sync::Arc::clone(&if_source),
        audio_source: std::sync::Arc::clone(&audio_source),
    });
    let config = server::ServerConfig {
        raw_tcp_port: args.raw_tcp_port,
        raw_udp_port: args.raw_udp_port,
        rigctl_port: args.rigctl_port,
        console_port: args.console_port,
        if_source: Some(std::sync::Arc::clone(&if_source)),
        audio_source: Some(std::sync::Arc::clone(&audio_source)),
        // What *this* machine can see. A console on another one has no
        // way to know, and its own sound cards are not this radio's.
        devices: Some(devices.clone() as std::sync::Arc<dyn cat_signal::DeviceDirectory>),
        installation: Some({
            let devices = std::sync::Arc::clone(&devices);
            std::sync::Arc::new(move || devices.installation())
        }),
    };

    // `server::run` is `async fn` on Linux and a plain blocking `fn` on
    // Windows (matching `cat_rigctl::run`'s own per-platform split, since
    // `#[monoio::main]` cannot exist there -- see `server::run`'s own doc
    // comment). Both variants share the same name and now support the same
    // full feature set (including `--rigctl-port`), so only this call
    // site's `.await` needs to differ.
    #[cfg(target_os = "linux")]
    let result = server::run(session, config).await;
    #[cfg(target_os = "windows")]
    let result = server::run(session, config);

    if let Err(e) = result {
        eprintln!("Server error: {e}");
        std::process::exit(1);
    }
}

/// Shared application entry point, driven to completion by `#[monoio::main]`
/// on Linux and by `win_runtime::block_on` on Windows (see
/// `docs/adr/0006-windows-concurrency-model.md`). Everything below this
/// point (argument parsing, opening the chosen transport, `ui::run`) is
/// identical on both platforms; only the executor driving this future
/// differs.
async fn run_app() {
    // 1. Initialize logging — use RUST_LOG env var to control verbosity.
    tracing_subscriber::fmt().with_env_filter("info").init();

    info!("Starting TS-570D Radio Control Application");

    // `ts570d server ...` branches off entirely before the direct/TUI
    // argument parsing below -- it has its own flag set and never opens a
    // `ui::run` session.
    if std::env::args().nth(1).as_deref() == Some("server") {
        run_server_mode().await;
        return;
    }

    // `ts570d calibrate ...` likewise: its own flag set, no TUI, and it
    // needs a person at the front panel rather than a console.
    if std::env::args().nth(1).as_deref() == Some("calibrate") {
        run_calibrate_mode().await;
        return;
    }

    // 2. Parse CLI arguments.
    let args = parse_args();

    // 3. Open the chosen transport, then wrap it in the typed TS-570D client.
    let result = match args.transport {
        Transport::Serial {
            port,
            baud,
            stop_bits,
            keys_ptt,
        } => {
            // One flag pair, two transports, chosen by the shape of the
            // argument rather than by a third flag. A device path opens a
            // local port; a `host:port` opens the same port on an RFC 2217
            // device server. Everything above this line is identical.
            let session = if endpoint::is_network(&port) {
                let remote = Rfc2217Port::connect(
                    port.as_str(),
                    Rfc2217Config {
                        baud_rate: baud,
                        stop_bits,
                        // Never assert DTR at open. On a --cat-dtr station
                        // that is key-down the moment the program starts
                        // (SN-5 of the ACC2-IF datasheet); on a --cat-only
                        // station CAT does not need it either way.
                        initial_dtr: false,
                        // RTS must be asserted: the TS-570D's RTS input is
                        // receive-enable and it withholds CAT responses
                        // while the line is low (manual p. 70).
                        initial_rts: true,
                        ..Rfc2217Config::default()
                    },
                )
                .unwrap_or_else(|e| {
                    eprintln!("error: could not open the remote serial port at {port}: {e}");
                    std::process::exit(1);
                });
                info!(
                    "Remote serial port opened: {} @ {} baud {} stop bit(s) (RFC 2217)",
                    port, baud, stop_bits
                );
                Session::Remote(SerialCatSession::new(remote))
            } else {
                // SerialPort::open must be called inside an active monoio
                // runtime on Linux because it registers the fd with
                // io_uring; on Windows it is a plain synchronous call (see
                // radio-cat-rs's docs/adr/0004-windows-serial-backend.md).
                let local = SerialPort::open(
                    &port,
                    SerialConfig {
                        baud_rate: baud,
                        stop_bits,
                        initial_dtr: false,
                        ..SerialConfig::default()
                    },
                )
                .expect("serial open failed");
                info!(
                    "Serial port opened: {} @ {} baud {} stop bit(s)",
                    port, baud, stop_bits
                );
                Session::Local(SerialCatSession::new(local))
            };

            let sources = open_sources(args.if_out, args.acc2_audio, INITIAL_DIAL_HZ, args.if_trim);

            // `--cat-only` says this station does not key from DTR, so the
            // console is handed a line it will report as absent and the
            // `[P]` item does not appear. The port still has the pin; the
            // shack does not use it, and that is not something software can
            // detect for itself.
            match session {
                Session::Local(s) => {
                    let radio = Ts570d::new(s);
                    let ptt: Box<dyn radio::PttLine> = if keys_ptt {
                        radio.ptt_line_handle()
                    } else {
                        Box::new(radio::NoPttLine)
                    };
                    #[cfg(target_os = "linux")]
                    let ui_result = ui::run_console(radio, ptt, sources).await;
                    #[cfg(target_os = "windows")]
                    let ui_result = ui::run_console(radio, ptt, sources);
                    ui_result
                }
                Session::Remote(s) => {
                    let radio = Ts570d::new(s);
                    let ptt: Box<dyn radio::PttLine> = if keys_ptt {
                        radio.ptt_line_handle()
                    } else {
                        Box::new(radio::NoPttLine)
                    };
                    #[cfg(target_os = "linux")]
                    let ui_result = ui::run_console(radio, ptt, sources).await;
                    #[cfg(target_os = "windows")]
                    let ui_result = ui::run_console(radio, ptt, sources);
                    ui_result
                }
            }
        }
        Transport::Server { addr } => {
            let mut radio =
                native_console::NativeConsoleRadio::connect(&addr).unwrap_or_else(|e| {
                    eprintln!("error: {e}");
                    eprintln!(
                        "hint: --server wants a `ts570d server --console-port`. For a raw CAT \
                     listener (--raw-tcp-port), use --server-raw."
                    );
                    std::process::exit(1);
                });
            info!(
                "Attached to {} over the console protocol at {addr}",
                radio.capabilities().model
            );

            // The waterfall is fed by the same connection, on its own
            // thread: an FFT redraw must not be able to stall the radio
            // poll, and the two run at different rates on purpose.
            let sources = native_console::sources(&mut radio);

            // No PTT line, and none is possible: a socket has no DTR pin,
            // so the `[P]` item correctly does not appear.
            #[cfg(target_os = "linux")]
            let ui_result = ui::run_console(radio, Box::new(radio::NoPttLine), sources).await;
            #[cfg(target_os = "windows")]
            let ui_result = ui::run_console(radio, Box::new(radio::NoPttLine), sources);

            ui_result
        }
        Transport::ServerRaw { addr } => {
            let tcp_session = TcpCatSession::connect(&addr).await.unwrap_or_else(|e| {
                eprintln!("error: could not connect to {addr}: {e}");
                std::process::exit(1);
            });

            info!("Connected to remote ts570d server at {}", addr);

            // No `ptt_line_handle()` here, and none is possible:
            // `TcpClientSession` has no modem control lines at all, so the
            // `[P]` item correctly does not appear in this mode.
            let radio = Ts570d::new(TcpClientSession::new(tcp_session));

            // `remote()`, not `default()`: this console does not own the
            // radio, so this machine's devices are not the radio's and must
            // not be offered as if they were. See `ConsoleSources::remote`.
            #[cfg(target_os = "linux")]
            let ui_result =
                ui::run_console(radio, Box::new(radio::NoPttLine), ConsoleSources::remote()).await;
            #[cfg(target_os = "windows")]
            let ui_result =
                ui::run_console(radio, Box::new(radio::NoPttLine), ConsoleSources::remote());

            ui_result
        }
    };

    // 4. Report any UI-level error.
    if let Err(e) = result {
        eprintln!("UI error: {}", e);
        std::process::exit(1);
    }

    info!("Application stopped");
}

/// Print what the startup calibration check found.
///
/// A warning rather than a refusal to start: a radio with no snapshot is
/// perfectly usable, and an operator who has never wanted one should not
/// be blocked. But menus 38 (TX inhibit) and 39 (linear amplifier relay)
/// can each make a working radio look broken with nothing on the display
/// to explain it, and CAT cannot read either — so a server that says
/// nothing about them is hiding the one thing it cannot find out later.
fn report_calibration(status: &radio::calibration::CalibrationStatus) {
    let lines = status.lines();
    if status.is_warning() {
        for line in lines {
            tracing::warn!("{line}");
        }
    } else {
        for line in lines {
            info!("{line}");
        }
    }
}

/// Parse and run `ts570d calibrate ...`.
///
/// Separate from the TUI's argument parsing for the same reason `server`
/// is: a different flag set, and no console session at all.
async fn run_calibrate_mode() {
    let mut args = std::env::args().skip(2);
    let mut port: Option<String> = None;
    let mut server: Option<String> = None;
    let mut baud: u32 = 9600;
    let mut mode: Option<calibrate::Mode> = None;

    loop {
        match args.next().as_deref() {
            Some("--port") => port = args.next(),
            Some("--server") => server = args.next(),
            Some("--baud") => {
                baud = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| calibrate_usage_exit());
            }
            Some("--out") => mode = args.next().map(|out| calibrate::Mode::Capture { out }),
            Some("--verify") => mode = args.next().map(|file| calibrate::Mode::Verify { file }),
            Some("--restore") => mode = args.next().map(|file| calibrate::Mode::Restore { file }),
            Some(_) => {}
            None => break,
        }
    }

    let Some(mode) = mode else {
        eprintln!("error: one of --out, --verify or --restore is required");
        calibrate_usage_exit();
    };

    // A serial port can only be owned once, so an operator with a
    // `ts570d server` already running reaches the radio through it rather
    // than being told to stop it mid-calibration.
    match (port, server) {
        (Some(_), Some(_)) => {
            eprintln!("error: --port and --server are mutually exclusive");
            std::process::exit(1);
        }
        (Some(path), None) => {
            let local = SerialPort::open(
                &path,
                SerialConfig {
                    baud_rate: baud,
                    initial_dtr: false,
                    ..SerialConfig::default()
                },
            )
            .unwrap_or_else(|e| {
                eprintln!("error: could not open {path}: {e}");
                std::process::exit(1);
            });
            let mut radio = radio::Ts570d::new(SerialCatSession::new(local));
            calibrate::run(&mut radio, mode).await;
        }
        (None, Some(addr)) => {
            let tcp = TcpCatSession::connect(&addr).await.unwrap_or_else(|e| {
                eprintln!("error: could not connect to {addr}: {e}");
                std::process::exit(1);
            });
            let mut radio = radio::Ts570d::new(TcpClientSession::new(tcp));
            calibrate::run(&mut radio, mode).await;
        }
        (None, None) => {
            eprintln!("error: one of --port or --server is required");
            calibrate_usage_exit();
        }
    }
}

/// Parse an `--acc2-capture` / `--acc2-playback` value.
///
/// Raw mixer units, not dB: they are what `amixer` prints and what a bench
/// note records. Range-checking is left to the card, which knows its own
/// limits and clamps; this only insists on a number.
fn parse_mixer_level(value: Option<String>, flag: &str) -> i64 {
    match value.as_deref().map(str::parse::<i64>) {
        Some(Ok(n)) if n >= 0 => n,
        Some(Ok(n)) => {
            eprintln!("error: {flag} must not be negative, got {n}");
            std::process::exit(1);
        }
        Some(Err(_)) => {
            eprintln!("error: {flag} wants a whole number of mixer units");
            std::process::exit(1);
        }
        None => {
            eprintln!("error: {flag} requires a value");
            std::process::exit(1);
        }
    }
}

fn calibrate_usage_exit() -> ! {
    eprintln!(
        "Usage: ts570d calibrate (--port <port> | --server <host:port>) <action>\n\
         \n\
         Captures every setting this radio has, including the 52 menus. CAT\n\
         cannot read a menu it is not parked on, so the menu pass needs you at\n\
         the front panel: you sweep the MENU knob and it records what it sees.\n\
         \n\
         Actions:\n\
         \x20 --out <file>      capture the radio's state to <file>\n\
         \x20 --verify <file>   compare the radio against <file>\n\
         \x20 --restore <file>  write <file> back, then sweep to confirm it landed\n\
         \n\
         Reaching the radio:\n\
         \x20 --port <port>     own the serial port directly (a device path, or a\n\
         \x20                   host:port on an RFC 2217 server)\n\
         \x20 --server <addr>   go through a running `ts570d server`'s raw CAT\n\
         \x20                   port, when it already owns the serial line\n\
         \x20 --baud <rate>     default 9600\n\
         \n\
         NOTE: menu values cannot be read back over CAT. Any front-panel menu\n\
         change after a capture makes the file silently wrong, and no software\n\
         can detect that. Re-capture after changing settings by hand."
    );
    std::process::exit(1);
}

/// Linux entry point. Uses monoio's io_uring runtime (single-threaded, !Send).
#[cfg(target_os = "linux")]
#[monoio::main(timer_enabled = true)]
async fn main() {
    run_app().await;
}

/// Windows entry point. `#[monoio::main]` cannot exist on Windows (`monoio`
/// requires io_uring, a Linux kernel interface) -- `win_runtime::block_on`
/// drives the same `run_app()` future instead. See
/// `docs/adr/0006-windows-concurrency-model.md`.
#[cfg(target_os = "windows")]
fn main() {
    win_runtime::block_on(run_app());
}

#[cfg(test)]
mod trim_tests {
    use super::{parse_trim_hz, MAX_TRIM_HZ};

    #[test]
    fn a_measured_negative_trim_parses() {
        // The sign is the whole point. This bench measured its carrier
        // rendering +115 Hz high, so the correction is -115, and a parser
        // that dropped the sign would move the waterfall the wrong way by
        // twice the error.
        assert_eq!(parse_trim_hz("-115"), Ok(-115));
    }

    #[test]
    fn a_positive_trim_parses_with_or_without_its_sign() {
        assert_eq!(parse_trim_hz("115"), Ok(115));
        assert_eq!(parse_trim_hz("+115"), Ok(115));
    }

    #[test]
    fn no_calibration_is_zero_and_is_legal() {
        // An uncalibrated station is a real state, not an error.
        assert_eq!(parse_trim_hz("0"), Ok(0));
    }

    #[test]
    fn the_bounds_are_inclusive() {
        assert_eq!(parse_trim_hz(&MAX_TRIM_HZ.to_string()), Ok(MAX_TRIM_HZ));
        assert_eq!(parse_trim_hz(&(-MAX_TRIM_HZ).to_string()), Ok(-MAX_TRIM_HZ));
    }

    #[test]
    fn an_absurd_trim_is_refused_rather_than_silently_mislabelling() {
        // Half a band of "calibration" is a typo or a units mix-up. Taking
        // it would render a waterfall that is wrong everywhere and looks
        // authoritative, which is worse than refusing to start.
        for value in ["50001", "-50001", "14285690"] {
            let err = parse_trim_hz(value).expect_err("should refuse");
            assert!(
                err.contains("not a calibration"),
                "unhelpful message for {value:?}: {err}"
            );
        }
    }

    #[test]
    fn a_non_number_is_refused_and_says_what_it_wanted() {
        for value in ["", "115hz", "1e3", "-115.5", " 115"] {
            let err = parse_trim_hz(value).expect_err("should refuse");
            assert!(
                err.contains("whole number of Hz"),
                "unhelpful message for {value:?}: {err}"
            );
        }
    }
}
