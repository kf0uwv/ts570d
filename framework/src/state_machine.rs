//! State Machine Framework
//!
//! Provides application state management and state transitions.

use thiserror::Error;

use crate::errors::FrameworkResult;
use crate::errors::FrameworkError;

/// Errors specific to state machine operations
#[derive(Error, Debug)]
pub enum StateMachineError {
    /// Attempted an invalid state transition
    #[error("Invalid state transition from {from:?} to {to:?}")]
    InvalidTransition { from: String, to: String },

    /// State machine has not been initialized
    #[error("State machine not initialized")]
    NotInitialized,
}

/// Application states
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// Initial state before initialization
    Uninitialized,
    /// Initialized but not yet running
    Initialized,
    /// Actively running
    Running,
    /// Temporarily paused
    Paused,
    /// In the process of stopping
    Stopping,
    /// Stopped cleanly
    Stopped,
    /// Error state with a description
    Error(String),
}

impl State {
    /// Return a human-readable string representation
    pub fn as_string(&self) -> String {
        match self {
            State::Uninitialized => "Uninitialized".to_string(),
            State::Initialized => "Initialized".to_string(),
            State::Running => "Running".to_string(),
            State::Paused => "Paused".to_string(),
            State::Stopping => "Stopping".to_string(),
            State::Stopped => "Stopped".to_string(),
            State::Error(msg) => format!("Error({})", msg),
        }
    }
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_string())
    }
}

/// Inner state machine that tracks and transitions the application state
pub struct StateMachine {
    state: State,
}

impl StateMachine {
    fn new() -> Self {
        Self {
            state: State::Uninitialized,
        }
    }

    /// Return the current application state
    pub fn get_state(&self) -> FrameworkResult<State> {
        Ok(self.state.clone())
    }

    /// Perform any periodic update work (currently a no-op placeholder)
    pub fn update(&self) -> FrameworkResult<()> {
        Ok(())
    }

    fn set_state(&mut self, new_state: State) {
        self.state = new_state;
    }
}

/// High-level application state machine
///
/// Wraps [`StateMachine`] and exposes lifecycle methods that drive state
/// transitions in a well-defined order:
///
/// ```text
/// Uninitialized -> Initialized -> Running -> Stopping -> Stopped
/// ```
pub struct ApplicationStateMachine {
    machine: StateMachine,
}

impl ApplicationStateMachine {
    /// Create a new application state machine in the `Uninitialized` state.
    pub fn new() -> Self {
        Self {
            machine: StateMachine::new(),
        }
    }

    /// Transition from `Uninitialized` to `Initialized`.
    pub fn initialize(&mut self) -> FrameworkResult<()> {
        match self.machine.state {
            State::Uninitialized => {
                self.machine.set_state(State::Initialized);
                Ok(())
            }
            ref s => Err(FrameworkError::InvalidMessage(format!(
                "Cannot initialize from state: {}",
                s.as_string()
            ))),
        }
    }

    /// Transition from `Initialized` (or `Paused`) to `Running`.
    pub fn start(&mut self) -> FrameworkResult<()> {
        match self.machine.state {
            State::Initialized | State::Paused => {
                self.machine.set_state(State::Running);
                Ok(())
            }
            ref s => Err(FrameworkError::InvalidMessage(format!(
                "Cannot start from state: {}",
                s.as_string()
            ))),
        }
    }

    /// Pause a running application (transition `Running` -> `Paused`).
    pub fn pause(&mut self) -> FrameworkResult<()> {
        match self.machine.state {
            State::Running => {
                self.machine.set_state(State::Paused);
                Ok(())
            }
            ref s => Err(FrameworkError::InvalidMessage(format!(
                "Cannot pause from state: {}",
                s.as_string()
            ))),
        }
    }

    /// Begin shutdown (transition `Running` | `Paused` -> `Stopping`).
    pub fn stop(&mut self) -> FrameworkResult<()> {
        match self.machine.state {
            State::Running | State::Paused | State::Initialized => {
                self.machine.set_state(State::Stopping);
                Ok(())
            }
            ref s => Err(FrameworkError::InvalidMessage(format!(
                "Cannot stop from state: {}",
                s.as_string()
            ))),
        }
    }

    /// Provide access to the inner [`StateMachine`] for querying state and
    /// calling `update()`.
    pub fn machine(&self) -> &StateMachine {
        &self.machine
    }
}

impl Default for ApplicationStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state_is_uninitialized() {
        let asm = ApplicationStateMachine::new();
        assert_eq!(asm.machine().get_state().unwrap(), State::Uninitialized);
    }

    #[test]
    fn test_initialize_transitions_to_initialized() {
        let mut asm = ApplicationStateMachine::new();
        asm.initialize().unwrap();
        assert_eq!(asm.machine().get_state().unwrap(), State::Initialized);
    }

    #[test]
    fn test_start_transitions_to_running() {
        let mut asm = ApplicationStateMachine::new();
        asm.initialize().unwrap();
        asm.start().unwrap();
        assert_eq!(asm.machine().get_state().unwrap(), State::Running);
    }

    #[test]
    fn test_stop_transitions_to_stopping() {
        let mut asm = ApplicationStateMachine::new();
        asm.initialize().unwrap();
        asm.start().unwrap();
        asm.stop().unwrap();
        assert_eq!(asm.machine().get_state().unwrap(), State::Stopping);
    }

    #[test]
    fn test_pause_and_resume() {
        let mut asm = ApplicationStateMachine::new();
        asm.initialize().unwrap();
        asm.start().unwrap();
        asm.pause().unwrap();
        assert_eq!(asm.machine().get_state().unwrap(), State::Paused);
        asm.start().unwrap();
        assert_eq!(asm.machine().get_state().unwrap(), State::Running);
    }

    #[test]
    fn test_invalid_transition_returns_error() {
        let mut asm = ApplicationStateMachine::new();
        // Cannot start before initializing
        assert!(asm.start().is_err());
    }

    #[test]
    fn test_state_as_string() {
        assert_eq!(State::Running.as_string(), "Running");
        assert_eq!(State::Error("oops".to_string()).as_string(), "Error(oops)");
    }

    #[test]
    fn test_update_is_noop() {
        let asm = ApplicationStateMachine::new();
        assert!(asm.machine().update().is_ok());
    }
}
