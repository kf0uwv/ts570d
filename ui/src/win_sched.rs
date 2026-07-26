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

//! A minimal, `monoio`-free replacement for `monoio::spawn`'s cooperative
//! two-task concurrency, used only by `terminal::run`'s Windows
//! implementation. See `docs/adr/0006-windows-concurrency-model.md` for the
//! full design rationale, in particular why a real `std::thread::spawn`
//! worker (the shape `radio-cat-rs`'s ADR 0004 originally sketched) does not
//! work here: `radio::Ts570d<S>` is unconditionally `!Send` (its internal
//! `SharedSession` uses `Rc<RefCell<Option<S>>>`), so the radio task must
//! stay on the same OS thread as the UI task, exactly as it already does
//! under `monoio` on Linux.
//!
//! Nothing in this module is actually Windows-specific (plain `std::thread`/
//! `std::task`), so — mirroring `radio-cat-rs`'s own `cat-transport-tcp`/
//! `cat-transport-udp` `windows.rs` precedent (ADR 0006 §3) — it is not
//! `#[cfg(target_os = "windows")]`-gated and gets real, executable test
//! coverage on every platform this workspace builds for, even though its
//! only production caller (`terminal::run`'s Windows variant) is gated.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

/// Wakes a specific thread on `wake()`/`wake_by_ref()` — the same shape
/// `cat_transport_core::completion`'s and `cat_server::block_on`'s own
/// tests/production code already rely on, so a completion future woken from
/// one of `cat-transport-serial`'s/`cat-transport-tcp`'s Windows worker
/// threads correctly unparks [`block_on_two`]'s loop promptly.
struct ThreadWaker(std::thread::Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// A `monoio::time::sleep` replacement with no timer thread and no waker
/// registration of its own: correctness relies on [`block_on_two`]'s own
/// `park_timeout` bound re-polling it at least that often. A deliberate
/// simplification specific to this one caller (both the radio and UI tasks
/// are already polled every couple of milliseconds regardless, for
/// responsiveness) — not a general-purpose timer.
pub(crate) struct WinSleep {
    deadline: Instant,
}

impl WinSleep {
    pub(crate) fn new(duration: Duration) -> Self {
        Self {
            deadline: Instant::now() + duration,
        }
    }
}

impl Future for WinSleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        if Instant::now() >= self.deadline {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

/// Poll `ui_fut` and `radio_fut` in round-robin, returning as soon as
/// `ui_fut` resolves (mirroring Linux's `terminal::run`: awaiting the UI
/// task then dropping the radio task's handle without waiting for it).
/// `ui_fut` is always polled first, every iteration, regardless of what
/// `radio_fut` is doing internally — the property that keeps key events
/// (including Quit) responsive during a slow radio poll or a long
/// diagnostic run. `radio_fut` is polled at most until it first resolves;
/// polling a future again after `Ready` violates `Future`'s contract, but in
/// practice `radio_fut` never resolves before `ui_fut` does (the UI task
/// returns immediately after sending `RadioCmd::Quit`, before the radio task
/// can act on it).
pub(crate) fn block_on_two<U, R>(mut ui_fut: Pin<Box<U>>, mut radio_fut: Pin<Box<R>>) -> U::Output
where
    U: Future,
    R: Future<Output = ()> + ?Sized,
{
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut radio_done = false;
    loop {
        if let Poll::Ready(out) = ui_fut.as_mut().poll(&mut cx) {
            return out;
        }
        if !radio_done {
            if let Poll::Ready(()) = radio_fut.as_mut().poll(&mut cx) {
                radio_done = true;
            }
        }
        std::thread::park_timeout(Duration::from_millis(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_on_two_returns_as_soon_as_ui_future_resolves() {
        let ui = Box::pin(async { 42 });
        let radio: Pin<Box<dyn Future<Output = ()>>> = Box::pin(async {
            loop {
                WinSleep::new(Duration::from_secs(3600)).await;
            }
        });
        assert_eq!(block_on_two(ui, radio), 42);
    }

    #[test]
    fn win_sleep_resolves_only_after_its_deadline() {
        let start = Instant::now();
        let ui = Box::pin(async {
            WinSleep::new(Duration::from_millis(20)).await;
        });
        let radio: Pin<Box<dyn Future<Output = ()>>> = Box::pin(async {});
        block_on_two(ui, radio);
        assert!(start.elapsed() >= Duration::from_millis(20));
    }

    #[test]
    fn radio_future_is_polled_concurrently_with_a_slow_ui_future() {
        use std::cell::Cell;
        use std::rc::Rc;

        let radio_progressed = Rc::new(Cell::new(false));
        let radio_progressed_inner = Rc::clone(&radio_progressed);

        // UI "sleeps" much longer than the radio task takes, proving the
        // radio task is actually polled to completion while the UI task is
        // still pending -- not starved behind it.
        let ui = Box::pin(async move {
            WinSleep::new(Duration::from_millis(200)).await;
            radio_progressed_inner.get()
        });
        let radio: Pin<Box<dyn Future<Output = ()>>> = Box::pin(async {
            WinSleep::new(Duration::from_millis(10)).await;
            radio_progressed.set(true);
        });

        assert!(
            block_on_two(ui, radio),
            "radio task must make progress while the UI task is still pending"
        );
    }
}
