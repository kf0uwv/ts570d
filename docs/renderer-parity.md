# Renderer parity

Required by radio-cat-rs ADR 0013. Parity is on **capabilities**, not on
pixels: every capability this console offers should be reachable in both
renderers, and each place it is not gets a row here naming the ground.

The three grounds ADR 0013 allows are:

- **(a) fidelity** — the renderers can't represent the thing equally well
- **(b) gesture** — the interaction has no sensible counterpart
- **(c) in progress** — not built yet, with a tracking item

Development cost is explicitly **not** a ground.

A capability both renderers have, invoked differently because the media
differ, is **not** an exception and needs no ground — it belongs under
"Reached by different gestures" below. Listing one in the table above
would claim a console cannot do something it can.

## Capability parity: TUI vs GUI

The GUI now exists (`gui/`, `ts570d-gui`), so this is a real table.

It ran, for a while, in the **other** direction from what was expected: the
first GUI slice brought the accepted design's structure — capability-derived
workspaces, a persistent status strip, a command line — and the TUI did not
have any of it. **That is closed as of 2026-09-01.** The TUI was rebuilt to
option 3 (then `ui/src/console.rs`, since lifted into the shared
`cat-ui-ratatui` crate), and the three functions that derive the
structure from capabilities moved out of `gui/` into `cat-ui`
(`workspace::tabs`, `quick::controls`, `command::parse`), so both renderers
now call the same ones rather than each deriving their own answer.

What the design did not place — this radio's own feature menus, and the
TX-gated `[D]` and `[P]` screens — overlays the tab body rather than being
dropped or forced into a fifth tab the design rejected.

| capability | missing from | ground | tracking |
|---|---|---|---|
| ~~Waterfall / spectrum~~ | — | **closed** | `ts570d --if-out <endpoint>` attaches an `rtl_tcp` source and the TUI draws the coarse trace and waterfall ADR 0008 asks for. With no source the panel says `NO SPECTRUM SOURCE` rather than drawing nothing — an absence would read as a broken panel. |
| ~~AF scope / AF FFT~~ | — | **closed** | Both renderers draw both panels, from a local source (`--acc2-audio`) or a remote one (the native protocol carries audio — radio-cat-rs ADR 0019). The arithmetic is shared: `cat_ui::af` owns the fixed 3 kHz axis, the 40 dB window below the peak, the passband derivation from published capabilities, and the resolution cap, so the two consoles cannot show the same radio with different bar heights. Only the drawing differs — braille at 2×4 dots per cell against a pixel polyline — which is ADR 0013's fidelity ground, and neither shows a different waveform. |
| NOTCH value | GUI | (c) | Narrowed from "FILTER / IF SHIFT / NOTCH": `RadioState` carries `if_shift_hz` and `filter_width_hz`, and the GUI's ribbon shows both confirmed. Notch is the one with no field on the wire, so the GUI's cell stays `—` while the TUI's — which talks CAT and reads `Ts570dState` — can fill it. One field, not three. |
| ~~Capability-derived workspaces (tabs)~~ | — | **closed** | Both renderers call `cat_ui::workspace::tabs`. Digits select tabs in the TUI; `tab_for_digit` is shared too, so `1` is whatever is actually first on both. |
| ~~`:` command line~~ | — | **closed** | Both call `cat_ui::command::parse`. A bare digit is the tab verb, so `:2` and the `2` key are one action. |
| ~~Quick-settings bar~~ | — | **closed** | The TUI has the control layer: BAND, MODE, and the eight-cell ribbon. Three of its cells — FILTER, SHIFT, NOTCH — are values the **GUI** cannot fill, because the native protocol has no field for them (finding B2); the TUI talks CAT and `Ts570dState` carries all three. So this row has quietly inverted, and the new gap is in the table below. |
| ~~Device picker (choose a source from a list)~~ | — | **closed** | Both renderers ask the *server* what the radio's host can see (radio-cat-rs ADR 0018) and attach through it. The TUI's `--server` now speaks the console protocol rather than raw CAT, so it gets capabilities, the waterfall and the device list on the same connection; `r` on the SOURCE tab re-asks, matching the GUI's refresh button. Raw CAT client mode is still reachable as `--server-raw`, and correctly reports there that it cannot ask. |
| ~~Attaching remote **audio**~~ | — | **closed** | The protocol carries audio frames as of radio-cat-rs ADR 0019: `ts570d server --acc2-audio` publishes the ACC2 pair, and a console can attach one of the server's sound cards from the picker. The TUI draws both AF panels from it. The GUI's AF panels are still unbuilt — that is the row below, and it is now the only thing standing between the two renderers here. |
| ~~Attached-source view (SOURCE tab)~~ | — | **closed** | The TUI's tab lists both sources and distinguishes *attached*, *configured but not streaming*, and *nothing wired* — the three-state rule the design insists on. The GUI waited on installation state, and that has arrived: `RadioHost::installation` publishes what a bench actually has, `ts570d server` assembles it from what it opened, and the bridge asks per connection so an attach mid-session reaches the next console. |
| `[P]` PTT line (drive DTR/RTS, watch CTS/DSR) | GUI | **(a)** | The GUI is a **network client** (ADR 0008 §3): it never holds a serial handle, and there is no line at the other end of a socket to drive. `radio::PttLine` reports the capability absent for exactly this reason, so the GUI is not missing a control it could have — it is correctly not offering one it has no hardware for. It becomes a parity question only if the native protocol grows a keying command, at which point the ground moves to (c). See `docs/adr/0010`. |

### Neither renderer has these, and it is not their fault

| capability | ground | why |
|---|---|---|
| ~~Frequency, mode, split, meter **readout**~~ | **closed** | The protocol grew its read side: `Command::ReadState` answers with a whole `RadioState` in one round trip, and `ReadMeter` with an actual sample. Both renderers draw from it — the GUI as a native client throughout, the TUI whenever `--server` is used. Unknown values still render as `—` rather than zero, which remains the honest rendering between connecting and the first state arriving. |
| ~~Live spectrum from the CN4 tap~~ | **closed** | Both renderers have it. `ts570d server --if-out` reads the tap and pumps `FrameKind::Spectrum`; the GUI draws it as a waterfall and the TUI does too, over `--server` or from a source of its own. The device layer is still behind a default-off feature (radio-cat-rs ADR 0014 §5), which is a build-time choice rather than a parity gap: a build without it says what to do instead. |

### Reached by different gestures, which is not a gap

Both renderers have the capability. Only the way an operator invokes it
differs, because the media differ. These are **not** ADR 0013 exceptions
and need no ground: nothing is missing from either console.

The distinction matters because the table above is a list of holes, and
putting a gesture in it says the wrong thing — that the TUI cannot do
something, when it can, by a means that suits a keyboard better than a
mouse would.

| capability | GUI | TUI |
|---|---|---|
| Retune from the spectrum | **click-to-tune on the waterfall** — point at a signal, snap to the radio's finest step. A GUI-only affordance: it is what a pointing device is *for*, and a terminal has no pixel-accurate equivalent over a 20-column bar. | `:t <freq>` — the same `Command::Retune`, typed. Exact rather than aimed, which is the better gesture when you already know the frequency. |

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
