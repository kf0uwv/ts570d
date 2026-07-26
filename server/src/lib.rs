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

mod rigctl_radio;

pub use rigctl_radio::RigctlTs570d;

/// Which network listeners to bring up — re-exported unconditionally from
/// `cat_rigctl`, which is itself cross-platform since
/// docs/adr/0006-windows-network-transport.md's 2026-07-26 amendment
/// (`radio-cat-rs`).
pub use cat_rigctl::ServerConfig;

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
    cat_rigctl::run(
        session,
        &radio::TS570D_COMMAND_TABLE,
        config,
        |broker_session| RigctlTs570d(radio::Ts570d::new(broker_session)),
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
    cat_rigctl::run(
        session,
        &radio::TS570D_COMMAND_TABLE,
        config,
        |broker_session| RigctlTs570d(radio::Ts570d::new(broker_session)),
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
