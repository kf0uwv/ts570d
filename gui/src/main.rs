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

//! `ts570d-gui` — the GPU console.
//!
//! Network-only by ADR 0008: there is no `--port`, and this binary depends
//! on no transport crate. Running against a local radio means running
//! `ts570d server` locally, which is the same shape people already run for
//! WSJT-X.

use gui::{app::Console, Window};

/// The server's console port.
///
/// NOT 4532: that is where `ts570d server` binds the rigctld-compatible
/// listener for WSJT-X, and this binary speaks `cat-native` instead. The
/// two protocols share nothing, so the old default could only ever fail to
/// connect.
const DEFAULT_ADDRESS: &str = "127.0.0.1:7400";

fn main() -> eframe::Result<()> {
    let address = std::env::args()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .unwrap_or_else(|| DEFAULT_ADDRESS.to_string());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 560.0])
            // Ask for focus at creation. On Wayland a window cannot raise
            // itself later -- the compositor decides, and without asking
            // here the console opens behind whatever had focus and looks
            // like it failed to start. Requesting it up front is the one
            // moment the protocol allows.
            .with_active(true)
            .with_visible(true)
            .with_app_id("ts570d")
            .with_title("TS-570D"),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "ts570d-gui",
        options,
        Box::new(move |cc| {
            gui::theme::install(&cc.egui_ctx);
            let mut console = Console::new(address);
            // Try immediately: the common case is a server already running,
            // and making the operator click "connect" every time would be
            // ceremony.
            console.connect();
            Ok(Box::new(Window(console)))
        }),
    )
}
