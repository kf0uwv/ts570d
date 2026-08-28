# 8. A GPU-rendered `gui` crate on egui/wgpu, as a network client, on shared building blocks

Date: 2026-08-27

## Status

**Accepted** (2026-08-27) — user sign-off. Blocked on `radio-cat-rs` ADR 0010
landing first; see Status note below on sequencing. No code has been written yet.

Revision 4 (2026-08-27) after user direction: the TUI is permanent and holds
capability parity with the GUI (`radio-cat-rs` ADR 0013), and `ui` is
migrated onto the shared `cat-ui` / `cat-ui-ratatui` widget set rather than
left unchanged (`radio-cat-rs` ADR 0011 revision 4). The GUI is a **second**
renderer of the same feature set, not the successor to the first.

Revision 3 (2026-08-27) after user review: `gui` builds on `cat-ui` /
`cat-ui-egui` base widgets and owns only this radio's layout and features,
per `radio-cat-rs` ADR 0011. Depends on `radio-cat-rs` ADR 0010 (capability
model, multi-endpoint transports, `SpectrumSource`, native protocol with
rigctl as a compatibility layer), ADR 0011 and ADR 0013.

**Sequencing is settled: the library work lands first.** No crate described
here starts before ADR 0010 is Accepted and implemented for the TS-570D.

## Context

This app ships a ratatui TUI and nothing else. A cross-platform desktop GUI
is wanted, benchmarked against a third-party FT-710 console: a three-rail
dark layout with an analog S-meter, AF oscilloscope, AF FFT and TX meters on
the left; large VFO-A/B readouts over a live spectrum and waterfall in the
centre; and a band/mode/level/DSP control grid on the right.

Four constraints come from this repo as it stands, and one from the user.

1. **`Ts570d<S>` is unconditionally `!Send`.**
   [ADR 0006](0006-windows-concurrency-model.md) records why:
   `SharedSession`'s `Rc<RefCell<Option<S>>>` can never be `Send`, `Radio`
   is `#[async_trait(?Send)]`, and the `Arc<Mutex<_>>`-plus-worker-thread
   alternative was explicitly rejected. Any GUI that owns a radio session
   inherits that constraint whole.
2. **Tokio is banned.** `CLAUDE.md`, Core Technologies: "Tokio should NEVER
   be used in this project."
3. **`ui` depends only on `radio`**, through the `Radio` trait
   (Dependency Rule 4). A second renderer can slot in beside it without
   disturbing the crate graph.
4. **`server` and `ui` are declared "contractually TS-570D-shaped."**
   `radio-cat-rs` ADR 0010 requires rewriting that rule.
5. **The user's stated criterion is speed** — "I want this to be fast.
   Waterfall and charts rendered on the GPU" — and the interaction model is
   **network**, not a locally-owned serial port.

Constraint 5 dissolves constraint 1. A GUI that reaches the radio over the
network never constructs a `Ts570d<S>`; the headless `ts570d server` process
keeps owning the serial port, exactly as it does for WSJT-X today. ADR
0006's `!Send` analysis, its `win_sched` scheduler, and its residual risk
all continue to apply to the TUI and to `server`, and simply do not apply to
the GUI process at all.

The TS-570D also has **no bandscope**. Its spectrum can only come from the
CN4 IF tap (73.05 MHz first IF, inverted, one calibrated Hz trim) via an
external SDR — normalized by `radio-cat-rs` ADR 0010's `SpectrumSource` so
that no code in this repo carries that math.

## Decision

### 1. Add a `gui` crate; keep `ui`

A new workspace member `gui`, built on `eframe`/`egui` with the `wgpu`
backend, built on `radio-cat-rs`'s `cat-ui` and `cat-ui-egui`. Per ADR 0011
there, `gui` owns **only this radio's layout, feature set, menu topology,
keybindings and visual identity**; the waterfall pass, S-meter, knob, meter
rail, VFO readout, band/mode grids and settings-descriptor renderer come
from `cat-ui-egui`. It delegates every wire interaction to the shared
protocol client. The existing `ui` (ratatui) crate is **kept, maintained
and migrated** onto `cat-ui` + `cat-ui-ratatui`, keeping this radio's TUI
layout and feature set exactly as it stands — the acceptance bar for that
migration is that the operator sees no change. It is not "the headless/SSH
path" in the sense of a lesser one: per ADR 0013 it holds capability parity
with `gui`, and any gap in either direction is recorded in
`docs/renderer-parity.md` with its ground.

### 2. The GUI is network-only

`gui` speaks ADR 0010's typed session protocol to `ts570d server`. It does
**not** take a `--port` and does not depend on any transport crate. Running
against a local radio means running the server locally — the same shape
users already run for WSJT-X.

Consequences that follow for free: no `!Send` constraint, no `win_sched`
equivalent, no monoio/Windows executor split in the GUI, and the same binary
works against a radio in the shack from a laptop anywhere.

### 3. Dependency rules for `gui`

Extending `CLAUDE.md`'s existing numbered rules:

- `gui` depends on `cat-ui`, `cat-ui-egui`, the ADR 0010 protocol client,
  and `cat-signal`'s types. It **never** depends on `radio`,
  `cat-framework`, or any `cat-transport-*` crate.
- `gui` may contain TS-570D layout, feature and menu knowledge — that is
  what ADR 0011 leaves here. It must **not** contain wire framing, protocol
  state machines, command validation, IF-tap correction math, or a
  hand-written widget that `cat-ui-egui` already provides.
- **Do not push this radio's preferences up into `cat-ui-egui`.** If a
  proposed shared-widget parameter exists to express what the TS-570D
  wants, the widget belongs here instead. ADR 0011 names this as the seam's
  main failure mode.
- **A capability `gui` exposes, `ui` exposes too** (ADR 0013). A feature
  landing in `gui` with neither a TUI counterpart nor a row in
  `docs/renderer-parity.md` is a review failure, not a follow-up. The
  spectrum is the live case: the TUI gets a coarse, low-rate rendering of
  the same `SpectrumFrame`s, not an absence.
- `gui` renders the spectrum settings panel from `cat-ui-egui`'s generic
  `SettingDescriptor` renderer. It must not hand-write a panel per source
  type, and in particular must not special-case the TS-570D's `trim_hz`.
- Rule 5 stands unchanged: `src/main.rs` remains the only place concrete
  transport types are wired, and it does not wire `gui` at all — `gui` is
  its own binary.

### 4. `server` stops being TS-570D-shaped

`server` publishes `RadioCapabilities` on connect and serves ADR 0010 §6's
native typed protocol on its own port. Rigctl stays a **compatibility
layer** on its own port with unchanged wire behaviour, so WSJT-X does not
regress. `server/src/rigctl_radio.rs` (269 lines) is **deleted**:
`cat-rigctl` is reimplemented once over `RadioCapabilities`, so this radio
gains rigctl support by describing itself rather than by hand-writing a
bridge.

### Framework choice: egui/eframe on wgpu

Decided on the user's stated criterion (GPU-rendered, fast) with the
network model already removing the concurrency objections that would
otherwise have dominated.

| | `!Send` | Waterfall path | Tokio | ai-tools reuse |
|---|---|---|---|---|
| **egui/eframe + wgpu** | N/A (network client) | socket → `wgpu::Texture` → custom pass | none | design agents only |
| Iced | N/A | `shader` widget, wgpu-native | none | design agents only |
| Slint | N/A | pixel buffer → `Image` | none | design agents only |
| Tauri + React | N/A | socket → WS → JS typed array → WebGL | **hard dependency** | full stack |

**Tauri rejected.** It reuses the whole `ai-tools` React/TypeScript agent
stack, which is a real benefit and was the strongest argument for it. But
every spectrum frame would cross a JS heap with GC jitter and no control of
present timing, which is precisely the axis the user named as decisive, and
it reintroduces tokio into the product. The webview path is proven at these
frame rates (WebSDR, KiwiSDR) — it is not *disqualified*, it is second-best
against the stated criterion.

**Iced** is the closest runner-up: also wgpu-native, with a `shader` widget
purpose-built for custom primitives and more structured styling than egui.
Its usual disadvantage here — `Task`/`Command` expecting `Send` futures —
is irrelevant once the GUI is a network client. egui is chosen on ecosystem
size and iteration speed, not on a technical disqualification of Iced. If
egui's hand-rolled styling proves to be the bottleneck in reaching
the reference console's visual quality, Iced is the documented fallback.

**Slint rejected** on two counts: its waterfall path is a pixel buffer
rather than a native GPU pass, and its licensing needs clearing against this
repo's Apache-2.0 before it could be considered at all.

### Explicitly out of scope for this ADR

- **Retiring the TUI.** Explicitly rejected by the user, and now recorded
  as a standing constraint in `radio-cat-rs` ADR 0013: neither renderer is
  retired without a superseding ADR, and both ship in every release.
- **A local-serial GUI mode.** Would reintroduce every ADR 0006 constraint
  for no benefit the network path lacks. *Revisit trigger:* users find
  running a local server unacceptable friction.
- **Audio panels.** The reference console's AF oscilloscope and AF FFT are
  fed by `SignalCapability::AudioDerived`, which needs an audio-stream
  design that does not exist yet (ADR 0010 §Out of scope). The TS-570D has
  no USB codec at all — any audio would come from a soundcard on ACC2 — so
  it will report `AudioDerived` absent for the foreseeable future. The
  layout should reserve their space; the build should not attempt them.
- **The visual design.** A design process runs separately and feeds
  requirements + acceptance criteria into the normal spine.
- **Packaging.** `packaging/` gains a GUI target later; not decided here.

## Consequences

**Good.**

- The GUI escapes ADR 0006's concurrency model entirely rather than
  working around it.
- `gui` expresses the TS-570D faithfully in layout while the expensive
  parts — the wgpu waterfall pass above all — come from `cat-ui-egui` and
  are written once for the whole fleet.
- `server/src/rigctl_radio.rs` is deleted rather than maintained.
- The CN4 IF-tap correction math lives in `cat-signal` and appears nowhere
  in this repo. `if-panadapter-bridge.py` retires as the spike it is.

**Costs and risks.**

- `CLAUDE.md`'s "contractually TS-570D-shaped" rule must be rewritten with
  explicit user sign-off. It is binding on all agents; an architect cannot
  change it unilaterally.
- Blocked on `radio-cat-rs` ADR 0010, which is itself only Proposed and
  carries an open verification gap (whether any current radio has a native
  bandscope). **Sequencing is settled: the library work lands first —
  implement ADR 0010's capability, endpoint, and signal model with only the
  TS-570D + IF-tap backing, then write the GUI against the normalized
  protocol from day one.** Building the GUI against TS-570D wire types and
  normalizing later means writing it twice.
- egui's styling is code-driven and `egui_plot` tessellates on the CPU.
  Only the waterfall gets a custom GPU pass; the AF-rate charts do not need
  one, but reaching the reference console's polish is real effort.
- A GPU toolchain enters this repo's CI. `radio-cat-rs` ADR 0012 makes
  `x86_64-pc-windows-msvc` the single Windows target with `windows-latest`
  CI authoritative, which is what makes GUI verification meaningful at all —
  a `-gnu` type-check of a `wgpu`/DX12 surface would have been a false
  signal. Residual gap: there is still no Windows machine in this
  environment for local iteration, and no CI runner exercises a real GPU, so
  rendering correctness remains validated only by running the binary.
