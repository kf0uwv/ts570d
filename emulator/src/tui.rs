use radio::commands::COMMAND_TABLE;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::radio_state::{RadioState, VfoSel};

/// Look up the human-readable description for a 2-character CAT command code.
fn lookup_description(code: &str) -> Option<&'static str> {
    COMMAND_TABLE
        .iter()
        .find(|cmd| cmd.code == code)
        .map(|cmd| cmd.description)
}

// ─── 5-row block-character digit font ────────────────────────────────────────

fn big_digit(d: char) -> [&'static str; 5] {
    match d {
        '0' => ["▄███▄", "█   █", "█   █", "█   █", "▀███▀"],
        '1' => ["  ▄█ ", "  ██ ", "   █ ", "   █ ", "  ███"],
        '2' => ["▄███▄", "    █", "▄███▀", "█    ", "█████"],
        '3' => ["▄███▄", "    █", " ███▄", "    █", "▀███▀"],
        '4' => ["█   █", "█   █", "▀████", "    █", "    █"],
        '5' => ["█████", "█    ", "▀███▄", "    █", "▀███▀"],
        '6' => ["▄███▄", "█    ", "████▄", "█   █", "▀███▀"],
        '7' => ["█████", "    █", "   █ ", "  █  ", "  █  "],
        '8' => ["▄███▄", "█   █", "▄███▄", "█   █", "▀███▀"],
        '9' => ["▄███▄", "█   █", "▀████", "    █", "▀███▀"],
        '.' => ["     ", "     ", "     ", "  ▄  ", "  █  "],
        _ => ["     ", "     ", "     ", "     ", "     "],
    }
}

/// Render a frequency string as 5 lines of block-character glyphs.
/// Returns an array of 5 strings, one per row.
fn render_big_freq(freq_str: &str) -> [String; 5] {
    // Collect glyph rows for each character.
    let glyphs: Vec<[&'static str; 5]> = freq_str.chars().map(big_digit).collect();

    let mut rows: [String; 5] = Default::default();
    for row in 0..5 {
        let mut line = String::new();
        for (i, glyph) in glyphs.iter().enumerate() {
            if i > 0 {
                line.push(' ');
            }
            line.push_str(glyph[row]);
        }
        rows[row] = line;
    }
    rows
}

/// Format the active frequency in TS-570D display format: `14.280.00`
fn format_freq_ascii(state: &RadioState) -> String {
    let freq_hz = match state.active_vfo {
        VfoSel::B => state.vfo_b_hz,
        _ => state.vfo_a_hz,
    };
    let mhz = freq_hz / 1_000_000;
    let khz = (freq_hz % 1_000_000) / 1_000;
    let ten_hz = (freq_hz % 1_000) / 10;
    format!("{:>2}.{:03}.{:02}", mhz, khz, ten_hz)
}

/// Bright amber / yellow style used for ON annunciators.
fn on_style() -> Style {
    Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD)
}

/// Build the bargraph string from a 0.0–1.0 fill ratio and a total character width.
pub fn bargraph(ratio: f64, width: usize) -> String {
    const BLOCKS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if width == 0 {
        return String::new();
    }
    let total_eighths = (ratio.clamp(0.0, 1.0) * (width * 8) as f64).round() as usize;
    let full_blocks = total_eighths / 8;
    let remainder = total_eighths % 8;
    let mut s = String::with_capacity(width);
    for _ in 0..full_blocks.min(width) {
        s.push('█');
    }
    if full_blocks < width && remainder > 0 {
        s.push(BLOCKS[remainder - 1]);
        for _ in (full_blocks + 1)..width {
            s.push(' ');
        }
    } else {
        for _ in full_blocks..width {
            s.push(' ');
        }
    }
    s
}

// ─────────────────────────────────────────────────────────────────────────────
//  Public entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Draw the full three-column layout into `f`.
///
/// - Col 1 (~22 chars):  Meter column — S-meter (RX) or TX meters (TX)
/// - Col 2 (remaining):  Main LCD — annunciators, frequency, modes
/// - Col 3 (~45% total): Command/status panel — port, command log, controls
pub fn draw(f: &mut Frame, state: &RadioState, port: &str, log: &[String]) {
    let area = f.size();

    // Outer border titled "KENWOOD TS-570D" in amber.
    let outer_block = Block::default()
        .borders(Borders::ALL)
        .title(" KENWOOD TS-570D ")
        .border_style(Style::default().fg(Color::Yellow));
    let inner = outer_block.inner(area);
    f.render_widget(outer_block, area);

    // ── Three-column split ──────────────────────────────────────────────────
    let total_w = inner.width;
    let meter_w: u16 = 22;
    let remaining = total_w.saturating_sub(meter_w);
    let cmd_w: u16 = ((total_w as u32 * 45 / 100) as u16).min(remaining);
    let lcd_w: u16 = remaining.saturating_sub(cmd_w);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(meter_w),
            Constraint::Length(lcd_w),
            Constraint::Length(cmd_w),
        ])
        .split(inner);

    let meter_area = cols[0];
    let lcd_area = cols[1];
    let cmd_area = cols[2];

    draw_meter_col(f, meter_area, state);
    draw_lcd_main(f, lcd_area, state);
    draw_command_panel(f, cmd_area, port, log);
}

// ─────────────────────────────────────────────────────────────────────────────
//  COL 1 — Meter column
// ─────────────────────────────────────────────────────────────────────────────

fn draw_meter_col(f: &mut Frame, area: Rect, state: &RadioState) {
    if state.tx {
        draw_tx_meters(f, area, state);
    } else {
        draw_rx_smeter(f, area, state);
    }
}

fn draw_rx_smeter(f: &mut Frame, area: Rect, state: &RadioState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // scale label
            Constraint::Length(1), // bargraph
            Constraint::Min(0),    // padding
        ])
        .split(area);

    // 22-char scale: S1 at col 2, S3~4, S5~6, S7~8, S9~10, +20~13, +40~17, +60~21
    let scale = Paragraph::new(Span::styled(
        "S1 3 5 7 9  20  40 60",
        Style::default().fg(Color::DarkGray),
    ));

    // smeter: 0=S0, 9=S9, 30=S9+60dB
    let ratio = state.smeter as f64 / 30.0;
    let bar_w = area.width as usize;
    let bar_str = bargraph(ratio, bar_w.max(1));
    let bar_line = Line::from(Span::styled(bar_str, Style::default().fg(Color::Yellow)));

    if rows[0].height > 0 {
        f.render_widget(scale, rows[0]);
    }
    if rows[1].height > 0 {
        f.render_widget(Paragraph::new(bar_line), rows[1]);
    }
}

fn draw_tx_meters(f: &mut Frame, area: Rect, state: &RadioState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // PWR scale
            Constraint::Length(1), // PWR bar
            Constraint::Length(1), // SWR scale
            Constraint::Length(1), // SWR bar
            Constraint::Length(1), // COMP scale
            Constraint::Length(1), // COMP bar
            Constraint::Length(1), // ALC scale
            Constraint::Length(1), // ALC bar
            Constraint::Min(0),    // padding
        ])
        .split(area);

    let bar_w = area.width as usize;

    // PWR: power_control 0–100 → ratio 0.0–1.0
    let pwr_ratio = state.power_control as f64 / 100.0;
    // SWR: emulator always returns good SWR (1.0 = perfect); show fixed low value
    let swr_ratio = 0.05_f64;
    // COMP: speech processor off by default → 0.0
    let comp_ratio = 0.0_f64;
    // ALC: 0.0 when not transmitting hard
    let alc_ratio = 0.0_f64;

    let render_pair = |f: &mut Frame, scale_row: Rect, bar_row: Rect, label: &str, ratio: f64| {
        let scale = Paragraph::new(Span::styled(label, Style::default().fg(Color::DarkGray)));
        let bar_str = bargraph(ratio, bar_w.max(1));
        let bar_line = Line::from(Span::styled(bar_str, Style::default().fg(Color::Yellow)));
        if scale_row.height > 0 {
            f.render_widget(scale, scale_row);
        }
        if bar_row.height > 0 {
            f.render_widget(Paragraph::new(bar_line), bar_row);
        }
    };

    // Scale strings match real TS-570D label + tick positions (~22 chars wide)
    render_pair(f, rows[0], rows[1], "PWR 10  25  50  100%", pwr_ratio);
    render_pair(f, rows[2], rows[3], "SWR 1  1.5  2   3", swr_ratio);
    render_pair(f, rows[4], rows[5], "COMP 5  10  15  20dB", comp_ratio);
    render_pair(f, rows[6], rows[7], "ALC", alc_ratio);
}

// ─────────────────────────────────────────────────────────────────────────────
//  COL 2 — Main LCD: annunciators + freq + modes
// ─────────────────────────────────────────────────────────────────────────────

fn draw_lcd_main(f: &mut Frame, area: Rect, state: &RadioState) {
    // Layout: optional ann row 1, optional ann row 2, 5-row freq block, mode row.
    // We always allocate 1 row for each ann line (may be blank) and 1 for mode row.
    // The freq block takes 5 rows.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Ann line 1 (active items only, may be empty)
            Constraint::Length(1), // Ann line 2 (active items only, may be empty)
            Constraint::Length(5), // Large frequency display (5-row block glyphs)
            Constraint::Length(1), // Mode + bottom annunciators
            Constraint::Min(0),    // remaining padding
        ])
        .split(area);

    draw_ann_line1(f, rows[0], state);
    draw_ann_line2(f, rows[1], state);
    draw_freq_block(f, rows[2], state);
    draw_mode_row(f, rows[3], state);
}

// ─── Active-only annunciator helpers ─────────────────────────────────────────

/// Build a space-joined string of only the active labels from a list.
fn active_ann_str(items: &[(&str, bool)]) -> String {
    items
        .iter()
        .filter_map(|&(label, active)| if active { Some(label) } else { None })
        .collect::<Vec<_>>()
        .join(" ")
}

// ─── Annunciator line 1 ───────────────────────────────────────────────────────
// TX  RX  AT  ANT1  ANT2  ATT  PRE-AMP  VOX  PROC  NB  FAST  SPLIT
// Only ACTIVE indicators are shown; inactive ones are completely hidden.

fn draw_ann_line1(f: &mut Frame, area: Rect, state: &RadioState) {
    // RX: show when not transmitting (squelch_open concept; real radio shows RX when sq open).
    // We show RX when not TX as a reasonable default.
    let items: &[(&str, bool)] = &[
        ("TX", state.tx),
        ("RX", !state.tx),
        ("AT", state.antenna_tuner),
        ("ANT1", state.antenna == 1),
        ("ANT2", state.antenna == 2),
        ("ATT", state.attenuator),
        ("PRE-AMP", state.preamp),
        ("VOX", state.vox),
        ("PROC", state.proc),
        ("NB", state.noise_blanker),
        ("FAST", state.fast_agc),
        ("SPLIT", state.split),
    ];
    let text = active_ann_str(items);
    if !text.is_empty() {
        f.render_widget(Paragraph::new(Span::styled(text, on_style())), area);
    }
}

// ─── Annunciator line 2 ───────────────────────────────────────────────────────
// RIT  XIT  TX EQ.  N.R.1  N.R.2  BEAT CANCEL
// Only ACTIVE indicators are shown.

fn draw_ann_line2(f: &mut Frame, area: Rect, state: &RadioState) {
    let items: &[(&str, bool)] = &[
        ("RIT", state.rit),
        ("XIT", state.xit),
        ("TX EQ.", state.tx_eq),
        ("N.R.1", state.noise_reduction == 1),
        ("N.R.2", state.noise_reduction == 2),
        ("BEAT CANCEL", state.beat_cancel),
    ];
    let text = active_ann_str(items);
    if !text.is_empty() {
        f.render_widget(Paragraph::new(Span::styled(text, on_style())), area);
    }
}

// ─── 5-row large frequency display ───────────────────────────────────────────

fn draw_freq_block(f: &mut Frame, area: Rect, state: &RadioState) {
    if area.height < 5 {
        return;
    }

    let freq_str = format_freq_ascii(state);
    let rows = render_big_freq(&freq_str);

    // VFO badge — placed on row 2 (index 2) of the 5-row block, to the right of the freq digits.
    let vfo_badge = match state.active_vfo {
        VfoSel::A => "◄A►",
        VfoSel::B => "◄B►",
        VfoSel::Memory => "◄M►",
    };

    // Sub-display: shown only when split, RIT, or XIT is active.
    let sub_str: Option<String> = if state.split {
        // Show the transmit frequency (VFO B) during split operation.
        let tx_hz = state.vfo_b_hz;
        let mhz = tx_hz / 1_000_000;
        let khz = (tx_hz % 1_000_000) / 1_000;
        let ten_hz = (tx_hz % 1_000) / 10;
        Some(format!("{:>2}.{:03}.{:02}", mhz, khz, ten_hz))
    } else if state.rit {
        Some(format!("{:+06}", state.rit_offset))
    } else if state.xit {
        Some(format!("{:+06}", state.xit_offset))
    } else {
        None
    };

    // Split the 5 rows of the area.
    let row_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    for (i, row_text) in rows.iter().enumerate() {
        let row_area = row_areas[i];
        let spans: Vec<Span> = if i == 2 {
            // Middle row: left-pad | freq row | VFO badge | sub-display (if active)
            let mut s = vec![
                Span::raw("    "),
                Span::styled(row_text.clone(), on_style()),
                Span::raw(" "),
                Span::styled(vfo_badge, on_style()),
            ];
            if let Some(ref sub) = sub_str {
                s.push(Span::raw("  "));
                s.push(Span::styled(
                    sub.clone(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            s
        } else {
            // Other rows: left-pad to align with middle row (4 chars matches badge side)
            vec![
                Span::raw("    "),
                Span::styled(row_text.clone(), on_style()),
            ]
        };
        f.render_widget(Paragraph::new(Line::from(spans)), row_area);
    }
}

// ─── Mode row + bottom annunciators ──────────────────────────────────────────
// Only the ACTIVE mode is shown; all other modes are hidden.
// Bottom annunciators: only active ones shown, plus CTRL which is always shown.

fn draw_mode_row(f: &mut Frame, area: Rect, state: &RadioState) {
    // Current mode label.
    let mode_label = match state.mode {
        1 => "LSB",
        2 => "USB",
        3 => "CW",
        4 => "FM",
        5 => "AM",
        6 => "FSK",
        7 => "CW-R",
        9 => "FSK-R",
        _ => "---",
    };

    // Bottom annunciators — active only, plus CTRL always on.
    let bottom_items: &[(&str, bool)] = &[
        ("M.SCR", state.memory_scroll),
        ("F.LOCK", state.freq_lock),
        ("FINE", state.fine_step),
        ("1MHz", state.mhz_step),
        ("T", state.subtone),
        ("CTCSS", state.ctcss),
        ("CTRL", true), // always shown — we are always in CAT control mode
    ];
    let bottom_str = active_ann_str(bottom_items);

    let mut spans = vec![Span::styled(mode_label, on_style())];
    if !bottom_str.is_empty() {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(bottom_str, on_style()));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ─────────────────────────────────────────────────────────────────────────────
//  COL 3 — Command/status panel
// ─────────────────────────────────────────────────────────────────────────────

fn draw_command_panel(f: &mut Frame, area: Rect, port: &str, log: &[String]) {
    let panel_block = Block::default()
        .borders(Borders::ALL)
        .title(" Commands ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = panel_block.inner(area);
    f.render_widget(panel_block, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // PORT line
            Constraint::Min(1),    // scrolling command log
            Constraint::Length(1), // controls
        ])
        .split(inner);

    // PORT line.
    let port_line = Line::from(vec![
        Span::styled("PORT: ", Style::default().fg(Color::Cyan)),
        Span::styled(
            port,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    f.render_widget(Paragraph::new(port_line), sections[0]);

    // COMMANDS section — show last N lines that fit.
    let log_area = sections[1];
    let max_lines = log_area.height as usize;

    let visible: Vec<Line> = if log.len() > max_lines {
        log[log.len() - max_lines..]
            .iter()
            .map(|s| format_log_line(s))
            .collect()
    } else {
        log.iter().map(|s| format_log_line(s)).collect()
    };

    f.render_widget(Paragraph::new(visible).wrap(Wrap { trim: false }), log_area);

    // CONTROLS line.
    let controls_line = Line::from(vec![Span::styled(
        "[q] quit",
        Style::default().fg(Color::DarkGray),
    )]);
    f.render_widget(Paragraph::new(controls_line), sections[2]);
}

/// Extract a 2-character CAT command code from a log entry string.
///
/// Log entries have the form `"→ FA;"` or `"← FA00014000000;"`.
/// The arrow character is a multi-byte UTF-8 sequence, so we skip to the
/// first ASCII alphabetic character after the prefix.
fn extract_command_code(s: &str) -> Option<&str> {
    // Find the first ASCII letter — command codes always start there.
    let start = s
        .char_indices()
        .find(|(_, c)| c.is_ascii_alphabetic())
        .map(|(i, _)| i)?;
    let rest = &s[start..];
    if rest.len() >= 2 && rest.as_bytes()[..2].iter().all(|b| b.is_ascii_alphabetic()) {
        Some(&rest[..2])
    } else {
        None
    }
}

/// Style a single log entry line with an optional gray description comment.
/// Lines starting with "→" are incoming (yellow), "←" are responses (green).
fn format_log_line(s: &str) -> Line<'static> {
    let owned = s.to_owned();
    let code = extract_command_code(&owned).map(str::to_ascii_uppercase);
    let description = code
        .as_deref()
        .and_then(lookup_description)
        .map(|d| format!("  // {d}"));

    if owned.starts_with('→') {
        let mut spans = vec![Span::styled(owned, Style::default().fg(Color::Yellow))];
        if let Some(desc) = description {
            spans.push(Span::styled(desc, Style::default().fg(Color::DarkGray)));
        }
        Line::from(spans)
    } else if owned.starts_with('←') {
        let mut spans = vec![Span::styled(owned, Style::default().fg(Color::Green))];
        if let Some(desc) = description {
            spans.push(Span::styled(desc, Style::default().fg(Color::DarkGray)));
        }
        Line::from(spans)
    } else {
        Line::from(Span::raw(owned))
    }
}
