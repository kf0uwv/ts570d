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

**The GUI does not exist yet.** Every capability below is therefore
TUI-only under ground (c), and this table collapses to a single row rather
than one per feature — enumerating forty rows that all say "the GUI has not
been written" would be noise, not a record. It becomes a real table the
moment the first GUI panel ships, at which point anything still missing
needs its own row.

| capability | missing from | ground | tracking |
|---|---|---|---|
| all of them | GUI | (c) | the GUI console, not yet started |

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
