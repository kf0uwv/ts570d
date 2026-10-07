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

//! Test-only: a broker over an in-crate session double whose CAT exchange
//! can be made to stall, shared by every keying test in this crate (rule 6:
//! never a real transport).

// An in-crate session double (rule 6: never a real transport). Its
// `FA;` exchange never answers, like a radio that stopped talking
// mid-read, so the broker's worker holds the session until its request
// timeout. Every line change is logged with which handle made it:
// `primary` is the session's own borrowed lines (reachable only through
// the worker's queue), `release` the owned duplicate the broker keeps
// for de-keying (ADR 0022). Tests run on Linux under a monoio runtime
// and on Windows over the thread-based broker -- same assertions.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use cat_framework::ResponseDisposition;
use cat_server::{BrokerCatSession, ClientId};
use cat_transport_core::{CatSession, ModemControlLines, TransportError};

/// How long the broker lets the stuck exchange run before giving up.
pub(crate) const STALL: Duration = Duration::from_millis(1500);
/// What "did not wait for the queue" means.
pub(crate) const PROMPT: Duration = Duration::from_millis(100);

pub(crate) type EventLog = Arc<Mutex<Vec<String>>>;

pub(crate) fn logged(log: &EventLog) -> Vec<String> {
    log.lock().unwrap().clone()
}

pub(crate) fn position(events: &[String], entry: &str) -> Option<usize> {
    events.iter().position(|e| e == entry)
}

pub(crate) struct RecordingLines {
    tag: &'static str,
    log: EventLog,
}

impl ModemControlLines for RecordingLines {
    fn set_rts(&self, asserted: bool) -> Result<(), TransportError> {
        let entry = format!("{} rts={asserted}", self.tag);
        self.log.lock().unwrap().push(entry);
        Ok(())
    }
    fn set_dtr(&self, asserted: bool) -> Result<(), TransportError> {
        let entry = format!("{} dtr={asserted}", self.tag);
        self.log.lock().unwrap().push(entry);
        Ok(())
    }
    fn read_cts(&self) -> Result<bool, TransportError> {
        Ok(false)
    }
    fn read_dsr(&self) -> Result<bool, TransportError> {
        Ok(false)
    }
    fn read_dcd(&self) -> Result<bool, TransportError> {
        Ok(false)
    }
}

pub(crate) struct StallingSession {
    log: EventLog,
    primary: RecordingLines,
    with_release_lines: bool,
}

impl StallingSession {
    pub(crate) fn new(with_release_lines: bool) -> (Self, EventLog) {
        let log = EventLog::default();
        let session = Self {
            with_release_lines,
            primary: RecordingLines {
                tag: "primary",
                log: Arc::clone(&log),
            },
            log: Arc::clone(&log),
        };
        (session, log)
    }
}

/// Logs `cat over` however the exchange ends -- answered or cancelled
/// by the broker's timeout.
pub(crate) struct ExchangeOver(EventLog, String);

impl Drop for ExchangeOver {
    fn drop(&mut self) {
        let entry = format!("cat over {}", self.1);
        self.0.lock().unwrap().push(entry);
    }
}

#[async_trait(?Send)]
impl CatSession for StallingSession {
    type Error = TransportError;

    async fn execute(
        &mut self,
        request: &[u8],
        response: &mut Vec<u8>,
    ) -> Result<ResponseDisposition, Self::Error> {
        let req = String::from_utf8_lossy(request).into_owned();
        self.log.lock().unwrap().push(format!("cat begin {req}"));
        let _over = ExchangeOver(Arc::clone(&self.log), req);
        if request == b"FA;" {
            std::future::pending::<()>().await;
        }
        response.extend_from_slice(b"ID018;");
        Ok(ResponseDisposition::ResponseWritten)
    }

    fn modem_lines(&self) -> Option<&dyn ModemControlLines> {
        Some(&self.primary)
    }

    fn release_lines(&self) -> Option<Box<dyn ModemControlLines + Send + Sync>> {
        self.with_release_lines.then(|| {
            Box::new(RecordingLines {
                tag: "release",
                log: Arc::clone(&self.log),
            }) as Box<dyn ModemControlLines + Send + Sync>
        })
    }
}

/// Build a broker over a fresh [`StallingSession`] and start its worker.
/// Must be called inside [`run`].
pub(crate) fn broker() -> (cat_server::BrokerHandle, EventLog) {
    broker_with(true)
}

pub(crate) fn broker_with(release_lines: bool) -> (cat_server::BrokerHandle, EventLog) {
    let (session, log) = StallingSession::new(release_lines);
    let (worker, handle) =
        cat_server::build_with_timeout(session, &radio::TS570D_COMMAND_TABLE, STALL);
    #[cfg(target_os = "linux")]
    monoio::spawn(worker.run());
    #[cfg(not(target_os = "linux"))]
    std::thread::spawn(move || worker.run());
    (handle, log)
}

pub(crate) fn session(handle: &cat_server::BrokerHandle) -> BrokerCatSession {
    BrokerCatSession::new(handle.clone(), ClientId::from_raw(1))
}

/// Drive a test body to completion on this platform's executor.
pub(crate) fn run<F: std::future::Future<Output = ()>>(body: F) {
    #[cfg(target_os = "linux")]
    monoio::RuntimeBuilder::<monoio::LegacyDriver>::new()
        .enable_timer()
        .build()
        .expect("monoio runtime")
        .block_on(body);
    #[cfg(not(target_os = "linux"))]
    cat_server::block_on::block_on(body);
}

/// Run `work` concurrently with the test body.
#[cfg(target_os = "linux")]
pub(crate) fn in_background<F: std::future::Future<Output = ()> + 'static>(work: F) {
    monoio::spawn(work);
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn in_background<F: std::future::Future<Output = ()> + Send + 'static>(work: F) {
    std::thread::spawn(move || cat_server::block_on::block_on(work));
}

pub(crate) async fn pause(d: Duration) {
    #[cfg(target_os = "linux")]
    monoio::time::sleep(d).await;
    #[cfg(not(target_os = "linux"))]
    std::thread::sleep(d);
}

/// Submit the `FA;` that hangs, and return once the worker is inside it.
pub(crate) async fn stall_the_wire(handle: &cat_server::BrokerHandle, log: &EventLog) {
    let h = handle.clone();
    in_background(async move {
        let _ = h.submit(ClientId::from_raw(9), b"FA;".to_vec()).await;
    });
    until_logged(log, "cat begin FA;").await;
}

pub(crate) async fn until_logged(log: &EventLog, entry: &str) {
    let give_up = Instant::now() + STALL * 2;
    while position(&logged(log), entry).is_none() {
        assert!(
            Instant::now() < give_up,
            "never logged {entry:?}: {:?}",
            logged(log)
        );
        pause(Duration::from_millis(1)).await;
    }
}
