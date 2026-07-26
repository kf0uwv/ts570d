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

// Linux-only: implements `cat_rigctl::RigctlRadio`, and `cat-rigctl` itself
// has no Windows backend upstream (see `run`'s Windows doc comment below).
#[cfg(target_os = "linux")]
mod rigctl_radio;

#[cfg(target_os = "linux")]
pub use rigctl_radio::RigctlTs570d;

/// Which network listeners to bring up. Re-exported from `cat_rigctl` on
/// Linux, where every field is meaningful; on Windows this crate defines an
/// identical local copy instead (see `windows_run`'s module doc below) so
/// callers (`src/main.rs`) do not need to branch on platform to construct
/// it.
#[cfg(target_os = "linux")]
pub use cat_rigctl::ServerConfig;

/// See the Linux `ServerConfig`'s doc above -- same fields, same meaning.
/// `rigctl_port` is accepted here (so `src/main.rs`'s CLI parsing needs no
/// platform branching either) but is rejected at `run()`-time with a clear
/// error, not silently ignored -- see `windows_run`.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Default)]
pub struct ServerConfig {
    /// `cat-server`'s raw length-prefixed TCP protocol.
    pub raw_tcp_port: Option<u16>,
    /// `cat-server`'s raw enveloped UDP protocol.
    pub raw_udp_port: Option<u16>,
    /// The Hamlib rigctld-compatible TCP listener, for WSJT-X. Accepted
    /// syntactically for CLI-parsing symmetry with Linux, but rejected at
    /// `run()`-time on Windows -- see `windows_run`.
    pub rigctl_port: Option<u16>,
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
/// On Linux this delegates entirely to `cat_rigctl::run`, unchanged. On
/// Windows, `cat_rigctl` itself does not compile at all (it uses
/// `monoio::net`/`monoio::spawn` unconditionally in source, with no Windows
/// backend upstream in `radio-cat-rs` yet -- unlike `cat-transport-serial`,
/// `cat-transport-tcp`, `cat-transport-udp`, and `cat-server`, which all
/// have one). See `windows_run` below for what Windows supports instead:
/// the raw `cat-server` TCP/UDP listeners (fully Windows-capable today, via
/// `cat_server::tcp_windows`/`udp_windows`), but not the rigctld/WSJT-X
/// bridge, which has no Windows backend to call into yet. This is a real,
/// currently-unresolved upstream gap in `radio-cat-rs`, not a local
/// workaround this crate can close on its own -- see
/// `docs/adr/0006-windows-concurrency-model.md`'s note on this.
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

/// Windows implementation of [`run`] (same name, so `src/main.rs` needs no
/// platform branching to call it). Brings up `cat-server`'s raw TCP/UDP
/// listeners directly, via `cat_server::build`/`tcp_windows::serve`/
/// `udp_windows::serve` (all genuinely cross-platform -- see
/// `radio-cat-rs`'s `docs/adr/0006-windows-network-transport.md`), instead
/// of going through `cat_rigctl::run` at all. `--rigctl-port` (the WSJT-X
/// bridge) is rejected with a clear, actionable error rather than silently
/// ignored, since `cat-rigctl` has no Windows backend upstream yet.
///
/// Unlike the Linux implementation (which lets `cat_rigctl::run`'s
/// `monoio::spawn`ed listener tasks all run cooperatively on one thread),
/// this spawns one dedicated `std::thread` per listener (mirroring
/// `cat_server::tcp_windows`/`udp_windows`'s own "one thread per
/// connection/datagram" internal model one level up) and waits for the
/// first one to end via a `std::sync::mpsc` channel -- the direct `std`
/// analog of `cat_rigctl::run`'s `futures::future::select_all`.
#[cfg(target_os = "windows")]
pub fn run<S>(session: S, config: ServerConfig) -> std::io::Result<()>
where
    S: cat_transport_core::CatSession + Send + 'static,
    S::Error: std::error::Error + 'static,
{
    use std::sync::{Arc, Mutex};

    if config.rigctl_port.is_some() {
        return Err(std::io::Error::other(
            "--rigctl-port (the WSJT-X-compatible rigctld bridge) is not yet \
             supported on Windows: cat-rigctl has no Windows backend upstream \
             yet in radio-cat-rs. Use --raw-tcp-port and/or --raw-udp-port \
             instead (any radio-cat-rs-aware client, e.g. this application's \
             own `--server` mode, works over those today).",
        ));
    }
    if config.raw_tcp_port.is_none() && config.raw_udp_port.is_none() {
        return Err(std::io::Error::other(
            "server mode requires at least one of --raw-tcp-port/--raw-udp-port \
             (--rigctl-port is not yet available on Windows)",
        ));
    }

    let (worker, handle) = cat_server::build(session, &radio::TS570D_COMMAND_TABLE);
    std::thread::spawn(move || worker.run());

    let (done_tx, done_rx) = std::sync::mpsc::channel::<std::io::Result<()>>();
    let mut listener_count = 0;

    if let Some(port) = config.raw_tcp_port {
        let listener = std::net::TcpListener::bind(("0.0.0.0", port))?;
        tracing::info!("Raw CAT TCP listener bound on 0.0.0.0:{port}");
        let handle = handle.clone();
        let registry = Arc::new(Mutex::new(cat_server::ClientRegistry::new()));
        let done_tx = done_tx.clone();
        listener_count += 1;
        std::thread::spawn(move || {
            let result = cat_server::tcp_windows::serve(listener, handle, registry);
            if let Err(e) = &result {
                tracing::error!("Raw CAT TCP listener on 0.0.0.0:{port} failed: {e}");
            }
            let _ = done_tx.send(result);
        });
    }

    if let Some(port) = config.raw_udp_port {
        let socket = std::net::UdpSocket::bind(("0.0.0.0", port))?;
        tracing::info!("Raw CAT UDP listener bound on 0.0.0.0:{port}");
        let handle = handle.clone();
        let registry = Arc::new(Mutex::new(cat_server::ClientRegistry::new()));
        let done_tx = done_tx.clone();
        listener_count += 1;
        std::thread::spawn(move || {
            let result = cat_server::udp_windows::serve(socket, handle, registry);
            if let Err(e) = &result {
                tracing::error!("Raw CAT UDP listener on 0.0.0.0:{port} failed: {e}");
            }
            let _ = done_tx.send(result);
        });
    }
    drop(done_tx);
    debug_assert!(listener_count > 0);

    // Wait for the first listener thread to end (accept()/bind-time failure,
    // or never on the happy path), and propagate its result -- the `std`
    // analog of the Linux path's `futures::future::select_all`.
    done_rx
        .recv()
        .unwrap_or(Err(std::io::Error::other("all listener threads exited")))
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
