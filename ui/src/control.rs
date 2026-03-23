//! Interactive control state machine for keyboard-driven radio commands.

use crossterm::event::{KeyCode, KeyEvent};

// ---------------------------------------------------------------------------
// State types
// ---------------------------------------------------------------------------

/// Top-level command groups displayed in the control panel menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandGroup {
    Frequency,
    Mode,
    Receive,
    Transmission,
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
    ToggleProc,
    ToggleScan,
    ToggleLock,
    ToggleFine,
}

/// The interactive control panel state machine.
#[allow(dead_code)]
#[derive(Default)]
pub enum ControlState {
    /// Showing top-level command group menu.
    #[default]
    Menu,
    /// Showing commands within a group.
    GroupMenu { group: CommandGroup, cursor: usize }, // cursor reserved for future use
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
}

/// A validated radio command ready to execute.
#[derive(Debug)]
pub enum ExecuteAction {
    SetVfoA(u64),
    SetVfoB(u64),
    SetAfGain(u8),
    SetRfGain(u8),
    SetSqLevel(u8),
    SetMicGain(u8),
    SetPower(u8),
    SetVoxGain(u8),
    SetVoxDelay(u16),
    SetKeyerSpeed(u8),
    SetMode(u8),           // 1-indexed per Mode enum
    SetAgc(u8),            // 0=Off, 1=Slow, 2=Mid, 3=Fast
    SetNoiseReduction(u8), // 0=Off, 1=NR1, 2=NR2
    SetAntenna(u8),        // 1 or 2
    ToggleRit(bool),
    ToggleXit(bool),
    ToggleNb(bool),
    TogglePreamp(bool),
    ToggleAtt(bool),
    ToggleVox(bool),
    ToggleProc(bool),
    ToggleScan(bool),
    ToggleLock(bool),
    ToggleFine(bool),
}

// ---------------------------------------------------------------------------
// Group command descriptors
// ---------------------------------------------------------------------------

struct GroupCommand {
    label: &'static str,
}

fn frequency_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            label: "Set VFO A frequency",
        },
        GroupCommand {
            label: "Set VFO B frequency",
        },
        GroupCommand {
            label: "RIT on/off",
        },
        GroupCommand {
            label: "XIT on/off",
        },
        GroupCommand {
            label: "Frequency lock",
        },
        GroupCommand { label: "Fine step" },
        GroupCommand { label: "Scan" },
    ]
}

fn mode_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            label: "Operating mode",
        },
        GroupCommand {
            label: "Noise reduction",
        },
        GroupCommand { label: "AGC" },
        GroupCommand {
            label: "Noise blanker",
        },
    ]
}

fn receive_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            label: "AF gain (0-255)",
        },
        GroupCommand {
            label: "RF gain (0-255)",
        },
        GroupCommand {
            label: "Squelch (0-100)",
        },
        GroupCommand {
            label: "MIC gain (0-100)",
        },
    ]
}

fn transmission_commands() -> Vec<GroupCommand> {
    vec![
        GroupCommand {
            label: "TX power (0-100)",
        },
        GroupCommand { label: "Preamp" },
        GroupCommand {
            label: "Attenuator",
        },
        GroupCommand { label: "VOX" },
        GroupCommand {
            label: "VOX gain (0-100)",
        },
        GroupCommand {
            label: "Speech processor",
        },
        GroupCommand { label: "Antenna" },
    ]
}

pub fn group_command_labels(group: CommandGroup) -> Vec<&'static str> {
    let cmds = match group {
        CommandGroup::Frequency => frequency_commands(),
        CommandGroup::Mode => mode_commands(),
        CommandGroup::Receive => receive_commands(),
        CommandGroup::Transmission => transmission_commands(),
    };
    cmds.into_iter().map(|c| c.label).collect()
}

// ---------------------------------------------------------------------------
// Transition helpers — build the next state for a selected command
// ---------------------------------------------------------------------------

fn select_frequency_command(idx: usize) -> Option<ControlState> {
    let on_off = vec!["On".to_string(), "Off".to_string()];
    match idx {
        0 => Some(ControlState::TextInput {
            prompt: "Enter freq MHz (e.g. 14.195):".to_string(),
            buffer: String::new(),
            error: None,
            action: InputAction::SetVfoA,
        }),
        1 => Some(ControlState::TextInput {
            prompt: "Enter freq MHz (e.g. 14.195):".to_string(),
            buffer: String::new(),
            error: None,
            action: InputAction::SetVfoB,
        }),
        2 => Some(ControlState::ListSelect {
            options: on_off.clone(),
            cursor: 0,
            action: SelectAction::ToggleRit,
        }),
        3 => Some(ControlState::ListSelect {
            options: on_off.clone(),
            cursor: 0,
            action: SelectAction::ToggleXit,
        }),
        4 => Some(ControlState::ListSelect {
            options: on_off.clone(),
            cursor: 0,
            action: SelectAction::ToggleLock,
        }),
        5 => Some(ControlState::ListSelect {
            options: on_off.clone(),
            cursor: 0,
            action: SelectAction::ToggleFine,
        }),
        6 => Some(ControlState::ListSelect {
            options: on_off,
            cursor: 0,
            action: SelectAction::ToggleScan,
        }),
        _ => None,
    }
}

fn select_mode_command(idx: usize) -> Option<ControlState> {
    match idx {
        0 => Some(ControlState::ListSelect {
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
            cursor: 0,
            action: SelectAction::SetMode,
        }),
        1 => Some(ControlState::ListSelect {
            options: vec!["Off".to_string(), "NR1".to_string(), "NR2".to_string()],
            cursor: 0,
            action: SelectAction::SetNoiseReduction,
        }),
        2 => Some(ControlState::ListSelect {
            options: vec![
                "Off".to_string(),
                "Slow".to_string(),
                "Mid".to_string(),
                "Fast".to_string(),
            ],
            cursor: 0,
            action: SelectAction::SetAgc,
        }),
        3 => Some(ControlState::ListSelect {
            options: vec!["On".to_string(), "Off".to_string()],
            cursor: 0,
            action: SelectAction::ToggleNb,
        }),
        _ => None,
    }
}

fn select_receive_command(idx: usize) -> Option<ControlState> {
    match idx {
        0 => Some(ControlState::TextInput {
            prompt: "AF gain (0-255):".to_string(),
            buffer: String::new(),
            error: None,
            action: InputAction::SetAfGain,
        }),
        1 => Some(ControlState::TextInput {
            prompt: "RF gain (0-255):".to_string(),
            buffer: String::new(),
            error: None,
            action: InputAction::SetRfGain,
        }),
        2 => Some(ControlState::TextInput {
            prompt: "Squelch (0-100):".to_string(),
            buffer: String::new(),
            error: None,
            action: InputAction::SetSqLevel,
        }),
        3 => Some(ControlState::TextInput {
            prompt: "MIC gain (0-100):".to_string(),
            buffer: String::new(),
            error: None,
            action: InputAction::SetMicGain,
        }),
        _ => None,
    }
}

fn select_transmission_command(idx: usize) -> Option<ControlState> {
    let on_off = vec!["On".to_string(), "Off".to_string()];
    match idx {
        0 => Some(ControlState::TextInput {
            prompt: "TX power % (0-100):".to_string(),
            buffer: String::new(),
            error: None,
            action: InputAction::SetPower,
        }),
        1 => Some(ControlState::ListSelect {
            options: on_off.clone(),
            cursor: 0,
            action: SelectAction::TogglePreamp,
        }),
        2 => Some(ControlState::ListSelect {
            options: on_off.clone(),
            cursor: 0,
            action: SelectAction::ToggleAtt,
        }),
        3 => Some(ControlState::ListSelect {
            options: on_off.clone(),
            cursor: 0,
            action: SelectAction::ToggleVox,
        }),
        4 => Some(ControlState::TextInput {
            prompt: "VOX gain (0-100):".to_string(),
            buffer: String::new(),
            error: None,
            action: InputAction::SetVoxGain,
        }),
        5 => Some(ControlState::ListSelect {
            options: on_off,
            cursor: 0,
            action: SelectAction::ToggleProc,
        }),
        6 => Some(ControlState::ListSelect {
            options: vec!["ANT1".to_string(), "ANT2".to_string()],
            cursor: 0,
            action: SelectAction::SetAntenna,
        }),
        _ => None,
    }
}

fn select_group_command(group: CommandGroup, idx: usize) -> Option<ControlState> {
    match group {
        CommandGroup::Frequency => select_frequency_command(idx),
        CommandGroup::Mode => select_mode_command(idx),
        CommandGroup::Receive => select_receive_command(idx),
        CommandGroup::Transmission => select_transmission_command(idx),
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
                .map_err(|_| "Enter a number 0–255".to_string())?;
            if v > 255 {
                return Err("Value must be 0–255".to_string());
            }
            Ok(match action {
                InputAction::SetAfGain => ExecuteAction::SetAfGain(v as u8),
                _ => ExecuteAction::SetRfGain(v as u8),
            })
        }
        InputAction::SetSqLevel
        | InputAction::SetMicGain
        | InputAction::SetPower
        | InputAction::SetVoxGain => {
            let v: u16 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–100".to_string())?;
            if v > 100 {
                return Err("Value must be 0–100".to_string());
            }
            Ok(match action {
                InputAction::SetSqLevel => ExecuteAction::SetSqLevel(v as u8),
                InputAction::SetMicGain => ExecuteAction::SetMicGain(v as u8),
                InputAction::SetPower => ExecuteAction::SetPower(v as u8),
                _ => ExecuteAction::SetVoxGain(v as u8),
            })
        }
        InputAction::SetVoxDelay => {
            let v: u16 = buffer
                .parse()
                .map_err(|_| "Enter a number 0–9999".to_string())?;
            if v > 9999 {
                return Err("Value must be 0–9999".to_string());
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
        SelectAction::SetAgc => ExecuteAction::SetAgc(cursor as u8), // 0=Off,1=Slow,2=Mid,3=Fast
        SelectAction::SetNoiseReduction => ExecuteAction::SetNoiseReduction(cursor as u8), // 0=Off,1=NR1,2=NR2
        SelectAction::SetAntenna => ExecuteAction::SetAntenna(cursor as u8 + 1), // 1-indexed
        SelectAction::ToggleRit => ExecuteAction::ToggleRit(cursor == 0),
        SelectAction::ToggleXit => ExecuteAction::ToggleXit(cursor == 0),
        SelectAction::ToggleNb => ExecuteAction::ToggleNb(cursor == 0),
        SelectAction::TogglePreamp => ExecuteAction::TogglePreamp(cursor == 0),
        SelectAction::ToggleAtt => ExecuteAction::ToggleAtt(cursor == 0),
        SelectAction::ToggleVox => ExecuteAction::ToggleVox(cursor == 0),
        SelectAction::ToggleProc => ExecuteAction::ToggleProc(cursor == 0),
        SelectAction::ToggleScan => ExecuteAction::ToggleScan(cursor == 0),
        SelectAction::ToggleLock => ExecuteAction::ToggleLock(cursor == 0),
        SelectAction::ToggleFine => ExecuteAction::ToggleFine(cursor == 0),
    }
}

// ---------------------------------------------------------------------------
// handle_key — the main event handler
// ---------------------------------------------------------------------------

/// Process a key event and transition the control state.
///
/// Returns `KeyResult::Continue`, `KeyResult::Quit`, or `KeyResult::Execute`.
pub fn handle_key(key: KeyEvent, state: &mut ControlState) -> KeyResult {
    match state {
        ControlState::Menu => match key.code {
            KeyCode::Char('f') | KeyCode::Char('F') => {
                *state = ControlState::GroupMenu {
                    group: CommandGroup::Frequency,
                    cursor: 0,
                };
                KeyResult::Continue
            }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                *state = ControlState::GroupMenu {
                    group: CommandGroup::Mode,
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
                    group: CommandGroup::Transmission,
                    cursor: 0,
                };
                KeyResult::Continue
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => KeyResult::Quit,
            _ => KeyResult::Continue,
        },

        ControlState::GroupMenu { group, .. } => {
            let group = *group;
            match key.code {
                KeyCode::Char('1')
                | KeyCode::Char('2')
                | KeyCode::Char('3')
                | KeyCode::Char('4')
                | KeyCode::Char('5')
                | KeyCode::Char('6')
                | KeyCode::Char('7')
                | KeyCode::Char('8')
                | KeyCode::Char('9') => {
                    let digit = key.code;
                    if let KeyCode::Char(c) = digit {
                        let idx = (c as usize) - ('1' as usize);
                        if let Some(next) = select_group_command(group, idx) {
                            *state = next;
                        }
                    }
                    KeyResult::Continue
                }
                KeyCode::Esc => {
                    *state = ControlState::Menu;
                    KeyResult::Continue
                }
                _ => KeyResult::Continue,
            }
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
        let result = handle_key(key(KeyCode::Char('f')), &mut state);
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
    fn test_menu_q_quits() {
        let mut state = ControlState::Menu;
        let result = handle_key(key(KeyCode::Char('q')), &mut state);
        assert!(matches!(result, KeyResult::Quit));
    }

    #[test]
    fn test_group_menu_esc_returns_to_menu() {
        let mut state = ControlState::GroupMenu {
            group: CommandGroup::Mode,
            cursor: 0,
        };
        let result = handle_key(key(KeyCode::Esc), &mut state);
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }

    #[test]
    fn test_group_menu_1_transitions_to_text_input() {
        let mut state = ControlState::GroupMenu {
            group: CommandGroup::Frequency,
            cursor: 0,
        };
        handle_key(key(KeyCode::Char('1')), &mut state);
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
        let result = handle_key(key(KeyCode::Enter), &mut state);
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
        let result = handle_key(key(KeyCode::Enter), &mut state);
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
        let result = handle_key(key(KeyCode::Enter), &mut state);
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
        let result = handle_key(key(KeyCode::Enter), &mut state);
        assert!(matches!(result, KeyResult::Continue));
        assert!(matches!(state, ControlState::Menu));
    }
}
