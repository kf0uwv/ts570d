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

//! The console: status strip, quick bar, workspaces, command line.
//!
//! Placement only. Every decision this file appears to make is made in
//! `workspace`, `quick`, `tuning`, `command` or `readout`, which are
//! testable without a window — see the crate docs for why that split is
//! load-bearing rather than tidy.

use cat_native::{Client, Command, Event, ServerMessage};
use cat_signal::SpectrumFrame;
use cat_ui::MeterReading;
use eframe::egui;
use egui::{Align, Color32, Layout, RichText, Sense, Stroke, Vec2};

use crate::readout::Readout;
use crate::workspace::{self, Tab, TabEntry};
use crate::{command, quick, theme, tuning};

/// Where the console is in its life.
enum Link {
    /// Never connected, or connection lost. Carries why.
    Down(String),
    Up(Box<Client>),
}

pub struct Console {
    address: String,
    link: Link,
    tabs: Vec<TabEntry>,
    active: Tab,
    readout: Readout,
    /// The newest spectrum frame, and the reference the waterfall
    /// re-projects onto.
    latest: Option<SpectrumFrame>,
    waterfall: cat_ui_egui::waterfall::WaterfallImage,
    command_open: bool,
    command_text: String,
    /// The last thing the console has to say to the operator.
    status: String,
}

impl Console {
    pub fn new(address: String) -> Self {
        Self {
            address,
            link: Link::Down("not connected".to_string()),
            tabs: Vec::new(),
            active: Tab::Source,
            readout: Readout::default(),
            latest: None,
            // TURBO over MONO: this console's job is to make a weak
            // carrier findable, and a perceptually-uniform ramp separates
            // the bottom few dB where that carrier lives. The floor is a
            // starting value, not a calibration -- it becomes a published
            // setting once the source layer lands.
            waterfall: cat_ui_egui::waterfall::WaterfallImage::new(
                512,
                256,
                cat_ui_egui::waterfall::Palette::TURBO,
                -110.0,
            ),
            command_open: false,
            command_text: String::new(),
            status: String::new(),
        }
    }

    /// Try to connect, replacing whatever link there was.
    pub fn connect(&mut self) {
        // Spectrum is requested unconditionally: this console's whole
        // reason for existing on a GPU is the waterfall, and a client that
        // declined would then have to reconnect to change its mind.
        match Client::connect(self.address.as_str(), true) {
            Ok(client) => {
                self.tabs = workspace::tabs(client.capabilities());
                self.active = self.tabs.first().map(|t| t.tab).unwrap_or(Tab::Source);
                self.status = format!("connected to {}", client.capabilities().model);
                self.link = Link::Up(Box::new(client));
            }
            Err(e) => {
                self.status = format!("{e}");
                self.link = Link::Down(e.to_string());
            }
        }
    }

    fn capabilities(&self) -> Option<&cat_native::CapabilitiesWire> {
        match &self.link {
            Link::Up(client) => Some(client.capabilities()),
            Link::Down(_) => None,
        }
    }

    /// Drain everything the reader thread has for us. Never blocks.
    fn pump(&mut self) {
        let mut lost = None;
        if let Link::Up(client) = &self.link {
            while let Some(event) = client.try_event() {
                match event {
                    Event::Reply(ServerMessage::Error { code, message }) => {
                        self.status = format!("{code:?}: {message}");
                    }
                    Event::Reply(_) => {}
                    Event::Disconnected(why) => {
                        lost = Some(why);
                        break;
                    }
                }
            }
            if let Some(frame) = client.take_spectrum() {
                self.latest = Some(frame);
            }
        }
        if let Some(why) = lost {
            self.status = format!("connection lost: {why}");
            self.link = Link::Down(why);
        }
    }

    fn send(&mut self, command: Command) {
        if let Link::Up(client) = &self.link {
            if !client.send(command) {
                self.status = "connection lost".to_string();
                self.link = Link::Down("send failed".to_string());
            }
        } else {
            self.status = "not connected".to_string();
        }
    }

    fn run_line(&mut self) {
        let line = std::mem::take(&mut self.command_text);
        let Some(caps) = self.capabilities().cloned() else {
            self.status = "not connected".to_string();
            return;
        };
        match command::parse(&line, &caps) {
            Ok(command::Action::Quit) => std::process::exit(0),
            Ok(command::Action::SelectTab(n)) => match workspace::tab_for_digit(&self.tabs, n) {
                Some(tab) => self.active = tab,
                None => self.status = format!("no workspace {n}"),
            },
            Ok(command::Action::Radio(cmd)) => {
                self.note_request(&cmd);
                self.send(cmd);
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    /// Record what we asked for, so the display can show it as pending
    /// rather than either lying or looking frozen.
    fn note_request(&mut self, command: &Command) {
        match command {
            Command::SetFrequency { hz, .. } | Command::Retune { hz } => {
                self.readout.vfo_a_hz.request(*hz)
            }
            Command::SetMode { mode } => self.readout.mode.request(*mode),
            Command::SetSplit { enabled } => self.readout.split.request(*enabled),
            Command::SetIfShift { hz } => self.readout.if_shift_hz.request(*hz),
            Command::SetFilterWidth { hz } => self.readout.filter_width_hz.request(*hz),
            Command::SetMemoryChannel { channel } => self.readout.memory_channel.request(*channel),
            Command::ReadMeter { .. } => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

fn dim(text: impl Into<String>) -> RichText {
    RichText::new(text).color(theme::DIM).size(11.0)
}

fn absent(text: impl Into<String>) -> RichText {
    RichText::new(text).color(theme::ABSENT).size(11.0)
}

impl Console {
    /// The persistent capability strip: what this radio is, and what is
    /// actually attached to it right now.
    fn strip(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            match self.capabilities() {
                Some(caps) => {
                    ui.label(RichText::new(&caps.model).color(theme::AMBER).strong());
                    ui.label(dim("│"));
                    ui.label(dim("LINK"));
                    ui.label(RichText::new("CAT").color(theme::GREEN).size(11.0));
                    ui.label(dim("│"));
                    ui.label(dim("SIGNAL"));
                    match caps.signal {
                        cat_native::SignalSupport::None => {
                            ui.label(absent("NO SPECTRUM SOURCE"));
                        }
                        cat_native::SignalSupport::IfTapPoint { .. } => {
                            let colour = if self.latest.is_some() {
                                theme::GREEN
                            } else {
                                theme::ABSENT
                            };
                            ui.label(RichText::new("IF-TAP").color(colour).size(11.0));
                        }
                        cat_native::SignalSupport::NativeScope { .. } => {
                            ui.label(RichText::new("SCOPE").color(theme::GREEN).size(11.0));
                        }
                        // `SignalSupport` is `#[non_exhaustive]`: a source
                        // kind added upstream must show as unrecognised
                        // rather than stop this console compiling, and an
                        // unrecognised source is drawn as absent because
                        // that is what it is to a console that cannot read
                        // it.
                        _ => {
                            ui.label(absent("UNKNOWN SOURCE"));
                        }
                    }
                }
                None => {
                    ui.label(RichText::new("NO RADIO").color(theme::RED).strong());
                    ui.label(dim(format!("— {}", self.address)));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.capabilities().is_none() && ui.button("connect").clicked() {
                    // Deferred: `connect` needs &mut self, and this closure
                    // holds &self. Handled by the caller via the return.
                }
            });
        });
    }

    /// The frequency and meter row.
    fn readout_row(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(dim("VFO A"));
            let (text, colour) = match self.readout.vfo_a_hz.value() {
                Some(hz) => (
                    cat_ui::format_hz(hz),
                    if self.readout.vfo_a_hz.is_pending() {
                        theme::AMBER
                    } else {
                        theme::TEXT
                    },
                ),
                // Unknown is drawn as unknown. See `readout`'s docs: the
                // protocol has no read side yet, and a console showing
                // 0.000.000 MHz would be asserting something false.
                None => ("—.———.——— MHz".to_string(), theme::ABSENT),
            };
            ui.label(RichText::new(text).color(colour).size(22.0).strong());

            ui.label(dim("│"));
            ui.label(dim("S"));
            let reading = self.smeter_reading();
            let (w, h) = (140.0, 12.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
            match reading {
                Some(r) => {
                    cat_ui_egui::meter_bar(ui, rect, r, theme::GREEN, theme::LINE_BRIGHT);
                    ui.label(RichText::new(r.s_unit()).color(theme::GREEN).size(12.0));
                }
                None => {
                    ui.painter().rect_filled(rect, 0.0, theme::PANEL_ALT);
                    ui.label(absent("—"));
                }
            }
        });
    }

    /// The S-meter reading, carrying the radio's own range and table.
    ///
    /// Built through `MeterReading` rather than by hand so the raw value
    /// never travels without its scale — raw 15 is mid-scale here and
    /// under 6% on an FT-991A.
    fn smeter_reading(&self) -> Option<MeterReading> {
        let caps = self.capabilities()?;
        let raw = self.readout.smeter_raw.value()?;
        let descriptor = caps
            .meters
            .iter()
            .find(|m| m.kind == cat_native::MeterKind::S)?;
        let mut reading = MeterReading::new(descriptor.kind, raw, descriptor.raw_range);
        if let Some(scale) = descriptor.s_units {
            reading = reading.with_s_units(scale);
        }
        Some(reading)
    }

    /// The always-visible quick controls.
    ///
    /// The design review's one complaint about this direction was that
    /// mode and filters were reachable only by knowing a command. They are
    /// here, and the command line still reaches them too.
    fn quick_bar(&mut self, ui: &mut egui::Ui) {
        let Some(caps) = self.capabilities().cloned() else {
            return;
        };
        let controls = quick::controls(&caps);
        if controls.is_empty() {
            ui.label(absent("this radio has no quick controls"));
            return;
        }
        ui.horizontal(|ui| {
            for control in &controls {
                ui.label(dim(control.label()));
                match control {
                    quick::Control::Mode => {
                        let label = self
                            .readout
                            .mode
                            .value()
                            .and_then(|id| caps.modes.iter().find(|m| m.id == id))
                            .map(|m| m.label.clone())
                            .unwrap_or_else(|| "—".to_string());
                        if ui.button(RichText::new(label).size(12.0)).clicked() {
                            if let Some(next) = quick::next_mode(&caps, self.readout.mode.value()) {
                                self.readout.mode.request(next);
                                self.send(Command::SetMode { mode: next });
                            }
                        }
                    }
                    quick::Control::FilterWidth { widths_hz } => {
                        let current = self.readout.filter_width_hz.value();
                        let label = current.map(|w| format!("{w} Hz")).unwrap_or("—".into());
                        egui::ComboBox::from_id_salt("filter_width")
                            .selected_text(RichText::new(label).size(12.0))
                            .show_ui(ui, |ui| {
                                for width in widths_hz {
                                    if ui
                                        .selectable_label(
                                            current == Some(*width),
                                            format!("{width} Hz"),
                                        )
                                        .clicked()
                                    {
                                        self.readout.filter_width_hz.request(*width);
                                        self.send(Command::SetFilterWidth { hz: *width });
                                    }
                                }
                            });
                    }
                    quick::Control::IfShift { limit_hz } => {
                        let current = self.readout.if_shift_hz.value().unwrap_or(0);
                        ui.label(RichText::new(format!("{current:+} Hz")).size(12.0));
                        for (caption, delta) in [("−", -100), ("+", 100)] {
                            if ui.small_button(caption).clicked() {
                                let next = quick::clamp_shift(current + delta, *limit_hz);
                                self.readout.if_shift_hz.request(next);
                                self.send(Command::SetIfShift { hz: next });
                            }
                        }
                    }
                    quick::Control::Notch => {
                        ui.label(absent("—"));
                    }
                    quick::Control::Split => {
                        let on = self.readout.split.value().unwrap_or(false);
                        let colour = if on { theme::AMBER } else { theme::ABSENT };
                        if ui
                            .button(
                                RichText::new(if on { "ON" } else { "OFF" })
                                    .color(colour)
                                    .size(12.0),
                            )
                            .clicked()
                        {
                            self.readout.split.request(!on);
                            self.send(Command::SetSplit { enabled: !on });
                        }
                    }
                }
                ui.label(dim("│"));
            }
        });
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (i, entry) in self.tabs.clone().iter().enumerate() {
                let selected = self.active == entry.tab;
                let colour = if selected { theme::AMBER } else { theme::DIM };
                let text = RichText::new(format!("{}  {}", i + 1, entry.label))
                    .color(colour)
                    .size(12.0);
                if ui.selectable_label(selected, text).clicked() {
                    self.active = entry.tab;
                }
            }
        });
    }

    /// The waterfall, and the click that tunes it.
    fn spectrum(&mut self, ui: &mut egui::Ui) {
        let Some(caps) = self.capabilities().cloned() else {
            return;
        };
        let available = ui.available_size();
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(available.x, available.y - 4.0), Sense::click());

        ui.painter().rect_filled(rect, 0.0, theme::PANEL_ALT);

        let Some(frame) = self.latest.clone() else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "NO STREAM",
                egui::FontId::proportional(13.0),
                theme::ABSENT,
            );
            return;
        };

        self.waterfall.push(&frame);
        // A fixed reticle at the centre, not a movable cursor. An IF tap is
        // dial-centred by construction, so the dial IS the centre -- see
        // `tuning`.
        let x = rect.center().x;
        ui.painter().line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xe6, 0xab, 0x44, 90)),
        );

        if let Some(pos) = response.interact_pointer_pos() {
            if response.clicked() && rect.width() > 0.0 {
                let fraction = (pos.x - rect.left()) / rect.width();
                match tuning::tune_target(
                    &frame,
                    fraction,
                    &caps.tuning_steps_hz,
                    caps.rx_range.min_hz,
                    caps.rx_range.max_hz,
                ) {
                    Some(hz) => {
                        self.readout.vfo_a_hz.request(hz);
                        // Retune, not SetFrequency: this moves the dial and
                        // the IF-tap source with it, which is what makes
                        // the picture recentre.
                        self.send(Command::Retune { hz });
                        self.status = format!("tuning {}", cat_ui::format_hz(hz));
                    }
                    None => {
                        self.status = "outside this radio's coverage".to_string();
                    }
                }
            }
        }
    }

    fn command_line(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.command_open {
                ui.label(RichText::new(":").color(theme::SELECT_LINE).strong());
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.command_text)
                        .desired_width(f32::INFINITY)
                        .frame(false),
                );
                edit.request_focus();
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.run_line();
                    self.command_open = false;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.command_open = false;
                    self.command_text.clear();
                }
            } else {
                ui.label(dim(&self.status));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(absent("1–9 switch · : command"));
                });
            }
        });
    }
}

impl eframe::App for Console {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump();
        // Spectrum arrives on its own schedule, not on input, so the
        // console has to ask to be woken rather than waiting for a click.
        ctx.request_repaint_after(std::time::Duration::from_millis(33));

        let mut wants_connect = false;
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Colon)
                || (i.modifiers.shift && i.key_pressed(egui::Key::Semicolon))
            {
                self.command_open = true;
            }
            for (n, key) in [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
                egui::Key::Num5,
            ]
            .into_iter()
            .enumerate()
            {
                if i.key_pressed(key) && !self.command_open {
                    if let Some(tab) = workspace::tab_for_digit(&self.tabs, n + 1) {
                        self.active = tab;
                    }
                }
            }
        });

        egui::TopBottomPanel::top("strip").show(ctx, |ui| {
            self.strip(ui);
            ui.separator();
            self.readout_row(ui);
            self.quick_bar(ui);
            self.tab_bar(ui);
        });

        egui::TopBottomPanel::bottom("command").show(ctx, |ui| self.command_line(ui));

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.capabilities().is_none() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(RichText::new("NOT CONNECTED").color(theme::RED).size(16.0));
                    // The reason travels with the state rather than only
                    // in `status`, which the next message would overwrite.
                    let why = match &self.link {
                        Link::Down(why) => why.as_str(),
                        Link::Up(_) => "",
                    };
                    ui.label(dim(format!("{} — {why}", self.address)));
                    if ui.button("connect").clicked() {
                        wants_connect = true;
                    }
                });
                return;
            }
            match self.active {
                Tab::Spectrum => self.spectrum(ui),
                Tab::Memory => {
                    ui.label(dim("memory workspace"));
                    ui.label(absent(
                        "the protocol has no read side yet — see docs/renderer-parity.md",
                    ));
                }
                Tab::Menu => {
                    ui.label(dim("menu workspace"));
                    ui.label(absent(
                        "the protocol has no read side yet — see docs/renderer-parity.md",
                    ));
                }
                Tab::Source => {
                    ui.label(dim("attached sources"));
                    let installed = self
                        .capabilities()
                        .map(|c| c.installation.sources.len())
                        .unwrap_or(0);
                    if installed == 0 {
                        ui.label(absent("nothing attached"));
                    }
                }
            }
        });

        if wants_connect {
            self.connect();
        }
    }
}
