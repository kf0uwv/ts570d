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

mod console;
mod rigctl_radio;
mod spectrum;

pub use console::ConsoleTs570d;
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
/// the CN4 tap is a TS-570D fact.
#[derive(Debug, Clone, Default)]
pub struct ServerConfig {
    /// `cat-server`'s raw length-prefixed TCP protocol.
    pub raw_tcp_port: Option<u16>,
    /// `cat-server`'s raw enveloped UDP protocol.
    pub raw_udp_port: Option<u16>,
    /// The Hamlib rigctld-compatible listener, for WSJT-X.
    pub rigctl_port: Option<u16>,
    /// The typed console protocol, for `ts570d-gui`.
    pub console_port: Option<u16>,
    /// An RTL-SDR on CN4, as `host:port` speaking `rtl_tcp` — the
    /// emulator's `--cn4`, or a real dongle behind `rtl_tcp`.
    pub cn4: Option<String>,
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
    let shared = config
        .console_port
        .map(|_| cat_rigctl::native_bridge::NativeShared::new(&radio::capabilities::TS570D));
    if let (Some(shared), Some(addr)) = (shared.clone(), config.cn4.clone()) {
        spectrum::spawn(shared, addr);
    }
    cat_rigctl::run_with_native(
        session,
        &radio::TS570D_COMMAND_TABLE,
        config.listeners(),
        |broker_session| RigctlTs570d(radio::Ts570d::new(broker_session)),
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
    let shared = config
        .console_port
        .map(|_| cat_rigctl::native_bridge::NativeShared::new(&radio::capabilities::TS570D));
    if let (Some(shared), Some(addr)) = (shared.clone(), config.cn4.clone()) {
        spectrum::spawn(shared, addr);
    }
    cat_rigctl::run_with_native(
        session,
        &radio::TS570D_COMMAND_TABLE,
        config.listeners(),
        |broker_session| RigctlTs570d(radio::Ts570d::new(broker_session)),
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
