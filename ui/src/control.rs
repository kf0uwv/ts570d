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

//! Interactive control state machine for keyboard-driven radio commands.

use crossterm::event::{KeyCode, KeyEvent};

use crate::diag::DiagState;
use crate::RadioDisplay;

// ---------------------------------------------------------------------------
// State types
// ---------------------------------------------------------------------------

/// Top-level command groups displayed in the control panel menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandGroup {
    Frequency,
    Memory,
    ModeDsp,
    Receive,
    Transmit,
    Cw,
    Tones,
    System,
}

/// What radio action to perform when text input is confirmed.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum InputAction {
    SetVfoA,
    SetVfoB,
    SetAfGain,
    SetRfGain,
    SetSqLevel,
    SetMicGain,
    SetPower,
    SetVoxGain,
    SetVoxDelay,
    SetKeyerSpeed,
    // New
    SelectMemoryChannel,
    ReadMemoryChannel,
    WriteMemoryChannelFromVfoA,
    WriteMemoryChannelFromVfoB,
    ClearMemoryChannel,
    SetIfShiftFreq,
    SetHighCut,
    SetLowCut,
    SetCwPitch,
    SetSemiBreakInDelay,
    SendCw,
    SetCtcssToneNumber,
    SetToneNumber,
    VoiceRecall,
}

/// What radio action to perform when a list selection is confirmed.
#[derive(Debug, Clone)]
pub enum SelectAction {
    SetMode,
    SetAgc,
    SetNoiseReduction,
    SetAntenna,
    ToggleRit,
    ToggleXit,
    ToggleNb,
    TogglePreamp,
    ToggleAtt,
    ToggleVox,
    ToggleScan,
    ToggleLock,
    ToggleFine,
    // New
    SetRxVfo,
    SetTxVfo,
    SetBeatCancel,
    SetIfShiftDir,
    SetSpeechProcessor,
    SetAntennaThru,
    SetCwAutoZerobeat,
    SetCtcss,
    SetTone,
    SetAutoInfo,
    SetPowerOn,
}

/// The interactive control panel state machine.
#[allow(dead_code)]
#[derive(Default)]
pub enum ControlState {
    /// Showing top-level command group menu.
    #[default]
    Menu,
    /// Showing commands within a group.
    GroupMenu { group: CommandGroup, cursor: usize },
    /// User is typing text input.
    TextInput {
        prompt: String,
        buffer: String,
        error: Option<String>,
        action: InputAction,
    },
    /// User is selecting from a list.
    ListSelect {
        options: Vec<String>,
        cursor: usize,
        action: SelectAction,
    },
    /// Showing feedback after a command.
    Feedback { message: String, is_error: bool },
    /// Running or displaying diagnostics.
    Diagnostic(DiagState),
}

// ---------------------------------------------------------------------------
// KeyResult — returned by handle_key
// ---------------------------------------------------------------------------

/// The result of processing a key event.
pub enum KeyResult {
    /// Keep running — no radio command needed.
    Continue,
    /// Exit the UI.
    Quit,
    /// Execute a radio action with a validated value.
    Execute(ExecuteAction),
    /// Begin a diagnostic run.
    StartDiag,
}

/// A validated radio command ready to execute.
#[derive(Debug)]
pub enum ExecuteAction {
    // --- VFO ---
    SetVfoA(u64),
    SetVfoB(u64),
    // --- Existing toggles (renamed for clarity but kept for compat) ---
    SetAfGain(u8),
    SetRfGain(u8),
    SetSqLevel(u8),
    SetMicGain(u8),
    SetPower(u8),
    SetVoxGain(u8),
    SetVoxDelay(u16),
    SetKeyerSpeed(u8),
    SetMode(u8),           // 1-indexed per Mode enum
    SetAgc(u8),            // 0=Off,1=Fast,2=Mid,3=Mid-Slow,4=Slow
    SetNoiseReduction(u8), // 0=Off, 1=NR1, 2=NR2
    SetAntenna(u8),        // 1 or 2
    ToggleRit(bool),
    ToggleXit(bool),
    ToggleNb(bool),
    TogglePreamp(bool),
    ToggleAtt(bool),
    ToggleVox(bool),
    ToggleScan(bool),
    ToggleLock(bool),
    ToggleFine(bool),
    // --- Frequency group ---
    SetRxVfo(u8), // 0=A, 1=B, 2=Mem
    SetTxVfo(u8), // 0=A, 1=B
    ClearRit,
    RitUp,
    RitDown,
    // --- Memory group ---
    SelectMemoryChannel(u8),
    ReadMemoryChannel(u8),
    WriteMemoryChannelFromVfoA(u8),
    WriteMemoryChannelFromVfoB(u8),
    ClearMemoryChannel(u8),
    // --- Mode/DSP group ---
    SetBeatCancel(u8),   // 0=off, 1=on, 2=enhanced
    SetIfShiftDir(char), // ' ', '+', '-' — stored locally, no radio call
    SetIfShift(char, u16),
    SetHighCut(u8),
    SetLowCut(u8),
    // --- Transmit group ---
    Transmit,
    PttReceive,
    SetSpeechProcessor(bool),
    SetAntennaThru(bool),
    StartAntennaTuning,
    // --- CW group ---
    SetCwPitch(u8),
    SetSemiBreakInDelay(u16),
    SetCwAutoZerobeat(bool),
    SendCw(String),
    // --- Tones group ---
    SetCtcss(bool),
    SetCtcssToneNumber(u8),
    SetTone(bool),
    SetToneNumber(u8),
    // --- System group ---
    SetAutoInfo(u8),
    SetPowerOn(bool),
    VoiceRecall(u8),
    ResetPartial,
    ResetFull,
}

// ---------------------------------------------------------------------------
// Group command descriptors
// ---------------------------------------------------------------------------

/// How a menu item is activated.
enum CommandKind {
    /// Produces a TextInput state.
    Text {
        prompt: &'static str,
        action: InputAction,
    },
    /// Produces a ListSelect state.
    List {
        options: Vec<String>,
        action: SelectAction,
    },
    /// Immediately produces an ExecuteAction (no input needed).
    Immediate(ExecuteAction),
}

struct GroupCommand {
    label: &'static str,
    kind: CommandKind,
}

fn on_off() -> Vec<String> {
    vec!["On".to_string(), "Off".to_string()]
}

fn frequency_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            label: "Set VFO A frequency",
            kind: CommandKind::Text {
                prompt: "Enter freq MHz (e.g. 14.195):",
                action: InputAction::SetVfoA,
            },
        },
        GroupCommand {
            label: "Set VFO B frequency",
            kind: CommandKind::Text {
                prompt: "Enter freq MHz (e.g. 14.195):",
                action: InputAction::SetVfoB,
            },
        },
        GroupCommand {
            label: "Select RX VFO",
            kind: CommandKind::List {
                options: vec![
                    "VFO A".to_string(),
                    "VFO B".to_string(),
                    "Memory".to_string(),
                ],
                action: SelectAction::SetRxVfo,
            },
        },
        GroupCommand {
            label: "Select TX VFO",
            kind: CommandKind::List {
                options: vec!["VFO A".to_string(), "VFO B".to_string()],
                action: SelectAction::SetTxVfo,
            },
        },
        GroupCommand {
            label: "RIT on/off",
            kind: CommandKind::List {
                options: on_off(),
                action: SelectAction::ToggleRit,
            },
        },
        GroupCommand {
            label: "RIT clear",
            kind: CommandKind::Immediate(ExecuteAction::ClearRit),
        },
        GroupCommand {
            label: "RIT up",
            kind: CommandKind::Immediate(ExecuteAction::RitUp),
        },
        GroupCommand {
            label: "RIT down",
            kind: CommandKind::Immediate(ExecuteAction::RitDown),
        },
        GroupCommand {
            label: "XIT on/off",
            kind: CommandKind::List {
                options: on_off(),
                action: SelectAction::ToggleXit,
            },
        },
        GroupCommand {
            label: "Fine step on/off",
            kind: CommandKind::List {
                options: on_off(),
                action: SelectAction::ToggleFine,
            },
        },
        GroupCommand {
            label: "Frequency lock on/off",
            kind: CommandKind::List {
                options: on_off(),
                action: SelectAction::ToggleLock,
            },
        },
        GroupCommand {
            label: "Scan on/off",
            kind: CommandKind::List {
                options: on_off(),
                action: SelectAction::ToggleScan,
            },
        },
    ]
}

fn memory_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            label: "Select channel (0-99)",
            kind: CommandKind::Text {
                prompt: "Channel number (0-99):",
                action: InputAction::SelectMemoryChannel,
            },
        },
        GroupCommand {
            label: "Read channel (0-99)",
            kind: CommandKind::Text {
                prompt: "Read channel number (0-99):",
                action: InputAction::ReadMemoryChannel,
            },
        },
        GroupCommand {
            label: "Write channel from VFO A (0-99)",
            kind: CommandKind::Text {
                prompt: "Write from VFO A to channel (0-99):",
                action: InputAction::WriteMemoryChannelFromVfoA,
            },
        },
        GroupCommand {
            label: "Write channel from VFO B (0-99)",
            kind: CommandKind::Text {
                prompt: "Write from VFO B to channel (0-99):",
                action: InputAction::WriteMemoryChannelFromVfoB,
            },
        },
        GroupCommand {
            label: "Clear channel (0-99)",
            kind: CommandKind::Text {
                prompt: "Clear channel number (0-99):",
                action: InputAction::ClearMemoryChannel,
            },
        },
    ]
}

fn mode_dsp_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            label: "Operating mode",
            kind: CommandKind::List {
                options: vec![
                    "LSB".to_string(),
                    "USB".to_string(),
                    "CW".to_string(),
                    "CW-R".to_string(),
                    "FSK".to_string(),
                    "FSK-R".to_string(),
                    "FM".to_string(),
                    "AM".to_string(),
                ],
                action: SelectAction::SetMode,
            },
        },
        GroupCommand {
            label: "AGC",
            kind: CommandKind::List {
                options: vec![
                    "Off".to_string(),
                    "Fast".to_string(),
                    "Mid".to_string(),
                    "Mid-Slow".to_string(),
                    "Slow".to_string(),
                ],
                action: SelectAction::SetAgc,
            },
        },
        GroupCommand {
            label: "Beat cancel",
            kind: CommandKind::List {
                options: vec!["Off".to_string(), "On".to_string(), "Enhanced".to_string()],
                action: SelectAction::SetBeatCancel,
            },
        },
        GroupCommand {
            label: "IF shift direction",
            kind: CommandKind::List {
                options: vec!["Center".to_string(), "+".to_string(), "-".to_string()],
                action: SelectAction::SetIfShiftDir,
            },
        },
        GroupCommand {
            label: "IF shift frequency (0-9999 Hz)",
            kind: CommandKind::Text {
                prompt: "IF shift Hz (0-9999):",
                action: InputAction::SetIfShiftFreq,
            },
        },
        GroupCommand {
            label: "DSP high cut (0-20)",
            kind: CommandKind::Text {
                prompt: "High cut index (0-20):",
                action: InputAction::SetHighCut,
            },
        },
        GroupCommand {
            label: "DSP low cut (0-20)",
            kind: CommandKind::Text {
                prompt: "Low cut index (0-20):",
                action: InputAction::SetLowCut,
            },
        },
    ]
}

fn receive_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            label: "AF gain (0-100%)",
            kind: CommandKind::Text {
                prompt: "AF gain % (0-100):",
                action: InputAction::SetAfGain,
            },
        },
        GroupCommand {
            label: "RF gain (0-100%)",
            kind: CommandKind::Text {
                prompt: "RF gain % (0-100):",
                action: InputAction::SetRfGain,
            },
        },
        GroupCommand {
            label: "Squelch (0-255)",
            kind: CommandKind::Text {
                prompt: "Squelch (0-255):",
                action: InputAction::SetSqLevel,
            },
        },
        GroupCommand {
            label: "Preamp on/off",
            kind: CommandKind::List {
                options: on_off(),
                action: SelectAction::TogglePreamp,
            },
        },
        GroupCommand {
            label: "Attenuator on/off",
            kind: CommandKind::List {
                options: on_off(),
                action: SelectAction::ToggleAtt,
            },
        },
        GroupCommand {
            label: "Noise blanker on/off",
            kind: CommandKind::List {
                options: on_off(),
                action: SelectAction::ToggleNb,
            },
        },
        GroupCommand {
            label: "Noise reduction",
            kind: CommandKind::List {
                options: vec!["Off".to_string(), "NR1".to_string(), "NR2".to_string()],
                action: SelectAction::SetNoiseReduction,
            },
        },
    ]
}

fn transmit_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            label: "TX power (5-100, mult of 5)",
            kind: CommandKind::Text {
                prompt: "TX power (5-100, multiple of 5):",
                action: InputAction::SetPower,
            },
        },
        GroupCommand {
            label: "PTT transmit",
            kind: CommandKind::Immediate(ExecuteAction::Transmit),
        },
        GroupCommand {
            label: "PTT receive",
            kind: CommandKind::Immediate(ExecuteAction::PttReceive),
        },
        GroupCommand {
            label: "Mic gain (0-100%)",
            kind: CommandKind::Text {
                prompt: "MIC gain % (0-100):",
                action: InputAction::SetMicGain,
            },
        },
        GroupCommand {
            label: "Speech processor on/off",
            kind: CommandKind::List {
                options: on_off(),
                action: SelectAction::SetSpeechProcessor,
            },
        },
        GroupCommand {
            label: "VOX on/off",
            kind: CommandKind::List {
                options: on_off(),
                action: SelectAction::ToggleVox,
            },
        },
        GroupCommand {
            label: "VOX gain (1-9)",
            kind: CommandKind::Text {
                prompt: "VOX gain (1-9):",
                action: InputAction::SetVoxGain,
            },
        },
        GroupCommand {
            label: "VOX delay (0-3000 ms)",
            kind: CommandKind::Text {
                prompt: "VOX delay ms (0-3000):",
                action: InputAction::SetVoxDelay,
            },
        },
        GroupCommand {
            label: "Antenna",
            kind: CommandKind::List {
                options: vec!["ANT1".to_string(), "ANT2".to_string()],
                action: SelectAction::SetAntenna,
            },
        },
        GroupCommand {
            label: "Antenna tuner thru on/off",
            kind: CommandKind::List {
                options: on_off(),
                action: SelectAction::SetAntennaThru,
            },
        },
        GroupCommand {
            label: "Start antenna tuning",
            kind: CommandKind::Immediate(ExecuteAction::StartAntennaTuning),
        },
    ]
}

fn cw_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            label: "Keyer speed (5-60 WPM)",
            kind: CommandKind::Text {
                prompt: "Keyer speed WPM (5-60):",
                action: InputAction::SetKeyerSpeed,
            },
        },
        GroupCommand {
            label: "CW pitch (0-12)",
            kind: CommandKind::Text {
                prompt: "CW pitch (0-12):",
                action: InputAction::SetCwPitch,
            },
        },
        GroupCommand {
            label: "Semi break-in delay (0-1000 ms)",
            kind: CommandKind::Text {
                prompt: "Semi break-in delay ms (0-1000):",
                action: InputAction::SetSemiBreakInDelay,
            },
        },
        GroupCommand {
            label: "CW auto zero-beat on/off",
            kind: CommandKind::List {
                options: on_off(),
                action: SelectAction::SetCwAutoZerobeat,
            },
        },
        GroupCommand {
            label: "Send CW message (up to 24 chars)",
            kind: CommandKind::Text {
                prompt: "CW message (up to 24 chars):",
                action: InputAction::SendCw,
            },
        },
    ]
}

fn tones_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            label: "CTCSS on/off",
            kind: CommandKind::List {
                options: on_off(),
                action: SelectAction::SetCtcss,
            },
        },
        GroupCommand {
            label: "CTCSS tone number (0-39)",
            kind: CommandKind::Text {
                prompt: "CTCSS tone number (0-39):",
                action: InputAction::SetCtcssToneNumber,
            },
        },
        GroupCommand {
            label: "Tone on/off",
            kind: CommandKind::List {
                options: on_off(),
                action: SelectAction::SetTone,
            },
        },
        GroupCommand {
            label: "Tone number (0-39)",
            kind: CommandKind::Text {
                prompt: "Tone number (0-39):",
                action: InputAction::SetToneNumber,
            },
        },
    ]
}

fn system_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            label: "Auto-info",
            kind: CommandKind::List {
                options: vec!["Off".to_string(), "On".to_string()],
                action: SelectAction::SetAutoInfo,
            },
        },
        GroupCommand {
            label: "Power on/off",
            kind: CommandKind::List {
                options: on_off(),
                action: SelectAction::SetPowerOn,
            },
        },
        GroupCommand {
            label: "Voice recall (1-3)",
            kind: CommandKind::Text {
                prompt: "Voice recall number (1-3):",
                action: InputAction::VoiceRecall,
            },
        },
        GroupCommand {
            label: "Reset partial",
            kind: CommandKind::Immediate(ExecuteAction::ResetPartial),
        },
        GroupCommand {
            label: "Reset full",
            kind: CommandKind::Immediate(ExecuteAction::ResetFull),
        },
    ]
}

fn group_commands(group: CommandGroup) -> Vec<GroupCommand> {
    match group {
        CommandGroup::Frequency => frequency_commands(),
        CommandGroup::Memory => memory_commands(),
        CommandGroup::ModeDsp => mode_dsp_commands(),
        CommandGroup::Receive => receive_commands(),
        CommandGroup::Transmit => transmit_commands(),
        CommandGroup::Cw => cw_commands(),
        CommandGroup::Tones => tones_commands(),
        CommandGroup::System => system_commands(),
    }
}

pub fn group_command_labels(group: CommandGroup) -> Vec<&'static str> {
    group_commands(group).into_iter().map(|c| c.label).collect()
}

// ---------------------------------------------------------------------------
// Transition helpers — build the next state for a selected command
// ---------------------------------------------------------------------------

/// Select a group command by index, returning the next ControlState
/// (or None if the index is out of range).
///
/// For Immediate actions, returns ImmediateResult to signal the caller
/// to fire the action directly.
pub enum CommandTransition {
    State(ControlState),
    Immediate(ExecuteAction),
    None,
}

fn select_group_command(group: CommandGroup, idx: usize) -> CommandTransition {
    let cmds = group_commands(group);
    if idx >= cmds.len() {
        return CommandTransition::None;
    }
    let cmd = cmds.into_iter().nth(idx).unwrap();
    match cmd.kind {
        CommandKind::Text { prompt, action } => CommandTransition::State(ControlState::TextInput {
            prompt: prompt.to_string(),
            buffer: String::new(),
            error: None,
            action,
        }),
        CommandKind::List { options, action } => {
            CommandTransition::State(ControlState::ListSelect {
                options,
                cursor: 0,
                action,
            })
        }
        CommandKind::Immediate(exec) => CommandTransition::Immediate(exec),
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validate_text_input(action: &InputAction, buffer: &str) -> Result<ExecuteAction, String> {
    match action {
        InputAction::SetVfoA | InputAction::SetVfoB => {
            let mhz: f64 = buffer
                .parse()
                .map_err(|_| "Enter a number like 14.195".to_string())?;
            if !(0.5..=60.0).contains(&mhz) {
                return Err("Frequency must be 0.5–60.0 MHz".to_string());
            }
            let hz = (mhz * 1_000_000.0).round() as u64;
            Ok(match action {
                InputAction::SetVfoA => ExecuteAction::SetVfoA(hz),
                _ => ExecuteAction::SetVfoB(hz),
            })
        }
        InputAction::SetAfGain | InputAction::SetRfGain => {
            let v: u16 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–100".to_string())?;
            if v > 100 {
                return Err("Value must be 0–100".to_string());
            }
            let mapped = (v as f64 * 2.55).round() as u8;
            Ok(match action {
                InputAction::SetAfGain => ExecuteAction::SetAfGain(mapped),
                _ => ExecuteAction::SetRfGain(mapped),
            })
        }
        InputAction::SetSqLevel => {
            let v: u16 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–255".to_string())?;
            if v > 255 {
                return Err("Value must be 0–255".to_string());
            }
            Ok(ExecuteAction::SetSqLevel(v as u8))
        }
        InputAction::SetMicGain => {
            let v: u16 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–100".to_string())?;
            if v > 100 {
                return Err("Value must be 0–100".to_string());
            }
            let mapped = (v as f64 * 2.55).round() as u8;
            Ok(ExecuteAction::SetMicGain(mapped))
        }
        InputAction::SetPower => {
            let v: u16 = buffer
                .parse()
                .map_err(|_| "Enter a number 5–100 (multiple of 5)".to_string())?;
            if !(5..=100).contains(&v) || v % 5 != 0 {
                return Err("Value must be 5–100, multiple of 5".to_string());
            }
            Ok(ExecuteAction::SetPower(v as u8))
        }
        InputAction::SetVoxGain => {
            let v: u8 = buffer
                .parse()
                .map_err(|_| "Enter a number 1–9".to_string())?;
            if !(1..=9).contains(&v) {
                return Err("Value must be 1–9".to_string());
            }
            Ok(ExecuteAction::SetVoxGain(v))
        }
        InputAction::SetVoxDelay => {
            let v: u16 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–3000".to_string())?;
            if v > 3000 {
                return Err("Value must be 0–3000".to_string());
            }
            Ok(ExecuteAction::SetVoxDelay(v))
        }
        InputAction::SetKeyerSpeed => {
            let v: u8 = buffer
                .parse()
                .map_err(|_| "Enter a number 5–60".to_string())?;
            if !(5..=60).contains(&v) {
                return Err("Value must be 5–60".to_string());
            }
            Ok(ExecuteAction::SetKeyerSpeed(v))
        }
        InputAction::SelectMemoryChannel => {
            let v: u8 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–99".to_string())?;
            if v > 99 {
                return Err("Channel must be 0–99".to_string());
            }
            Ok(ExecuteAction::SelectMemoryChannel(v))
        }
        InputAction::ReadMemoryChannel => {
            let v: u8 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–99".to_string())?;
            if v > 99 {
                return Err("Channel must be 0–99".to_string());
            }
            Ok(ExecuteAction::ReadMemoryChannel(v))
        }
        InputAction::WriteMemoryChannelFromVfoA => {
            let v: u8 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–99".to_string())?;
            if v > 99 {
                return Err("Channel must be 0–99".to_string());
            }
            Ok(ExecuteAction::WriteMemoryChannelFromVfoA(v))
        }
        InputAction::WriteMemoryChannelFromVfoB => {
            let v: u8 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–99".to_string())?;
            if v > 99 {
                return Err("Channel must be 0–99".to_string());
            }
            Ok(ExecuteAction::WriteMemoryChannelFromVfoB(v))
        }
        InputAction::ClearMemoryChannel => {
            let v: u8 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–99".to_string())?;
            if v > 99 {
                return Err("Channel must be 0–99".to_string());
            }
            Ok(ExecuteAction::ClearMemoryChannel(v))
        }
        InputAction::SetIfShiftFreq => {
            let v: u16 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–9999".to_string())?;
            if v > 9999 {
                return Err("Value must be 0–9999".to_string());
            }
            // Direction is stored in terminal state; we pass 0 here as placeholder
            // The actual direction will be merged in terminal.rs
            Ok(ExecuteAction::SetIfShift(' ', v))
        }
        InputAction::SetHighCut => {
            let v: u8 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–20".to_string())?;
            if v > 20 {
                return Err("Value must be 0–20".to_string());
            }
            Ok(ExecuteAction::SetHighCut(v))
        }
        InputAction::SetLowCut => {
            let v: u8 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–20".to_string())?;
            if v > 20 {
                return Err("Value must be 0–20".to_string());
            }
            Ok(ExecuteAction::SetLowCut(v))
        }
        InputAction::SetCwPitch => {
            let v: u8 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–12".to_string())?;
            if v > 12 {
                return Err("Value must be 0–12".to_string());
            }
            Ok(ExecuteAction::SetCwPitch(v))
        }
        InputAction::SetSemiBreakInDelay => {
            let v: u16 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–1000".to_string())?;
            if v > 1000 {
                return Err("Value must be 0–1000".to_string());
            }
            Ok(ExecuteAction::SetSemiBreakInDelay(v))
        }
        InputAction::SendCw => {
            if buffer.len() > 24 {
                return Err("Message must be at most 24 characters".to_string());
            }
            Ok(ExecuteAction::SendCw(buffer.to_string()))
        }
        InputAction::SetCtcssToneNumber => {
            let v: u8 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–39".to_string())?;
            if v > 39 {
                return Err("Value must be 0–39".to_string());
            }
            Ok(ExecuteAction::SetCtcssToneNumber(v))
        }
        InputAction::SetToneNumber => {
            let v: u8 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–39".to_string())?;
            if v > 39 {
                return Err("Value must be 0–39".to_string());
            }
            Ok(ExecuteAction::SetToneNumber(v))
        }
        InputAction::VoiceRecall => {
            let v: u8 = buffer
                .parse()
                .map_err(|_| "Enter a number 1–3".to_string())?;
            if !(1..=3).contains(&v) {
                return Err("Value must be 1–3".to_string());
            }
            Ok(ExecuteAction::VoiceRecall(v))
        }
    }
}

fn select_action_to_execute(action: &SelectAction, cursor: usize) -> ExecuteAction {
    match action {
        SelectAction::SetMode => {
            // options: LSB USB CW CW-R FSK FSK-R FM AM
            // Map to Mode enum values: Lsb=1 Usb=2 Cw=3 CwReverse=7 Fsk=6 FskReverse=9 Fm=4 Am=5
            let mode_u8 = match cursor {
                0 => 1u8, // LSB
                1 => 2,   // USB
                2 => 3,   // CW
                3 => 7,   // CW-R
                4 => 6,   // FSK
                5 => 9,   // FSK-R
                6 => 4,   // FM
                7 => 5,   // AM
                _ => 2,
            };
            ExecuteAction::SetMode(mode_u8)
        }
        SelectAction::SetAgc => ExecuteAction::SetAgc(cursor as u8), // 0=Off,1=Fast,2=Mid,3=Mid-Slow,4=Slow
        SelectAction::SetNoiseReduction => ExecuteAction::SetNoiseReduction(cursor as u8), // 0=Off,1=NR1,2=NR2
        SelectAction::SetAntenna => ExecuteAction::SetAntenna(cursor as u8 + 1), // 1-indexed
        SelectAction::ToggleRit => ExecuteAction::ToggleRit(cursor == 0),
        SelectAction::ToggleXit => ExecuteAction::ToggleXit(cursor == 0),
        SelectAction::ToggleNb => ExecuteAction::ToggleNb(cursor == 0),
        SelectAction::TogglePreamp => ExecuteAction::TogglePreamp(cursor == 0),
        SelectAction::ToggleAtt => ExecuteAction::ToggleAtt(cursor == 0),
        SelectAction::ToggleVox => ExecuteAction::ToggleVox(cursor == 0),
        SelectAction::ToggleScan => ExecuteAction::ToggleScan(cursor == 0),
        SelectAction::ToggleLock => ExecuteAction::ToggleLock(cursor == 0),
        SelectAction::ToggleFine => ExecuteAction::ToggleFine(cursor == 0),
        SelectAction::SetRxVfo => ExecuteAction::SetRxVfo(cursor as u8), // 0=A,1=B,2=Mem
        SelectAction::SetTxVfo => ExecuteAction::SetTxVfo(cursor as u8), // 0=A,1=B
        SelectAction::SetBeatCancel => ExecuteAction::SetBeatCancel(cursor as u8), // 0=off,1=on,2=enhanced
        SelectAction::SetIfShiftDir => {
            let dir = match cursor {
                0 => ' ',
                1 => '+',
                _ => '-',
            };
            ExecuteAction::SetIfShiftDir(dir)
        }
        SelectAction::SetSpeechProcessor => ExecuteAction::SetSpeechProcessor(cursor == 0),
        SelectAction::SetAntennaThru => ExecuteAction::SetAntennaThru(cursor == 0),
        SelectAction::SetCwAutoZerobeat => ExecuteAction::SetCwAutoZerobeat(cursor == 0),
        SelectAction::SetCtcss => ExecuteAction::SetCtcss(cursor == 0),
        SelectAction::SetTone => ExecuteAction::SetTone(cursor == 0),
        SelectAction::SetAutoInfo => ExecuteAction::SetAutoInfo(cursor as u8), // 0=off, 1=on
        SelectAction::SetPowerOn => ExecuteAction::SetPowerOn(cursor == 0),
    }
}

// ---------------------------------------------------------------------------
// Initial cursor position for list selects
// ---------------------------------------------------------------------------

/// Return the cursor index that should be pre-selected when a list opens,
/// based on the current radio state so the highlight starts on the active value.
pub fn initial_list_cursor(action: &SelectAction, radio: &RadioDisplay) -> usize {
    match action {
        SelectAction::SetMode => {
            // options: LSB USB CW CW-R FSK FSK-R FM AM
            // radio.mode is the name string from Mode::name()
            match radio.mode.as_str() {
                "LSB" => 0,
                "USB" => 1,
                "CW" => 2,
                "CW-R" => 3,
                "FSK" => 4,
                "FSK-R" => 5,
                "FM" => 6,
                "AM" => 7,
                _ => 0,
            }
        }
        SelectAction::SetAgc => radio.agc.min(4) as usize,
        SelectAction::SetNoiseReduction => radio.noise_reduction.min(2) as usize,
        SelectAction::SetAntenna => {
            // antenna is 1-indexed; options: ANT1=0, ANT2=1
            radio.antenna.saturating_sub(1).min(1) as usize
        }
        SelectAction::SetRxVfo => radio.rx_vfo.min(2) as usize,
        SelectAction::SetTxVfo => radio.tx_vfo.min(1) as usize,
        SelectAction::SetBeatCancel => radio.beat_cancel.min(2) as usize,
        // on_off() options: On=0, Off=1
        SelectAction::ToggleRit => {
            if radio.rit {
                0
            } else {
                1
            }
        }
        SelectAction::ToggleXit => {
            if radio.xit {
                0
            } else {
                1
            }
        }
        SelectAction::ToggleNb => {
            if radio.noise_blanker {
                0
            } else {
                1
            }
        }
        SelectAction::TogglePreamp => {
            if radio.preamp {
                0
            } else {
                1
            }
        }
        SelectAction::ToggleAtt => {
            if radio.attenuator {
                0
            } else {
                1
            }
        }
        SelectAction::ToggleVox => {
            if radio.vox {
                0
            } else {
                1
            }
        }
        SelectAction::ToggleScan => {
            if radio.scan {
                0
            } else {
                1
            }
        }
        SelectAction::ToggleLock => {
            if radio.freq_lock {
                0
            } else {
                1
            }
        }
        SelectAction::ToggleFine => {
            if radio.fine_step {
                0
            } else {
                1
            }
        }
        SelectAction::SetSpeechProcessor => {
            if radio.speech_processor {
                0
            } else {
                1
            }
        }
        SelectAction::SetCtcss => {
            if radio.ctcss {
                0
            } else {
                1
            }
        }
        // All other actions: default to 0
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// handle_key — the main event handler
// ---------------------------------------------------------------------------

/// Process a key event and transition the control state.
///
/// Returns `KeyResult::Continue`, `KeyResult::Quit`, or `KeyResult::Execute`.
pub fn handle_key(key: KeyEvent, state: &mut ControlState, radio: &RadioDisplay) -> KeyResult {
    match state {
        ControlState::Menu => match key.code {
            KeyCode::Char('f') | KeyCode::Char('F') => {
                *state = ControlState::GroupMenu {
                    group: CommandGroup::Frequency,
                    cursor: 0,
                };
                KeyResult::Continue
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                *state = ControlState::GroupMenu {
                    group: CommandGroup::Memory,
                    cursor: 0,
                };
                KeyResult::Continue
            }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                *state = ControlState::GroupMenu {
                    group: CommandGroup::ModeDsp,
                    cursor: 0,
                };
                KeyResult::Continue
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                *state = ControlState::GroupMenu {
                    group: CommandGroup::Receive,
                    cursor: 0,
                };
                KeyResult::Continue
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                *state = ControlState::GroupMenu {
                    group: CommandGroup::Transmit,
                    cursor: 0,
                };
                KeyResult::Continue
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                *state = ControlState::GroupMenu {
                    group: CommandGroup::Cw,
                    cursor: 0,
                };
                KeyResult::Continue
            }
            KeyCode::Char('o') | KeyCode::Char('O') => {
                *state = ControlState::GroupMenu {
                    group: CommandGroup::Tones,
                    cursor: 0,
                };
                KeyResult::Continue
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                *state = ControlState::GroupMenu {
                    group: CommandGroup::System,
                    cursor: 0,
                };
                KeyResult::Continue
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                *state = ControlState::Diagnostic(DiagState::Running {
                    current_label: "starting…",
                    current_round: 1,
                    results: Vec::new(),
                });
                KeyResult::StartDiag
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => KeyResult::Quit,
            _ => KeyResult::Continue,
        },

        ControlState::GroupMenu { group, .. } => {
            let group = *group;
            let idx_opt = match key.code {
                KeyCode::Char('1') => Some(0),
                KeyCode::Char('2') => Some(1),
                KeyCode::Char('3') => Some(2),
                KeyCode::Char('4') => Some(3),
                KeyCode::Char('5') => Some(4),
                KeyCode::Char('6') => Some(5),
                KeyCode::Char('7') => Some(6),
                KeyCode::Char('8') => Some(7),
                KeyCode::Char('9') => Some(8),
                KeyCode::Char('a') | KeyCode::Char('A') => Some(9),
                KeyCode::Char('b') | KeyCode::Char('B') => Some(10),
                KeyCode::Char('c') | KeyCode::Char('C') => Some(11),
                KeyCode::Esc => {
                    *state = ControlState::Menu;
                    return KeyResult::Continue;
                }
                _ => None,
            };
            if let Some(idx) = idx_opt {
                match select_group_command(group, idx) {
                    CommandTransition::State(mut next) => {
                        // Pre-select the cursor to the current radio value.
                        if let ControlState::ListSelect {
                            ref action,
                            ref mut cursor,
                            ..
                        } = next
                        {
                            *cursor = initial_list_cursor(action, radio);
                        }
                        *state = next;
                    }
                    CommandTransition::Immediate(exec) => {
                        *state = ControlState::Feedback {
                            message: String::new(),
                            is_error: false,
                        };
                        return KeyResult::Execute(exec);
                    }
                    CommandTransition::None => {}
                }
            }
            KeyResult::Continue
        }

        ControlState::TextInput {
            buffer,
            error,
            action,
            ..
        } => match key.code {
            KeyCode::Char(c) if c.is_ascii_graphic() || c == ' ' => {
                buffer.push(c);
                *error = None;
                KeyResult::Continue
            }
            KeyCode::Backspace => {
                buffer.pop();
                *error = None;
                KeyResult::Continue
            }
            KeyCode::Enter => {
                let buf = buffer.clone();
                let act = action.clone();
                match validate_text_input(&act, &buf) {
                    Ok(exec) => {
                        *state = ControlState::Feedback {
                            message: String::new(), // will be filled by caller after execute
                            is_error: false,
                        };
                        KeyResult::Execute(exec)
                    }
                    Err(msg) => {
                        if let ControlState::TextInput { error, .. } = state {
                            *error = Some(msg);
                        }
                        KeyResult::Continue
                    }
                }
            }
            KeyCode::Esc => {
                *state = ControlState::Menu;
                KeyResult::Continue
            }
            _ => KeyResult::Continue,
        },

        ControlState::ListSelect {
            options,
            cursor,
            action,
        } => match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                if *cursor > 0 {
                    *cursor -= 1;
                }
                KeyResult::Continue
            }
            KeyCode::Right | KeyCode::Char('l') => {
                let max = options.len().saturating_sub(1);
                if *cursor < max {
                    *cursor += 1;
                }
                KeyResult::Continue
            }
            KeyCode::Enter => {
                let exec = select_action_to_execute(action, *cursor);
                *state = ControlState::Feedback {
                    message: String::new(), // will be filled by caller
                    is_error: false,
                };
                KeyResult::Execute(exec)
            }
            KeyCode::Esc => {
                *state = ControlState::Menu;
                KeyResult::Continue
            }
            _ => KeyResult::Continue,
        },

        ControlState::Feedback { .. } => {
            *state = ControlState::Menu;
            KeyResult::Continue
        }

        ControlState::Diagnostic(DiagState::Done { scroll, .. }) => match key.code {
            KeyCode::Up => {
                *scroll = scroll.saturating_sub(1);
                KeyResult::Continue
            }
            KeyCode::Down => {
                *scroll = scroll.saturating_add(1);
                KeyResult::Continue
            }
            KeyCode::PageUp => {
                *scroll = scroll.saturating_sub(10);
                KeyResult::Continue
            }
            KeyCode::PageDown => {
                *scroll = scroll.saturating_add(10);
                KeyResult::Continue
            }
            KeyCode::Esc => {
                *state = ControlState::Menu;
                KeyResult::Continue
            }
            _ => KeyResult::Continue,
        },

        ControlState::Diagnostic(_) => match key.code {
            KeyCode::Esc => {
                *state = ControlState::Menu;
                KeyResult::Continue
            }
            _ => KeyResult::Continue,
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn test_menu_f_transitions_to_frequency_group() {
        let mut state = ControlState::Menu;
        let radio = RadioDisplay::default();
        let result = handle_key(key(KeyCode::Char('f')), &mut state, &radio);
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::Frequency,
                ..
            }
        ));
    }

    #[test]
    fn test_menu_n_transitions_to_memory_group() {
        let mut state = ControlState::Menu;
        let radio = RadioDisplay::default();
        let result = handle_key(key(KeyCode::Char('n')), &mut state, &radio);
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::Memory,
                ..
            }
        ));
    }

    #[test]
    fn test_menu_c_transitions_to_cw_group() {
        let mut state = ControlState::Menu;
        let radio = RadioDisplay::default();
        let result = handle_key(key(KeyCode::Char('c')), &mut state, &radio);
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(
            state,
            ControlState::GroupMenu {
                group: CommandGroup::Cw,
                ..
            }
        ));
    }

    #[test]
    fn test_menu_q_quits() {
        let mut state = ControlState::Menu;
        let radio = RadioDisplay::default();
        let result = handle_key(key(KeyCode::Char('q')), &mut state, &radio);
        assert!(matches!(result, KeyResult::Quit));
    }

    #[test]
    fn test_group_menu_esc_returns_to_menu() {
        let mut state = ControlState::GroupMenu {
            group: CommandGroup::ModeDsp,
            cursor: 0,
        };
        let radio = RadioDisplay::default();
        let result = handle_key(key(KeyCode::Esc), &mut state, &radio);
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    #[test]
    fn test_group_menu_1_transitions_to_text_input() {
        let mut state = ControlState::GroupMenu {
            group: CommandGroup::Frequency,
            cursor: 0,
        };
        let radio = RadioDisplay::default();
        handle_key(key(KeyCode::Char('1')), &mut state, &radio);
        assert!(matches!(state, ControlState::TextInput { .. }));
    }

    #[test]
    fn test_text_input_valid_frequency() {
        let mut state = ControlState::TextInput {
            prompt: "Enter freq MHz:".to_string(),
            buffer: "14.195".to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        let radio = RadioDisplay::default();
        let result = handle_key(key(KeyCode::Enter), &mut state, &radio);
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::SetVfoA(_))
        ));
    }

    #[test]
    fn test_text_input_invalid_frequency_sets_error() {
        let mut state = ControlState::TextInput {
            prompt: "Enter freq MHz:".to_string(),
            buffer: "999.0".to_string(),
            error: None,
            action: InputAction::SetVfoA,
        };
        let radio = RadioDisplay::default();
        let result = handle_key(key(KeyCode::Enter), &mut state, &radio);
        assert!(matches!(result, KeyResult::Continue));
        if let ControlState::TextInput { error, .. } = &state {
            assert!(error.is_some());
        } else {
            panic!("Expected TextInput state");
        }
    }

    #[test]
    fn test_list_select_enter_produces_execute() {
        let mut state = ControlState::ListSelect {
            options: vec!["On".to_string(), "Off".to_string()],
            cursor: 1,
            action: SelectAction::ToggleRit,
        };
        let radio = RadioDisplay::default();
        let result = handle_key(key(KeyCode::Enter), &mut state, &radio);
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::ToggleRit(false))
        ));
    }

    #[test]
    fn test_feedback_any_key_returns_to_menu() {
        let mut state = ControlState::Feedback {
            message: "OK".to_string(),
            is_error: false,
        };
        let radio = RadioDisplay::default();
        let result = handle_key(key(KeyCode::Enter), &mut state, &radio);
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    #[test]
    fn test_immediate_action_rit_clear() {
        // Item 6 (index 5) in Frequency is "RIT clear" → Immediate
        let mut state = ControlState::GroupMenu {
            group: CommandGroup::Frequency,
            cursor: 0,
        };
        let radio = RadioDisplay::default();
        let result = handle_key(key(KeyCode::Char('6')), &mut state, &radio);
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::ClearRit)
        ));
    }

    #[test]
    fn test_frequency_group_has_12_items() {
        let labels = group_command_labels(CommandGroup::Frequency);
        assert_eq!(labels.len(), 12);
    }

    #[test]
    fn test_transmit_ptt_immediate() {
        // Item 2 (index 1) in Transmit is "PTT transmit" → Immediate
        let mut state = ControlState::GroupMenu {
            group: CommandGroup::Transmit,
            cursor: 0,
        };
        let radio = RadioDisplay::default();
        let result = handle_key(key(KeyCode::Char('2')), &mut state, &radio);
        assert!(matches!(
            result,
            KeyResult::Execute(ExecuteAction::Transmit)
        ));
    }
}
