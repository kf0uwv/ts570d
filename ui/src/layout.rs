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

use crate::control::{group_command_labels, ControlState, WarnedAction};
use crate::diag::{DiagResult, DiagState};
use crate::terminal::DIAG_STEP_COUNT;

// Shared console logic and terminal widgets (radio-cat-rs ADR 0011 rev 4).
// What stays here is this radio's LAYOUT and FEATURE SET; what comes from
// these crates is anything with one correct answer per input.
use cat_ui_ratatui::{link_panel, menu_column, LinkState};

// ---------------------------------------------------------------------------
// Top-level layout splitter
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// draw_header
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// draw_errors — poll error panel
// ---------------------------------------------------------------------------

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
///
/// `ptt_line_available` decides whether the `[P]` PTT-line item appears in
/// the menu: it is a property of the port, not of the radio, and a console
/// talking to a remote server over TCP has no line to drive.
pub fn draw_control_panel(
    f: &mut Frame,
    area: Rect,
    state: &ControlState,
    ptt_line_available: bool,
) {
    if let ControlState::Diagnostic(diag_state) = state {
        draw_diag_panel(f, area, diag_state);
        return;
    }
    if let ControlState::DiagWarning(what) = state {
        draw_diag_warning_panel(f, area, *what);
        return;
    }
    if let ControlState::PttLine {
        line,
        asserted,
        cts,
        dsr,
        error,
    } = state
    {
        draw_ptt_line_panel(f, area, *line, *asserted, *cts, *dsr, error.as_deref());
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
            let mut right: Vec<(&str, &str)> =
                vec![("C", "CW"), ("O", "Tones"), ("S", "System"), ("D", "Diag")];
            if ptt_line_available {
                right.push(("P", "PTT line"));
            }
            right.push(("Q", "Quit"));
            let right: &[(&str, &str)] = &right;

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

/// Draw the hard-to-miss warning shown before anything that keys the
/// transmitter.
///
/// Both things behind this gate genuinely key the transmitter: the
/// diagnostic run (PTT, and CW if a callsign is supplied) does it over CAT,
/// and the PTT-line screen does it by asserting a handshake line on the
/// port. Transmitting into an open or mismatched load can damage the
/// transceiver's final amplifier stage, so this screen requires an explicit
/// acknowledgment before either one starts. See `docs/adr/0007` and
/// `docs/adr/0010`.
pub fn draw_diag_warning_panel(f: &mut Frame, area: Rect, what: WarnedAction) {
    let title = match what {
        WarnedAction::Diagnostics => " \u{26a0} DIAGNOSTICS \u{2014} TRANSMIT WARNING \u{26a0} ",
        WarnedAction::PttLine => " \u{26a0} PTT LINE \u{2014} TRANSMIT WARNING \u{26a0} ",
    };
    let outer_block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD));
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    let headline = match what {
        WarnedAction::Diagnostics => "This diagnostic run will KEY THE TRANSMITTER.",
        WarnedAction::PttLine => "The next screen will KEY THE TRANSMITTER.",
    };
    let detail: Vec<Line> = match what {
        WarnedAction::Diagnostics => vec![
            Line::from("It briefly transmits PTT, and sends a real CW test"),
            Line::from("message if you supply a callsign on the next screen."),
        ],
        WarnedAction::PttLine => vec![
            Line::from("Asserting the line an interface is wired to holds the"),
            Line::from("radio in transmit for as long as you leave it up."),
        ],
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            headline,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    lines.extend(detail);
    lines.extend(vec![
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
    ]);

    f.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false }),
        inner,
    );
}

// ---------------------------------------------------------------------------
// draw_ptt_line_panel -- hand control of the port's PTT handshake line
// ---------------------------------------------------------------------------

/// Draw the PTT-line screen.
///
/// Shows which line is selected, whether it is up, and the handshake inputs
/// the port reports back. CTS is the useful one on this radio: it follows
/// the radio's COM port being alive, so an operator who sees the line go up
/// with CTS down knows the radio, not the cable, is what is missing.
#[allow(clippy::too_many_arguments)]
pub fn draw_ptt_line_panel(
    f: &mut Frame,
    area: Rect,
    line: radio::PttLineKind,
    asserted: bool,
    cts: bool,
    dsr: bool,
    error: Option<&str>,
) {
    let outer_block = Block::default()
        .title(" PTT line ")
        .borders(Borders::ALL)
        .border_style(if asserted {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        });
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    let (state_text, state_style) = if asserted {
        (
            format!("{} ASSERTED \u{2014} TRANSMITTING", line.name()),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        (
            format!("{} deasserted", line.name()),
            Style::default().fg(Color::Green),
        )
    };

    fn flag(name: &str, up: bool) -> Span<'static> {
        Span::styled(
            format!("{name}:{} ", if up { "up" } else { "--" }),
            Style::default().fg(if up { Color::Green } else { Color::DarkGray }),
        )
    }

    let mut lines = vec![
        Line::from(Span::styled(state_text, state_style)),
        Line::from(vec![
            Span::styled("handshake  ", Style::default().fg(Color::DarkGray)),
            flag("CTS", cts),
            flag("DSR", dsr),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("[Space/Enter]", menu_key_style()),
            Span::raw(if asserted { " unkey   " } else { " key   " }),
            Span::styled("[D]", menu_key_style()),
            Span::raw(" DTR   "),
            Span::styled("[R]", menu_key_style()),
            Span::raw(" RTS   "),
            Span::styled("[Esc]", menu_key_style()),
            Span::raw(" back (releases the line)"),
        ]),
    ];
    if let Some(error) = error {
        lines.push(Line::from(Span::styled(
            error.to_string(),
            Style::default().fg(Color::Red),
        )));
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
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
    use crate::RadioDisplay;

    #[test]
    fn test_radio_display_default() {
        let d = RadioDisplay::default();
        assert_eq!(d.vfo_a_hz, 14_000_000);
    }

    // -----------------------------------------------------------------
    // PTT line
    // -----------------------------------------------------------------

    /// Render one panel into a throwaway terminal and return its text.
    fn rendered(state: &ControlState, ptt_line_available: bool) -> String {
        use ratatui::{backend::TestBackend, Terminal};
        let mut terminal = Terminal::new(TestBackend::new(72, 12)).unwrap();
        terminal
            .draw(|f| draw_control_panel(f, f.size(), state, ptt_line_available))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    #[test]
    fn the_menu_offers_the_ptt_line_only_when_the_port_has_one() {
        assert!(!rendered(&ControlState::Menu, false).contains("PTT line"));
        assert!(rendered(&ControlState::Menu, true).contains("PTT line"));
    }

    #[test]
    fn the_warning_names_which_thing_is_about_to_key_the_transmitter() {
        // One gate, two things behind it -- an operator has to be able to
        // tell which one they just agreed to.
        let diag = rendered(&ControlState::DiagWarning(WarnedAction::Diagnostics), true);
        assert!(diag.contains("DIAGNOSTICS"));
        let ptt = rendered(&ControlState::DiagWarning(WarnedAction::PttLine), true);
        assert!(ptt.contains("PTT LINE"));
        // Both must still say the words that matter.
        for screen in [&diag, &ptt] {
            assert!(screen.contains("KEY THE TRANSMITTER"));
            assert!(screen.contains("antenna or dummy load"));
        }
    }

    #[test]
    fn the_line_screen_says_plainly_when_the_radio_is_transmitting() {
        let up = rendered(
            &ControlState::PttLine {
                line: radio::PttLineKind::Dtr,
                asserted: true,
                cts: true,
                dsr: false,
                error: None,
            },
            true,
        );
        assert!(up.contains("DTR ASSERTED"));
        assert!(up.contains("TRANSMITTING"));
        assert!(up.contains("CTS:up"));

        let down = rendered(
            &ControlState::PttLine {
                line: radio::PttLineKind::Rts,
                asserted: false,
                cts: false,
                dsr: false,
                error: Some("Radio session busy".to_string()),
            },
            true,
        );
        assert!(down.contains("RTS deasserted"));
        assert!(!down.contains("TRANSMITTING"));
        assert!(down.contains("Radio session busy"));
    }
}
