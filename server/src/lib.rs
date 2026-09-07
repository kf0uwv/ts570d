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

//! Network server mode: one process owns the physical TS-570D serial
//! session, shared by any number of remote clients over the network
//! instead of each needing their own exclusive serial connection.
//!
//! Thin wiring over `radio-cat-rs`'s `cat-rigctl` crate, which now owns
//! everything radio-independent: the request broker, raw TCP/UDP
//! listeners, the Hamlib rigctld-compatible bridge (dispatch/dump_state/
//! line framing), and `run()`'s overall orchestration. This crate supplies
//! only [`rigctl_radio`]'s `cat_rigctl::RigctlRadio` impl for
//! `radio::Ts570d` -- the one seam where TS-570D-specific knowledge (mode
//! names, frequency range, which typed methods back which rigctld command)
//! plugs in.

pub mod audio;

use cat_framework::installation::Installation;
mod console;
mod rigctl_radio;
pub mod spectrum;

// Shared with the binary rather than duplicated: what counts as a device
// path and what counts as a host:port must not be two different answers
// depending on which program is asking.
#[path = "../../src/endpoint.rs"]
mod endpoint;

pub use console::ConsoleTs570d;
pub mod ptt_dtr;
pub use ptt_dtr::{PttDtr, MAX_KEY_DOWN};
pub use rigctl_radio::RigctlTs570d;

/// Which network listeners to bring up — re-exported unconditionally from
/// `cat_rigctl`, which is itself cross-platform since
/// docs/adr/0006-windows-network-transport.md's 2026-07-26 amendment
/// (`radio-cat-rs`).
/// Which listeners to bring up, plus where this radio's spectrum comes
/// from.
///
/// A superset of `cat_rigctl::ServerConfig` rather than a re-export,
/// because the SDR is this radio's business: `cat-rigctl` orchestrates
/// listeners and has no opinion about where a spectrum comes from, and
/// where the IF output comes from is this radio's business.
#[derive(Clone, Default)]
pub struct ServerConfig {
    /// `cat-server`'s raw length-prefixed TCP protocol.
    pub raw_tcp_port: Option<u16>,
    /// `cat-server`'s raw enveloped UDP protocol.
    pub raw_udp_port: Option<u16>,
    /// The Hamlib rigctld-compatible listener, for WSJT-X.
    pub rigctl_port: Option<u16>,
    /// The typed console protocol, for `ts570d-gui`.
    pub console_port: Option<u16>,
    /// Which IF source the tap thread reads, and the handle a console
    /// uses to change it. On a TS-570D the connector is the CN4 header;
    /// the field names the signal, because that is what a radio-generic
    /// consumer of this would want.
    ///
    /// Constructed by the wiring layer rather than from a string here, so
    /// that a source an operator picks at runtime and one named by
    /// `--if-out` are opened by the same code (Rule 5).
    pub if_source: Option<std::sync::Arc<spectrum::IfSelection>>,
    /// Which ACC2 receive-audio source the capture thread reads, and the
    /// handle a console uses to change it.
    ///
    /// Separate from `if_source` because they are separate hardware: a
    /// bench can have an SDR on the IF tap and nothing on the audio pair,
    /// or the reverse.
    pub audio_source: Option<std::sync::Arc<audio::AudioSelection>>,
    /// What the *radio's* machine can see, for a console on another one.
    ///
    /// Supplied by the wiring layer, because enumerating a sound card
    /// needs that card's driver and this crate should not link one to say
    /// so (Rule 5). `None` declines the question, and a client is told
    /// exactly that rather than being handed an empty list -- which would
    /// be a claim about this machine that a server which never looked is
    /// in no position to make.
    pub devices: Option<std::sync::Arc<dyn cat_signal::DeviceDirectory>>,
    /// What this bench has wired, for a console to be told at handshake.
    ///
    /// A closure rather than a value, because it changes: a console can
    /// attach a source at runtime, and the next console to connect should
    /// be told what is actually there rather than what was there when the
    /// server started.
    pub installation: Option<std::sync::Arc<dyn Fn() -> Installation + Send + Sync>>,
}

/// Hand-written because neither a device directory nor an IF selection is
/// `Debug`, and both are the kind of thing whose *presence* is what a log
/// line wants anyway -- the contents are a live socket and a live dongle.
impl std::fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfig")
            .field("raw_tcp_port", &self.raw_tcp_port)
            .field("raw_udp_port", &self.raw_udp_port)
            .field("rigctl_port", &self.rigctl_port)
            .field("console_port", &self.console_port)
            .field("if_source", &self.if_source.is_some())
            .field("audio_source", &self.audio_source.is_some())
            .field("devices", &self.devices.is_some())
            .field("installation", &self.installation.is_some())
            .finish()
    }
}

impl ServerConfig {
    fn listeners(&self) -> cat_rigctl::ServerConfig {
        cat_rigctl::ServerConfig {
            raw_tcp_port: self.raw_tcp_port,
            raw_udp_port: self.raw_udp_port,
            rigctl_port: self.rigctl_port,
            native_port: self.console_port,
            // Consoles are told what this radio is from the one
            // declaration, so the handshake cannot disagree with the
            // rigctl bridge about the same facts.
        }
    }
}

/// Bring up the broker (owning `session`, the one physical radio
/// connection) plus every listener `config` requests, and run until one of
/// them fails. `S` is generic (not hardcoded to `SerialCatSession`) so
/// `main.rs` remains the only place a concrete transport type is named, per
/// this repo's Rule 5 -- but this crate is otherwise contractually
/// TS-570D-shaped (it names `radio::TS570D_COMMAND_TABLE` directly, exactly
/// like `ui` does for the UI-facing traits), not radio-generic.
///
/// # Platform note
///
/// Delegates entirely to `cat_rigctl::run`, which is itself `#[cfg]`-
/// selected per platform (`async fn` on Linux, a plain blocking `fn` on
/// Windows, since `#[monoio::main]` cannot exist there) — see
/// `docs/adr/0006-windows-concurrency-model.md`'s amendment for the history
/// of this crate's earlier, now-superseded hand-rolled Windows fallback
/// that dropped `--rigctl-port` support entirely. Full `--rigctl-port`/
/// WSJT-X support now works identically on both platforms.
#[cfg(target_os = "linux")]
pub async fn run<S>(session: S, config: ServerConfig) -> std::io::Result<()>
where
    S: cat_transport_core::CatSession + 'static,
    S::Error: std::error::Error + 'static,
{
    let shared = config.console_port.map(|_| match &config.devices {
        Some(devices) => cat_rigctl::native_bridge::NativeShared::with_devices(
            &radio::capabilities::TS570D,
            std::sync::Arc::clone(devices),
        ),
        None => cat_rigctl::native_bridge::NativeShared::new(&radio::capabilities::TS570D),
    });
    if let (Some(shared), Some(selection)) = (shared.clone(), config.if_source.clone()) {
        spectrum::spawn(shared, selection);
    }
    if let (Some(shared), Some(selection)) = (shared.clone(), config.audio_source.clone()) {
        audio::spawn(shared, selection);
    }
    if let Some(shared) = shared.clone() {
        // What this radio's console should look like, authored by the
        // crate that knows the radio. Published in the handshake, so a
        // console is told how to arrange itself in the same breath as
        // what it is arranging.
        shared.set_layout(radio::console_layout::layout());
        shared.set_theme(radio::console_layout::theme());
    }
    if let (Some(shared), Some(installation)) = (shared.clone(), config.installation.clone()) {
        // The closure, not its result: it is called afresh per connection,
        // so a console connecting after somebody attached a source is told
        // what is now wired rather than what was there at startup.
        shared.set_installation(installation);
    }
    cat_rigctl::run_with_native(
        session,
        &radio::TS570D_COMMAND_TABLE,
        config.listeners(),
        |broker_session| {
            // Key on DTR, not CAT `TX;`. On this station PTT is DTR through
            // an opto onto ACC2 pin 9 (PKS), and the two are not equivalent:
            // pin 9 mutes the mic while keyed, `TX;` does not. A second
            // handle onto the same broker carries the keying tasks.
            let keying = cat_server::BrokerCatSession::new(
                broker_session.handle(),
                broker_session.client_id(),
            );
            RigctlTs570d::with_dtr_ptt(radio::Ts570d::new(broker_session), keying)
        },
        |broker_session| ConsoleTs570d(radio::Ts570d::new(broker_session)),
        shared,
    )
    .await
}

/// Windows implementation of [`run`] — see the Linux version's doc comment.
/// A plain blocking `fn` since `cat_rigctl::run` itself is one on Windows
/// (genuine OS threads instead of `monoio`'s cooperative tasks); there is
/// nothing to `.await` here.
#[cfg(target_os = "windows")]
pub fn run<S>(session: S, config: ServerConfig) -> std::io::Result<()>
where
    S: cat_transport_core::CatSession + Send + 'static,
    S::Error: std::error::Error + 'static,
{
    let shared = config.console_port.map(|_| match &config.devices {
        Some(devices) => cat_rigctl::native_bridge::NativeShared::with_devices(
            &radio::capabilities::TS570D,
            std::sync::Arc::clone(devices),
        ),
        None => cat_rigctl::native_bridge::NativeShared::new(&radio::capabilities::TS570D),
    });
    if let (Some(shared), Some(selection)) = (shared.clone(), config.if_source.clone()) {
        spectrum::spawn(shared, selection);
    }
    if let (Some(shared), Some(selection)) = (shared.clone(), config.audio_source.clone()) {
        audio::spawn(shared, selection);
    }
    if let Some(shared) = shared.clone() {
        // What this radio's console should look like, authored by the
        // crate that knows the radio. Published in the handshake, so a
        // console is told how to arrange itself in the same breath as
        // what it is arranging.
        shared.set_layout(radio::console_layout::layout());
        shared.set_theme(radio::console_layout::theme());
    }
    if let (Some(shared), Some(installation)) = (shared.clone(), config.installation.clone()) {
        // The closure, not its result: it is called afresh per connection,
        // so a console connecting after somebody attached a source is told
        // what is now wired rather than what was there at startup.
        shared.set_installation(installation);
    }
    cat_rigctl::run_with_native(
        session,
        &radio::TS570D_COMMAND_TABLE,
        config.listeners(),
        |broker_session| {
            // Key on DTR, not CAT `TX;`. On this station PTT is DTR through
            // an opto onto ACC2 pin 9 (PKS), and the two are not equivalent:
            // pin 9 mutes the mic while keyed, `TX;` does not. A second
            // handle onto the same broker carries the keying tasks.
            let keying = cat_server::BrokerCatSession::new(
                broker_session.handle(),
                broker_session.client_id(),
            );
            RigctlTs570d::with_dtr_ptt(radio::Ts570d::new(broker_session), keying)
        },
        |broker_session| ConsoleTs570d(radio::Ts570d::new(broker_session)),
        shared,
    )
}

// Gated to Linux: uses #[monoio::test] (see
// docs/adr/0006-windows-concurrency-model.md).
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use cat_transport_core::test_support::ScriptedCatSession;

    #[monoio::test(driver = "legacy")]
    async fn run_with_no_listeners_configured_returns_an_error() {
        let session = ScriptedCatSession::new();
        let result = run(session, ServerConfig::default()).await;
        assert!(result.is_err());
    }
}
