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
    last_state_request: std::time::Instant,
    /// Capabilities for a still, when there is no link to get them from.
    offline_capabilities: Option<cat_native::CapabilitiesWire>,
    /// GPU copy of the waterfall, re-uploaded as rows arrive.
    waterfall_texture: Option<egui::TextureHandle>,
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
            last_state_request: std::time::Instant::now(),
            offline_capabilities: None,
            waterfall_texture: None,
        }
    }

    /// Fill the readout with plausible values, for the offscreen renderer.
    ///
    /// Not a demo mode: it exists so a still of the console shows the
    /// layout under load rather than a row of em dashes, which is what an
    /// unconnected console correctly shows and which says nothing about
    /// whether the design is right.
    /// Install a capability set without a socket, so a still shows the
    /// real layout rather than the disconnected state.
    pub fn demo_capabilities(&mut self, caps: cat_native::CapabilitiesWire) {
        self.tabs = crate::workspace::tabs(&caps);
        self.active = self.tabs.first().map(|t| t.tab).unwrap_or(Tab::Source);
        self.offline_capabilities = Some(caps);
    }

    /// Push a spectrum frame in, for a still.
    ///
    /// A screenshot showing NO STREAM proves the empty state and nothing
    /// about the waterfall, which is the part of this console with the
    /// most that can go wrong.
    pub fn demo_spectrum(&mut self, frames: &[cat_signal::SpectrumFrame]) {
        for frame in frames {
            self.waterfall.push(frame);
        }
        self.latest = frames.last().cloned();
    }

    pub fn demo_state(&mut self) {
        self.readout.vfo_a_hz.confirm(14_074_000);
        self.readout.mode.confirm(cat_native::ModeId::Usb);
        self.readout.split.confirm(false);
        self.readout.smeter_raw.confirm(17);
        self.readout.if_shift_hz.confirm(0);
        self.status = "connected to Kenwood TS-570D".to_string();
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
            Link::Down(_) => self.offline_capabilities.as_ref(),
        }
    }

    /// Drain everything the reader thread has for us. Never blocks.
    fn pump(&mut self) {
        let mut lost = None;
        let mut confirmed = None;
        if let Link::Up(client) = &self.link {
            while let Some(event) = client.try_event() {
                match event {
                    Event::Reply(ServerMessage::Error { code, message }) => {
                        self.status = format!("{code:?}: {message}");
                    }
                    // The radio's own account of itself. It wins over
                    // anything this console asked for -- see
                    // `readout::Field::confirm`.
                    Event::Reply(ServerMessage::State(state)) => {
                        confirmed = Some(*state);
                    }
                    Event::Reply(ServerMessage::Meter(sample)) => {
                        if sample.kind == cat_native::MeterKind::S {
                            self.readout.smeter_raw.confirm(sample.raw);
                        }
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
        if let Some(state) = confirmed {
            self.readout.vfo_a_hz.confirm(state.vfo_a_hz);
            self.readout.mode.confirm(state.mode);
            self.readout.split.confirm(state.split);
            if let Some(hz) = state.if_shift_hz {
                self.readout.if_shift_hz.confirm(hz);
            }
            if let Some(hz) = state.filter_width_hz {
                self.readout.filter_width_hz.confirm(hz);
            }
            if let Some(channel) = state.memory_channel {
                self.readout.memory_channel.confirm(channel);
            }
            if let Some(raw) = state.meter(cat_native::MeterKind::S) {
                self.readout.smeter_raw.confirm(raw);
            }
        }
        if let Some(why) = lost {
            self.status = format!("connection lost: {why}");
            self.link = Link::Down(why);
        }
    }

    /// Ask the radio what it is doing, at a rate a human can read.
    ///
    /// Ten times a second, not per frame. State is request/response over
    /// the same socket the spectrum uses, and asking at frame rate would
    /// put sixty round trips a second in front of the traffic that
    /// actually needs to be fast -- ADR 0011's two-rate discipline, which
    /// exists precisely so a menu read cannot stall a waterfall.
    fn poll_state(&mut self) {
        const INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
        if self.last_state_request.elapsed() < INTERVAL {
            return;
        }
        self.last_state_request = std::time::Instant::now();
        if let Link::Up(client) = &self.link {
            client.request_state();
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
            // Reads ask a question; they do not request a change.
            Command::ReadMeter { .. } | Command::ReadState => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Drawing
//
// Placement only. The structure follows the accepted mockup: a persistent
// strip of key-over-value fields, a left rail of meters that is always
// there, capability-derived tabs, and a command line. Everything is
// square, tight and bordered -- an instrument panel, not a form.
// ---------------------------------------------------------------------------

fn key(text: impl Into<String>) -> RichText {
    RichText::new(text.into().to_uppercase())
        .color(theme::DIM)
        .size(theme::SIZE_KEY)
}

fn value(text: impl Into<String>, colour: Color32) -> RichText {
    RichText::new(text).color(colour).size(theme::SIZE_VALUE)
}

fn dim(text: impl Into<String>) -> RichText {
    RichText::new(text).color(theme::DIM).size(theme::SIZE_BODY)
}

fn absent(text: impl Into<String>) -> RichText {
    RichText::new(text)
        .color(theme::ABSENT)
        .size(theme::SIZE_BODY)
}

/// A hairline, for separating regions.
fn rule(ui: &mut egui::Ui, horizontal: bool) {
    let rect = ui.max_rect();
    let (a, b) = if horizontal {
        (
            egui::pos2(rect.left(), rect.top()),
            egui::pos2(rect.right(), rect.top()),
        )
    } else {
        (
            egui::pos2(rect.left(), rect.top()),
            egui::pos2(rect.left(), rect.bottom()),
        )
    };
    ui.painter()
        .line_segment([a, b], Stroke::new(1.0, theme::LINE));
}

impl Console {
    /// One field of the persistent strip: a dim key with a value under it.
    ///
    /// The mockup's basic unit. Stacking the label above the value is what
    /// lets twelve facts sit in one strip and still be scannable — inline
    /// `KEY: value` pairs need separators and twice the width.
    fn field(ui: &mut egui::Ui, k: &str, v: RichText, width: f32) {
        ui.allocate_ui(Vec2::new(width, 34.0), |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(key(k));
                ui.label(v);
            });
        });
    }

    /// The persistent capability strip.
    fn strip(&mut self, ui: &mut egui::Ui) {
        let caps = self.capabilities().cloned();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;

            match &caps {
                Some(c) => Self::field(ui, "radio", value(&c.model, theme::AMBER), 150.0),
                None => Self::field(ui, "radio", value("NO RADIO", theme::RED), 150.0),
            }

            let (link_text, link_colour) = match &self.link {
                Link::Up(_) => (format!("◆ {}", self.address), theme::GREEN),
                Link::Down(_) => (format!("◇ {}", self.address), theme::ABSENT),
            };
            Self::field(ui, "link", value(link_text, link_colour).size(11.0), 190.0);

            // The dial. The one thing on the strip that gets read from
            // across the room, so it is the one thing that is large.
            let mode_label = caps
                .as_ref()
                .and_then(|c| {
                    self.readout
                        .mode
                        .value()
                        .and_then(|id| c.modes.iter().find(|m| m.id == id))
                })
                .map(|m| m.label.clone())
                .unwrap_or_else(|| "—".to_string());
            ui.allocate_ui(Vec2::new(280.0, 34.0), |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(key(format!("vfo a · {mode_label}")));
                    let (text, colour) = match self.readout.vfo_a_hz.value() {
                        Some(hz) => (
                            cat_ui::format_hz_compact(hz),
                            if self.readout.vfo_a_hz.is_pending() {
                                theme::AMBER
                            } else {
                                theme::TEXT
                            },
                        ),
                        None => ("—.———.———".to_string(), theme::ABSENT),
                    };
                    ui.label(RichText::new(text).color(colour).size(theme::SIZE_DIAL));
                });
            });

            let split = match self.readout.split.value() {
                Some(true) => value("ON", theme::AMBER),
                Some(false) => value("OFF", theme::DIM),
                None => value("—", theme::ABSENT),
            };
            Self::field(ui, "split", split, 60.0);

            let s = match self.smeter_reading() {
                Some(r) => value(
                    format!("{}  {}/{}", r.s_unit(), r.raw, r.range.max),
                    theme::TEXT,
                ),
                None => value("—", theme::ABSENT),
            };
            Self::field(ui, "s", s, 130.0);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let state = if self.readout.split.value().is_some() {
                    value("RX", theme::GREEN)
                } else {
                    value("—", theme::ABSENT)
                };
                Self::field(ui, "state", state, 44.0);

                let signal = match caps.as_ref().map(|c| c.signal) {
                    Some(cat_native::SignalSupport::IfTapPoint { .. }) => {
                        let live = self.latest.is_some();
                        value("IF-TAP", if live { theme::GREEN } else { theme::ABSENT })
                    }
                    Some(cat_native::SignalSupport::None) => value("NONE", theme::ABSENT),
                    Some(_) => value("SCOPE", theme::GREEN),
                    None => value("—", theme::ABSENT),
                };
                Self::field(ui, "signal · rf", signal.size(11.0), 80.0);
            });
        });
    }

    /// The left rail: every meter this radio has, always visible.
    ///
    /// A rail rather than a row, and never reflowed. A meter that is inert
    /// keeps its place dimmed — a TX meter appearing and vanishing on every
    /// transmit would make the whole panel jump.
    fn rail(&mut self, ui: &mut egui::Ui) {
        let Some(caps) = self.capabilities().cloned() else {
            ui.label(absent("no radio"));
            return;
        };
        Self::pane_header(ui, "METERS · MeterSet", None);
        ui.add_space(4.0);

        for descriptor in &caps.meters {
            let reading = if descriptor.kind == cat_native::MeterKind::S {
                self.smeter_reading()
            } else {
                None
            };
            let active = reading.is_some();
            let label_colour = if active { theme::TEXT } else { theme::ABSENT };

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{:?}", descriptor.kind).to_uppercase())
                        .color(label_colour)
                        .size(theme::SIZE_BODY),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let v = reading
                        .map(|r| r.raw.to_string())
                        .unwrap_or_else(|| "—".to_string());
                    ui.label(RichText::new(v).color(label_colour).size(theme::SIZE_BODY));
                });
            });

            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 9.0), Sense::hover());
            ui.painter().rect_filled(rect, 0.0, theme::PANEL_ALT);
            ui.painter()
                .rect_stroke(rect, 0.0, Stroke::new(1.0, theme::LINE));
            if let Some(r) = reading {
                cat_ui_egui::meter_bar(ui, rect.shrink(1.0), r, theme::GREEN, theme::PANEL_ALT);
            }
            ui.add_space(7.0);
        }
    }

    /// A pane header: dim uppercase left, an optional note right.
    fn pane_header(ui: &mut egui::Ui, left: &str, right: Option<&str>) {
        let height = 17.0;
        let rect = ui
            .allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover())
            .0;
        ui.painter().rect_filled(rect, 0.0, theme::PANEL_ALT);
        ui.painter().line_segment(
            [
                egui::pos2(rect.left(), rect.bottom()),
                egui::pos2(rect.right(), rect.bottom()),
            ],
            Stroke::new(1.0, theme::LINE_BRIGHT),
        );
        ui.painter().text(
            egui::pos2(rect.left() + 7.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            left,
            egui::FontId::monospace(theme::SIZE_KEY),
            theme::DIM,
        );
        if let Some(right) = right {
            ui.painter().text(
                egui::pos2(rect.right() - 7.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                right,
                egui::FontId::monospace(theme::SIZE_KEY),
                theme::DIMMER,
            );
        }
    }

    /// The tab bar, derived from what the radio says it is.
    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let entries = self.tabs.clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, entry) in entries.iter().enumerate() {
                let selected = self.active == entry.tab;
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(entry.label.len() as f32 * 8.0 + 46.0, 26.0),
                    Sense::click(),
                );
                if selected {
                    ui.painter().rect_filled(rect, 0.0, theme::PANEL);
                    ui.painter().line_segment(
                        [
                            egui::pos2(rect.left(), rect.bottom() - 1.0),
                            egui::pos2(rect.right(), rect.bottom() - 1.0),
                        ],
                        Stroke::new(2.0, theme::AMBER),
                    );
                }
                let colour = if selected { theme::AMBER } else { theme::DIM };
                // The digit that selects it, then the name. The digits are
                // the whole reason this design can hold TUI parity.
                ui.painter().text(
                    egui::pos2(rect.left() + 10.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!("{}", i + 1),
                    egui::FontId::monospace(theme::SIZE_KEY),
                    theme::DIMMER,
                );
                ui.painter().text(
                    egui::pos2(rect.left() + 24.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &entry.label,
                    egui::FontId::monospace(theme::SIZE_BODY),
                    colour,
                );
                if response.clicked() {
                    self.active = entry.tab;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(10.0);
                ui.label(
                    RichText::new("1–9 switch · : command")
                        .color(theme::DIMMER)
                        .size(theme::SIZE_KEY),
                );
            });
        });
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
            return;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for control in &controls {
                ui.label(key(control.label()));
                match control {
                    quick::Control::Mode => {
                        let label = self
                            .readout
                            .mode
                            .value()
                            .and_then(|id| caps.modes.iter().find(|m| m.id == id))
                            .map(|m| m.label.clone())
                            .unwrap_or_else(|| "—".to_string());
                        if ui
                            .add(egui::Button::new(
                                RichText::new(format!("{label:<8}"))
                                    .color(theme::TEXT)
                                    .size(theme::SIZE_BODY),
                            ))
                            .clicked()
                        {
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
                            .selected_text(RichText::new(label).size(theme::SIZE_BODY))
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
                        ui.label(
                            RichText::new(format!("{current:+5} Hz"))
                                .color(theme::TEXT)
                                .size(theme::SIZE_BODY),
                        );
                        for (caption, delta) in [("−", -100), ("+", 100)] {
                            if ui.small_button(caption).clicked() {
                                let next = quick::clamp_shift(current + delta, *limit_hz);
                                self.readout.if_shift_hz.request(next);
                                self.send(Command::SetIfShift { hz: next });
                            }
                        }
                    }
                    quick::Control::Notch => {}
                    quick::Control::Split => {
                        let on = self.readout.split.value().unwrap_or(false);
                        let colour = if on { theme::AMBER } else { theme::DIM };
                        if ui
                            .add(egui::Button::new(
                                RichText::new(if on { "ON " } else { "OFF" })
                                    .color(colour)
                                    .size(theme::SIZE_BODY),
                            ))
                            .clicked()
                        {
                            self.readout.split.request(!on);
                            self.send(Command::SetSplit { enabled: !on });
                        }
                    }
                }
                ui.label(
                    RichText::new("│")
                        .color(theme::LINE_BRIGHT)
                        .size(theme::SIZE_BODY),
                );
            }
        });
    }

    /// The S-meter reading, carrying the radio's own range and table.
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

    /// The waterfall, and the click that tunes it.
    fn spectrum(&mut self, ui: &mut egui::Ui) {
        let Some(caps) = self.capabilities().cloned() else {
            return;
        };
        let span = match caps.signal {
            cat_native::SignalSupport::IfTapPoint { .. } => "IF TAP · CN4 → RTL-SDR",
            _ => "SPECTRUM",
        };
        Self::pane_header(
            ui,
            span,
            Some("click to tune · snaps to the radio's finest step"),
        );

        let available = ui.available_size();
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(available.x, available.y.max(1.0)), Sense::click());
        ui.painter()
            .rect_filled(rect, 0.0, Color32::from_rgb(4, 7, 10));

        let Some(frame) = self.latest.clone() else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "NO STREAM",
                egui::FontId::monospace(13.0),
                theme::ABSENT,
            );
            return;
        };

        self.waterfall.push(&frame);

        // Paint it. The buffer was being filled and never drawn -- a
        // waterfall the console maintained and never showed, which is the
        // kind of thing only looking at the thing catches.
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [
                self.waterfall.width() as usize,
                self.waterfall.height() as usize,
            ],
            &self.waterfall.rgba(),
        );
        match &mut self.waterfall_texture {
            Some(handle) => handle.set(image, egui::TextureOptions::NEAREST),
            None => {
                self.waterfall_texture = Some(ui.ctx().load_texture(
                    "waterfall",
                    image,
                    egui::TextureOptions::NEAREST,
                ))
            }
        }
        if let Some(handle) = &self.waterfall_texture {
            ui.painter().image(
                handle.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        // A fixed reticle at the centre, not a movable cursor. An IF tap is
        // dial-centred by construction, so the dial IS the centre.
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
                ui.label(
                    RichText::new(":")
                        .color(theme::SELECT_LINE)
                        .size(theme::SIZE_BODY)
                        .strong(),
                );
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.command_text)
                        .desired_width(f32::INFINITY)
                        .font(egui::FontId::monospace(theme::SIZE_BODY))
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
                ui.label(
                    RichText::new(&self.status)
                        .color(theme::DIM)
                        .size(theme::SIZE_KEY),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new("press :  for a command")
                            .color(theme::DIMMER)
                            .size(theme::SIZE_KEY),
                    );
                });
            }
        });
    }
}

impl eframe::App for Console {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.draw(ctx);
    }
}

impl Console {
    /// One frame, against a bare `egui::Context`.
    ///
    /// Split out from `eframe::App::update` so the console can be drawn
    /// without a window. `examples/render.rs` rasterises this offscreen
    /// through lavapipe and writes a PNG, which is the only way to *look*
    /// at a change here on a machine with no display — and looking is the
    /// thing that was missing when this first shipped not resembling the
    /// design it was built from.
    pub fn draw(&mut self, ctx: &egui::Context) {
        self.pump();
        self.poll_state();
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

        egui::TopBottomPanel::top("strip")
            .frame(
                egui::Frame::none()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin {
                        left: 10.0,
                        right: 10.0,
                        top: 6.0,
                        bottom: 0.0,
                    }),
            )
            .show(ctx, |ui| {
                self.strip(ui);
                ui.add_space(4.0);
                self.quick_bar(ui);
                ui.add_space(2.0);
                self.tab_bar(ui);
            });

        egui::TopBottomPanel::bottom("command")
            .exact_height(24.0)
            .frame(
                egui::Frame::none()
                    .fill(theme::PANEL_ALT)
                    .inner_margin(egui::Margin::symmetric(10.0, 4.0)),
            )
            .show(ctx, |ui| self.command_line(ui));

        // The rail is part of the console's permanent furniture, not part
        // of a workspace, so it lives outside the central panel.
        egui::SidePanel::left("rail")
            .exact_width(230.0)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(8.0, 0.0)),
            )
            .show(ctx, |ui| {
                rule(ui, false);
                self.rail(ui);
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG))
            .show(ctx, |ui| {
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
