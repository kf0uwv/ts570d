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

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::control::{group_command_labels, ControlState};
use crate::diag::{DiagResult, DiagState};
use crate::terminal::DIAG_STEP_COUNT;
use crate::RadioDisplay;

// Shared console logic and terminal widgets (radio-cat-rs ADR 0011 rev 4).
// What stays here is this radio's LAYOUT and FEATURE SET; what comes from
// these crates is anything with one correct answer per input.
use cat_framework::capabilities::MeterKind;
use cat_ui::{format_hz, MeterReading};
use cat_ui_ratatui::{
    bar_spans, error_panel, header, link_panel, menu_column, meter_spans, ErrorPanelStyles,
    LinkState,
};

/// This radio's S-meter, with the raw value the last poll returned.
///
/// Goes through `from_meters` rather than being built by hand so the
/// reading arrives carrying both its 0-30 range and this radio's S-unit
/// table. Neither is a thing the widgets should have to be told.
fn smeter_reading(state: &RadioDisplay) -> Option<MeterReading> {
    MeterReading::from_meters(
        &radio::capabilities::TS570D.meters,
        MeterKind::S,
        state.smeter,
    )
}

/// Build AGC label from numeric code.
fn agc_label(agc: u8) -> &'static str {
    match agc {
        0 => "Off",
        1 => "Slow",
        2 => "Mid",
        3 => "Fast",
        _ => "?",
    }
}

/// Build noise reduction label.
fn nr_label(nr: u8) -> &'static str {
    match nr {
        1 => "NR1",
        2 => "NR2",
        _ => "OFF",
    }
}

/// Build beat cancel label.
fn bc_label(bc: u8) -> &'static str {
    match bc {
        1 => "BC1",
        2 => "BC2",
        _ => "OFF",
    }
}

/// Style for an ON indicator.
fn on_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

/// Style for an OFF indicator.
fn off_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

// ---------------------------------------------------------------------------
// Top-level layout splitter
// ---------------------------------------------------------------------------

/// Split the full terminal area into (header, status, errors, controls) areas.
pub fn split_areas(area: Rect) -> (Rect, Rect, Rect, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(7), // Status
            Constraint::Length(5), // Errors  (border + 3 lines)
            Constraint::Min(8),    // Controls
        ])
        .split(area);
    (chunks[0], chunks[1], chunks[2], chunks[3])
}

// ---------------------------------------------------------------------------
// draw_header
// ---------------------------------------------------------------------------

/// Draw the TS-570D title header block.
pub fn draw_header(f: &mut Frame, area: Rect) {
    header(
        "TS-570D RADIO CONTROL",
        Alignment::Center,
        area,
        f.buffer_mut(),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
}

// ---------------------------------------------------------------------------
// draw_errors — poll error panel
// ---------------------------------------------------------------------------

/// Draw the poll error panel.
///
/// The slot is reserved whether or not anything went wrong, so an empty
/// list draws "No errors" rather than nothing — an empty bordered box
/// reads as a panel that has failed, not one with nothing to say.
///
/// One thing changed when this moved onto the shared widget: the three
/// errors shown are now the **most recent** three rather than the first
/// three. A radio failing in a loop used to pin this panel to its oldest
/// failures and never show the current one. Recorded in
/// `docs/renderer-parity.md`.
pub fn draw_errors(f: &mut Frame, area: Rect, state: &RadioDisplay) {
    error_panel(
        &state.poll_errors,
        "Errors",
        ErrorPanelStyles {
            error: Style::default().fg(Color::Red),
            quiet: Some(("No errors", Style::default().fg(Color::DarkGray))),
        },
        area,
        f.buffer_mut(),
    );
}

// ---------------------------------------------------------------------------
// draw_disconnected — connection-lost overlay (replaces control panel)
// ---------------------------------------------------------------------------

/// Draw a full-panel overlay when the radio is unreachable or still connecting.
///
/// This replaces the control panel outright, so the `[Q] Quit` footer is
/// the only thing on screen telling the operator which key still works.
pub fn draw_disconnected(f: &mut Frame, area: Rect, errors: &[String], initializing: bool) {
    let state = if initializing {
        LinkState::Connecting
    } else {
        LinkState::Lost
    };
    link_panel(
        state,
        errors,
        "Radio Status",
        Some(Span::styled("[Q] Quit", Style::default().fg(Color::White))),
        area,
        f.buffer_mut(),
    );
}

// ---------------------------------------------------------------------------
// draw_ui — status panel (accepts explicit area)
// ---------------------------------------------------------------------------

pub fn draw_ui(f: &mut Frame, area: Rect, state: &RadioDisplay) {
    // Outer block with title
    let outer_block = Block::default().title(" Status ").borders(Borders::ALL);
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    // Inner vertical layout: 5 rows
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Row 1: Primary (VFO + S-meter)
            Constraint::Length(1), // Row 2: Gains
            Constraint::Length(1), // Row 3: Receiver features
            Constraint::Length(1), // Row 4: Flags
            Constraint::Length(1), // Row 5: Status bar
            Constraint::Min(0),    // Filler
        ])
        .split(inner);

    // -----------------------------------------------------------------------
    // Row 1 — Primary
    // -----------------------------------------------------------------------

    let smeter = smeter_reading(state);
    let label = smeter.map(|r| r.s_unit()).unwrap_or("--");
    let (tx_text, tx_color) = if state.tx {
        ("TX", Color::Red)
    } else {
        ("RX", Color::Green)
    };

    let mut line1_spans = vec![
        Span::styled("VFO A  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format_hz(state.vfo_a_hz),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{:<6}", state.mode),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("  S "),
        Span::styled("▐", Style::default().fg(Color::Green)),
    ];
    // The end caps are layout and stay here; the 20 cells between them are
    // the shared bar. Both halves keep the green this panel has always
    // used -- the block characters carry the contrast -- so the only thing
    // an operator sees change is that the bar now resolves eight sub-levels
    // per cell instead of whole cells.
    line1_spans.extend(match smeter {
        Some(r) => meter_spans(
            r,
            20,
            Style::default().fg(Color::Green),
            Style::default().fg(Color::Green),
        ),
        None => bar_spans(
            0.0,
            20,
            Style::default().fg(Color::DarkGray),
            Style::default().fg(Color::DarkGray),
        ),
    });
    line1_spans.extend([
        Span::styled("▌", Style::default().fg(Color::Green)),
        Span::raw(" "),
        Span::styled(format!("{:<6}", label), Style::default().fg(Color::Green)),
        Span::styled(
            tx_text,
            Style::default().fg(tx_color).add_modifier(Modifier::BOLD),
        ),
    ]);
    let line1 = Line::from(line1_spans);

    let mut line2_spans = vec![
        Span::styled("VFO B  ", Style::default().fg(Color::DarkGray)),
        Span::styled(format_hz(state.vfo_b_hz), Style::default().fg(Color::White)),
        Span::raw("  "),
    ];

    if state.rit {
        line2_spans.push(Span::styled(
            format!("RIT: {:+}Hz  ", state.rit_xit_offset_hz),
            Style::default().fg(Color::Yellow),
        ));
    }
    if state.xit {
        line2_spans.push(Span::styled(
            format!("XIT: {:+}Hz  ", state.rit_xit_offset_hz),
            Style::default().fg(Color::Yellow),
        ));
    }
    if !state.rit && !state.xit {
        line2_spans.push(Span::styled("RIT:OFF  XIT:OFF  ", off_style()));
    }

    let split_style = if state.split { on_style() } else { off_style() };
    let split_text = if state.split {
        "Split:ON  "
    } else {
        "Split:OFF  "
    };
    line2_spans.push(Span::styled(split_text, split_style));

    if state.memory_mode {
        line2_spans.push(Span::styled(
            format!("CH: {:02}", state.memory_channel),
            Style::default().fg(Color::Cyan),
        ));
    } else {
        line2_spans.push(Span::styled(
            format!("ANT:{}", state.antenna),
            Style::default().fg(Color::DarkGray),
        ));
    }

    let primary = Paragraph::new(vec![line1, Line::from(line2_spans)]);
    f.render_widget(primary, rows[0]);

    // -----------------------------------------------------------------------
    // Row 2 — Gains
    // -----------------------------------------------------------------------

    let label_style = Style::default().fg(Color::DarkGray);
    let value_style = Style::default().fg(Color::White);
    let bracket_style = Style::default().fg(Color::DarkGray);
    let filled_style = Style::default().fg(Color::Yellow);
    let empty_style = Style::default().fg(Color::DarkGray);

    // These used to build a bar string and then filter it character by
    // character back into the two halves the line needs. `bar_spans`
    // returns those halves directly -- it is the same bar `meter_bar`
    // draws, in the shape this panel composes in.
    let af = bar_spans(state.af_gain as f32 / 255.0, 10, filled_style, empty_style);
    let rf = bar_spans(state.rf_gain as f32 / 255.0, 10, filled_style, empty_style);
    let mic = bar_spans(state.mic_gain as f32 / 100.0, 10, filled_style, empty_style);

    let mut gain_spans = vec![
        Span::styled("AF:", label_style),
        Span::styled("[", bracket_style),
    ];
    gain_spans.extend(af);
    gain_spans.extend([
        Span::styled("]", bracket_style),
        Span::raw("  "),
        Span::styled("RF:", label_style),
        Span::styled("[", bracket_style),
    ]);
    gain_spans.extend(rf);
    gain_spans.extend([
        Span::styled("]", bracket_style),
        Span::raw("  "),
        Span::styled("MIC:", label_style),
        Span::styled("[", bracket_style),
    ]);
    gain_spans.extend(mic);
    gain_spans.extend([
        Span::styled("]", bracket_style),
        Span::raw("  "),
        Span::styled("SQL:", label_style),
        Span::styled(format!("{:>3}", state.squelch), value_style),
        Span::raw("  "),
        Span::styled("PWR:", label_style),
        Span::styled(format!("{:3}W", state.power_pct), value_style),
        Span::raw("  "),
        Span::styled("AGC:", label_style),
        Span::styled(agc_label(state.agc), value_style),
    ]);
    let gains_line = Line::from(gain_spans);

    f.render_widget(Paragraph::new(gains_line), rows[1]);

    // -----------------------------------------------------------------------
    // Row 3 — Receiver features
    // -----------------------------------------------------------------------

    let nb_style = if state.noise_blanker {
        on_style()
    } else {
        off_style()
    };
    let nb_text = if state.noise_blanker { "ON " } else { "OFF" };

    let nr_text = nr_label(state.noise_reduction);
    let nr_style = if state.noise_reduction != 0 {
        on_style()
    } else {
        off_style()
    };

    let att_style = if state.attenuator {
        on_style()
    } else {
        off_style()
    };
    let att_text = if state.attenuator { "ON " } else { "OFF" };

    let pre_style = if state.preamp {
        on_style()
    } else {
        off_style()
    };
    let pre_text = if state.preamp { "ON " } else { "OFF" };

    let proc_style = if state.speech_processor {
        on_style()
    } else {
        off_style()
    };
    let proc_text = if state.speech_processor { "ON " } else { "OFF" };

    let vox_style = if state.vox { on_style() } else { off_style() };
    let vox_text = if state.vox { "ON " } else { "OFF" };

    let bc_text = bc_label(state.beat_cancel);
    let bc_style = if state.beat_cancel != 0 {
        on_style()
    } else {
        off_style()
    };

    let rx_line = Line::from(vec![
        Span::styled("NB:", label_style),
        Span::styled(nb_text, nb_style),
        Span::raw("  "),
        Span::styled("NR:", label_style),
        Span::styled(nr_text, nr_style),
        Span::raw("  "),
        Span::styled("ATT:", label_style),
        Span::styled(att_text, att_style),
        Span::raw("  "),
        Span::styled("PRE:", label_style),
        Span::styled(pre_text, pre_style),
        Span::raw("  "),
        Span::styled("PROC:", label_style),
        Span::styled(proc_text, proc_style),
        Span::raw("  "),
        Span::styled("VOX:", label_style),
        Span::styled(vox_text, vox_style),
        Span::raw("  "),
        Span::styled("BC:", label_style),
        Span::styled(bc_text, bc_style),
    ]);

    f.render_widget(Paragraph::new(rx_line), rows[2]);

    // -----------------------------------------------------------------------
    // Row 4 — Flags
    // -----------------------------------------------------------------------

    let scan_style = if state.scan { on_style() } else { off_style() };
    let scan_text = if state.scan { "ON " } else { "OFF" };

    let lock_style = if state.freq_lock {
        on_style()
    } else {
        off_style()
    };
    let lock_text = if state.freq_lock { "ON " } else { "OFF" };

    let fine_style = if state.fine_step {
        on_style()
    } else {
        off_style()
    };
    let fine_text = if state.fine_step { "ON " } else { "OFF" };

    let ctcss_style = if state.ctcss { on_style() } else { off_style() };
    let ctcss_text = if state.ctcss { "ON " } else { "OFF" };

    let flags_line = Line::from(vec![
        Span::styled("Scan:", label_style),
        Span::styled(scan_text, scan_style),
        Span::raw("  "),
        Span::styled("Lock:", label_style),
        Span::styled(lock_text, lock_style),
        Span::raw("  "),
        Span::styled("Fine:", label_style),
        Span::styled(fine_text, fine_style),
        Span::raw("  "),
        Span::styled("CTCSS:", label_style),
        Span::styled(ctcss_text, ctcss_style),
    ]);

    f.render_widget(Paragraph::new(flags_line), rows[3]);

    // -----------------------------------------------------------------------
    // Row 5 — Status bar
    // -----------------------------------------------------------------------

    let rx_vfo_label = match state.rx_vfo {
        0 => "VFO-A",
        1 => "VFO-B",
        2 => "MEM",
        _ => "?",
    };
    let ant_label = match state.antenna {
        1 => "ANT1",
        2 => "ANT2",
        _ => "ANT?",
    };
    let status_line = Line::from(vec![
        Span::styled(
            "TS-570D Radio Control",
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("  |  "),
        Span::styled("RX:", Style::default().fg(Color::DarkGray)),
        Span::styled(rx_vfo_label, Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled(ant_label, Style::default().fg(Color::White)),
        Span::raw("  |  "),
        Span::styled("q: quit", Style::default().fg(Color::DarkGray)),
    ]);

    f.render_widget(Paragraph::new(status_line), rows[4]);
}

// ---------------------------------------------------------------------------
// draw_control_panel
// ---------------------------------------------------------------------------

/// The yellow-key styling both menu columns use.
///
/// The columns themselves come from `cat_ui_ratatui::menu_column`, which
/// is generic over both cell types -- this crate had two copies of it that
/// differed only in whether the labels were `&'static str` or built at
/// runtime.
fn menu_key_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

/// Draw the interactive control panel.
pub fn draw_control_panel(f: &mut Frame, area: Rect, state: &ControlState) {
    if let ControlState::Diagnostic(diag_state) = state {
        draw_diag_panel(f, area, diag_state);
        return;
    }
    if let ControlState::DiagWarning = state {
        draw_diag_warning_panel(f, area);
        return;
    }

    let outer_block = Block::default().title(" Controls ").borders(Borders::ALL);
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    match state {
        ControlState::Menu => {
            // Split inner area: content rows above, prompt line at bottom.
            let sections = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(inner);
            let content_area = sections[0];
            let prompt_area = sections[1];

            // Split content area into 2 equal columns.
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(content_area);

            let left: &[(&str, &str)] = &[
                ("F", "Freq"),
                ("N", "Mem"),
                ("M", "Mode/DSP"),
                ("R", "Receive"),
                ("T", "Transmit"),
            ];
            let right: &[(&str, &str)] = &[
                ("C", "CW"),
                ("O", "Tones"),
                ("S", "System"),
                ("D", "Diag"),
                ("Q", "Quit"),
            ];

            f.render_widget(
                Paragraph::new(menu_column(left, menu_key_style(), Style::default())),
                cols[0],
            );
            f.render_widget(
                Paragraph::new(menu_column(right, menu_key_style(), Style::default())),
                cols[1],
            );
            f.render_widget(Paragraph::new(">"), prompt_area);
        }

        ControlState::GroupMenu { group, .. } => {
            let labels = group_command_labels(*group);
            let key_chars = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c"];

            let half = labels.len().div_ceil(2);
            let left_items: Vec<(String, String)> = labels[..half]
                .iter()
                .enumerate()
                .map(|(i, lbl)| {
                    let k = key_chars.get(i).copied().unwrap_or("?").to_string();
                    (k, lbl.to_string())
                })
                .collect();
            let mut right_items: Vec<(String, String)> = labels[half..]
                .iter()
                .enumerate()
                .map(|(i, lbl)| {
                    let k = key_chars.get(half + i).copied().unwrap_or("?").to_string();
                    (k, lbl.to_string())
                })
                .collect();
            right_items.push(("Esc".to_string(), "Back".to_string()));

            // Split inner area: content rows above, prompt line at bottom.
            let sections = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(inner);
            let content_area = sections[0];
            let prompt_area = sections[1];

            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(content_area);

            f.render_widget(
                Paragraph::new(menu_column(&left_items, menu_key_style(), Style::default())),
                cols[0],
            );
            f.render_widget(
                Paragraph::new(menu_column(
                    &right_items,
                    menu_key_style(),
                    Style::default(),
                )),
                cols[1],
            );
            f.render_widget(Paragraph::new(">"), prompt_area);
        }

        // For input/selection states, use the original 3-line layout.
        _ => {
            let lines = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // Line 1: hints / prompt
                    Constraint::Length(1), // Line 2: error / blank
                    Constraint::Min(1),    // Line 3: input / cursor
                ])
                .split(inner);

            match state {
                ControlState::TextInput {
                    prompt,
                    buffer,
                    error,
                    ..
                } => {
                    f.render_widget(Paragraph::new(prompt.as_str()), lines[0]);
                    if let Some(err) = error {
                        let err_line = Line::from(vec![Span::styled(
                            format!("⚠ {}", err),
                            Style::default().fg(Color::Red),
                        )]);
                        f.render_widget(Paragraph::new(err_line), lines[1]);
                    }
                    let input_line = Line::from(vec![
                        Span::raw("> "),
                        Span::raw(buffer.as_str()),
                        Span::styled("_", Style::default().fg(Color::Yellow)),
                    ]);
                    f.render_widget(Paragraph::new(input_line), lines[2]);
                }

                ControlState::ListSelect {
                    options, cursor, ..
                } => {
                    let hint = Line::from("← → to select, Enter to confirm, Esc to cancel");
                    f.render_widget(Paragraph::new(hint), lines[0]);

                    let mut option_spans: Vec<Span> = vec![Span::raw("> ")];
                    for (i, opt) in options.iter().enumerate() {
                        if i == *cursor {
                            option_spans.push(Span::styled(
                                format!("[{}]", opt),
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ));
                        } else {
                            option_spans.push(Span::raw(format!(" {} ", opt)));
                        }
                        if i + 1 < options.len() {
                            option_spans.push(Span::raw("  "));
                        }
                    }
                    f.render_widget(Paragraph::new(Line::from(option_spans)), lines[2]);
                }

                ControlState::Feedback { message, is_error } => {
                    let msg_style = if *is_error {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default().fg(Color::Green)
                    };
                    f.render_widget(
                        Paragraph::new(Line::from(Span::styled(message.as_str(), msg_style))),
                        lines[1],
                    );
                    f.render_widget(Paragraph::new("Press any key to continue"), lines[2]);
                }

                // Menu, GroupMenu, and Diagnostic are handled above.
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// draw_diag_warning_panel — pre-diagnostic TX safety gate
// ---------------------------------------------------------------------------

/// Draw the hard-to-miss warning shown before a diagnostic run starts.
///
/// The diagnostic run genuinely keys the transmitter (PTT, and CW if a
/// callsign is supplied). Transmitting into an open or mismatched load can
/// damage the transceiver's final amplifier stage, so this screen requires
/// an explicit acknowledgment before anything is sent to the radio.
pub fn draw_diag_warning_panel(f: &mut Frame, area: Rect) {
    let outer_block = Block::default()
        .title(" \u{26a0} DIAGNOSTICS \u{2014} TRANSMIT WARNING \u{26a0} ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD));
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "This diagnostic run will KEY THE TRANSMITTER.",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("It briefly transmits PTT, and sends a real CW test"),
        Line::from("message if you supply a callsign on the next screen."),
        Line::from(""),
        Line::from(Span::styled(
            "The radio MUST be connected to a proper antenna or dummy load.",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("Transmitting into an open or mismatched load can damage"),
        Line::from("the transceiver's final amplifier stage."),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "[Enter/Y]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" I have a load connected, proceed   "),
            Span::styled(
                "[Esc]",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" cancel"),
        ]),
    ];

    f.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false }),
        inner,
    );
}

// ---------------------------------------------------------------------------
// draw_diag_panel — diagnostic results panel
// ---------------------------------------------------------------------------

/// Draw the diagnostic results panel (replaces control panel during diag mode).
///
/// - `Idle`: prompt to press [D]
/// - `Running`: live progress — "Now testing: <label> [round N/3]" + scrolling results
/// - `Done`: summary — one line per command, OK (green) or FAILED (red) with details
pub fn draw_diag_panel(f: &mut Frame, area: Rect, diag: &DiagState) {
    let outer_block = Block::default()
        .title(" Diagnostics ")
        .borders(Borders::ALL);
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    match diag {
        DiagState::Idle => {
            let hint = Paragraph::new(Line::from(vec![
                Span::styled("Press ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    "[D]",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to run diagnostics", Style::default().fg(Color::DarkGray)),
            ]));
            f.render_widget(hint, inner);
        }

        DiagState::Running {
            current_label,
            current_round,
            results,
        } => {
            let total_commands = DIAG_STEP_COUNT;
            let total_steps = total_commands * crate::diag::DIAG_ROUNDS;
            let done = results.len();

            let mut all_lines = build_summary_lines(results);

            // "Running..." header
            all_lines.insert(
                0,
                Line::from(vec![Span::styled(
                    format!(
                        "Running...  ({}/{} commands × {} rounds)",
                        done + 1,
                        total_steps,
                        crate::diag::DIAG_ROUNDS,
                    ),
                    Style::default().fg(Color::Cyan),
                )]),
            );

            // "Now testing:" line
            all_lines.insert(
                1,
                Line::from(vec![
                    Span::styled("Now testing: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{:<44}", current_label),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("[round {}/{}]", current_round, crate::diag::DIAG_ROUNDS),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
            );

            // blank separator
            all_lines.insert(2, Line::from(""));

            // [Esc] abort hint
            all_lines.push(Line::from(vec![
                Span::styled("[Esc]", Style::default().fg(Color::DarkGray)),
                Span::styled(" abort", Style::default().fg(Color::DarkGray)),
            ]));

            let height = inner.height as usize;
            let start = all_lines.len().saturating_sub(height);
            let visible: Vec<Line> = all_lines.into_iter().skip(start).collect();
            f.render_widget(Paragraph::new(visible), inner);
        }

        DiagState::Done { results, scroll } => {
            let summary_lines = build_summary_lines(results);

            // Classify each unique label as skipped, passed, or failed.
            let unique_labels: std::collections::BTreeSet<&str> =
                results.iter().map(|r| r.label).collect();
            let total_labels = unique_labels.len();
            let skipped_labels = unique_labels
                .iter()
                .filter(|&&lbl| results.iter().filter(|r| r.label == lbl).all(|r| r.skipped))
                .count();
            let passed_labels = unique_labels
                .iter()
                .filter(|&&lbl| {
                    results
                        .iter()
                        .filter(|r| r.label == lbl)
                        .all(|r| r.passed && !r.skipped)
                })
                .count();
            let failed_labels = total_labels - passed_labels - skipped_labels;

            let summary_style = if failed_labels == 0 {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            };

            let mut lines = vec![
                Line::from(Span::styled(
                    format!(
                        "Complete: {}/{} passed, {} skipped, {} failed",
                        passed_labels, total_labels, skipped_labels, failed_labels,
                    ),
                    summary_style,
                )),
                Line::from(""),
            ];
            lines.extend(summary_lines);
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("[↑/↓]", Style::default().fg(Color::DarkGray)),
                Span::styled(" scroll  ", Style::default().fg(Color::DarkGray)),
                Span::styled("[PgUp/PgDn]", Style::default().fg(Color::DarkGray)),
                Span::styled("  ", Style::default()),
                Span::styled("[Esc]", Style::default().fg(Color::DarkGray)),
                Span::styled(" menu", Style::default().fg(Color::DarkGray)),
            ]));

            let height = inner.height as usize;
            let max_scroll = lines.len().saturating_sub(height);
            let start = (*scroll).min(max_scroll);
            let visible: Vec<Line> = lines.into_iter().skip(start).collect();
            f.render_widget(Paragraph::new(visible), inner);
        }
    }
}

/// Convert a list of `DiagResult`s into summary `Line`s.
///
/// One line per unique command label: `OK` (green) if all rounds passed,
/// `FAILED` (red) with indented per-round detail lines if any round failed.
/// Label is padded to 32 chars, then `"...OK"` or `"...FAILED"`.
fn build_summary_lines(results: &[DiagResult]) -> Vec<Line<'static>> {
    // Collect unique labels in order of first appearance.
    let mut seen: Vec<&str> = Vec::new();
    for r in results {
        if !seen.contains(&r.label) {
            seen.push(r.label);
        }
    }

    let mut lines: Vec<Line<'static>> = Vec::new();

    for label in seen {
        let rounds: Vec<&DiagResult> = results.iter().filter(|r| r.label == label).collect();
        let all_skipped = rounds.iter().all(|r| r.skipped);
        let all_passed = rounds.iter().all(|r| r.passed && !r.skipped);

        if all_skipped {
            let text = format!("{:<32}...SKIPPED", label);
            lines.push(Line::from(Span::styled(
                text,
                Style::default().fg(Color::Yellow),
            )));
            // Show the reason once (identical across rounds).
            if let Some(r) = rounds.first() {
                lines.push(Line::from(Span::styled(
                    format!("  {}", r.detail),
                    Style::default().fg(Color::Yellow),
                )));
            }
        } else if all_passed {
            let text = format!("{:<32}...OK", label);
            lines.push(Line::from(Span::styled(
                text,
                Style::default().fg(Color::Green),
            )));
        } else {
            let text = format!("{:<32}...FAILED", label);
            lines.push(Line::from(Span::styled(
                text,
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
            // Indented detail lines for failing rounds
            for r in rounds.iter().filter(|r| !r.passed) {
                let detail = format!("  round {}: {}", r.round, r.detail);
                lines.push(Line::from(Span::styled(
                    detail,
                    Style::default().fg(Color::Red),
                )));
            }
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_hz_14mhz() {
        assert_eq!(format_hz(14_000_000), "14.000.000 MHz");
    }

    #[test]
    fn test_format_hz_7250khz() {
        assert_eq!(format_hz(7_250_000), "7.250.000 MHz");
    }

    /// The table this console shipped with, before any of it moved into a
    /// shared crate. Written out in full rather than referenced, so that a
    /// change to `SUnitScale::TS570D` upstream shows up here as a failure
    /// rather than as agreement.
    fn as_shipped(smeter: u16) -> &'static str {
        match smeter {
            0..=2 => "S0",
            3..=4 => "S1",
            5..=6 => "S2",
            7..=8 => "S3",
            9..=10 => "S4",
            11..=12 => "S5",
            13..=14 => "S6",
            15..=16 => "S7",
            17..=18 => "S8",
            19..=20 => "S9",
            21..=24 => "S9+10",
            25..=28 => "S9+20",
            _ => "S9+30",
        }
    }

    #[test]
    fn every_value_the_meter_can_report_still_reads_the_way_it_always_has() {
        // The acceptance bar for moving onto shared widgets (radio-cat-rs
        // ADR 0011 rev 4) is that the operator sees no change. For the
        // S-unit readout that is checkable exhaustively, so it is: the
        // meter reports 0-30 and this walks all 31.
        //
        // It also exercises the whole migrated path rather than a formula
        // -- capabilities to `MeterReading` to label -- so it fails if the
        // radio stops publishing its table, not only if the table changes.
        for raw in 0..=30u16 {
            let state = RadioDisplay {
                smeter: raw,
                ..Default::default()
            };
            let reading = smeter_reading(&state).expect("this radio has an S meter");
            assert_eq!(
                reading.s_unit(),
                as_shipped(raw),
                "raw {raw} changed meaning"
            );
        }
    }

    #[test]
    fn the_reading_carries_this_radios_range_and_not_some_other_ones() {
        // 15 is mid-scale here and under 6% on an FT-991A. Getting the
        // range from capabilities rather than a literal is what keeps the
        // bar honest.
        let state = RadioDisplay {
            smeter: 15,
            ..Default::default()
        };
        let reading = smeter_reading(&state).unwrap();
        assert_eq!(reading.range.max, 30);
        assert_eq!(reading.fraction(), 0.5);
    }

    #[test]
    fn test_radio_display_default() {
        let d = RadioDisplay::default();
        assert_eq!(d.vfo_a_hz, 14_000_000);
    }
}
