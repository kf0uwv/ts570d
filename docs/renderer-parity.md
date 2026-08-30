# Renderer parity

Required by radio-cat-rs ADR 0013. Parity is on **capabilities**, not on
pixels: every capability this console offers should be reachable in both
renderers, and each place it is not gets a row here naming the ground.

The three grounds ADR 0013 allows are:

- **(a) fidelity** — the renderers can't represent the thing equally well
- **(b) gesture** — the interaction has no sensible counterpart
- **(c) in progress** — not built yet, with a tracking item

Development cost is explicitly **not** a ground.

## Capability parity: TUI vs GUI

The GUI now exists (`gui/`, `ts570d-gui`), so this is a real table.

It runs mostly in the **other** direction from what was expected: the
first GUI slice brought the accepted design's structure — capability-derived
workspaces, a persistent status strip, a command line — and the TUI does
not have those yet. ADR 0008's design direction says option 3 is for *both*
renderers, so these are ground (c) and tracked, not permanent.

| capability | missing from | ground | tracking |
|---|---|---|---|
| Waterfall / spectrum | TUI | (c) | ADR 0008 is explicit that the TUI gets a **coarse, low-rate rendering of the same `SpectrumFrame`s, not an absence**. `cat-ui-ratatui::waterfall` already exists; the TUI is not yet wired to a frame source. |
| Capability-derived workspaces (tabs) | TUI | (c) | `gui::workspace::tabs` is renderer-agnostic and takes a `CapabilitiesWire`. The TUI can call the same function. |
| `:` command line | TUI | (c) | `gui::command::parse` is renderer-agnostic for the same reason. |
| Quick-settings bar | TUI | (c) | The TUI shows mode, AF/RF/MIC and AGC already, but as a **readout**, not as controls. `gui::quick::controls` derives the control set from capabilities and the TUI can use it. |
| Click-to-tune on the waterfall | TUI | **(b)** | A terminal has no pixel-accurate pointing gesture over a 20-column bar. The TUI reaches the same capability by `:t <freq>` — the same `Command::Retune`. **This one is permanent.** |
| Attached-source view (SOURCE tab) | TUI | (c) | Waiting on the same thing the GUI is: the protocol does not report installation state yet. |

### Neither renderer has these, and it is not their fault

| capability | ground | why |
|---|---|---|
| Frequency, mode, split, meter **readout** | (c) | **The native protocol has no read side.** `Command::ReadMeter` validates that the meter exists and answers `Ack` without a reading, and no command reports the dial. A console on this protocol can send and cannot see. The GUI draws every unknown value as `—` rather than as zero, which is the honest rendering and is also what has to happen anyway between connecting and the first state arriving. Tracked as the next protocol change. |
| Live spectrum from the CN4 tap | (c) | `cat-signal-rtlsdr`'s device layer is behind a default-off feature (radio-cat-rs ADR 0014 §5), and no server serves the native protocol yet — `ServerConfig` has no port for it. |

## Operator-visible changes from the shared-widget migration

Not ADR 0013 exceptions — both renderers would show these — but radio-cat-rs
ADR 0011 rev 4 sets "the operator sees no change" as the bar for migrating
the TUI onto shared widgets, and these three are where that bar was
knowingly crossed. Recorded here because this is the file a reviewer opens.

| what changed | before | after | why |
|---|---|---|---|
| S-meter bar resolution | whole cells, `(raw × 20) / 30` truncated | eight sub-levels per cell, rounded | the shared bar resolves 160 steps across 20 cells. Strictly finer than the meter reports, so no reading is lost — but the bar moves at raw values where it used to sit still. |
| Error panel ordering | first three errors of the cycle | most recent three | a radio failing in a loop used to pin the panel to its oldest failures and never show the current one. This is a bug fix that happens to be visible. |
| S-meter bar end caps | inside the bar string | drawn by this crate | no visual change; noted because the caps are now layout (ours) and the 20 cells between them are the shared widget. |

Everything else is byte-identical, and the S-unit readout is checked
exhaustively — `every_value_the_meter_can_report_still_reads_the_way_it_always_has`
walks all 31 values the meter can produce against the table this console
shipped with.

## Where the S-unit table lives now

On the radio, not in the console: `radio::capabilities::TS570D` publishes
it, `MeterReading::from_meters` carries it, and the widget is never told a
scale — so it cannot be told the wrong one. Where an S-meter's unit
boundaries fall is a property of the meter circuit; this radio gives S0
three raw counts and every other unit two, and an interpolated scale
disagrees at 8 of the 31 values.
