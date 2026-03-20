//! TS-570D Radio Control Application
//! 
//! Main entry point that coordinates all workspace components using the framework.

use ts570d::framework::{ApplicationStateMachine, State, FrameworkResult};
use tracing::{info, warn, error};

#[monoio::main]
async fn main() -> FrameworkResult<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    info!("Starting TS-570D Radio Control Application");

    // Create and initialize application state machine
    let mut state_machine = ApplicationStateMachine::new();
    state_machine.initialize()?;

    // Start the application
    state_machine.start()?;
    info!("Application state: {}", state_machine.machine().get_state()?.as_string());

    // TODO: Initialize and start all workspace components
    // This will be implemented by other agents:
    // - Serial communication (@serial agent)
    // - Radio protocol (@kenwood agent) 
    // - User interface (@ui agent)
    // - Emulator (@ui agent with emulator focus)

    // Main application loop
    run_application_loop(&state_machine).await?;

    // Stop the application
    state_machine.stop()?;
    info!("Application stopped successfully");

    Ok(())
}

/// Main application coordination loop
async fn run_application_loop(state_machine: &ApplicationStateMachine) -> FrameworkResult<()> {
    info!("Entering main application loop");

    loop {
        // Check current state
        let current_state = state_machine.machine().get_state()?;
        
        match current_state {
            State::Running => {
                // TODO: Process messages and coordinate components
                // This will handle inter-crate communication via framework
                monoio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            State::Paused => {
                info!("Application paused - waiting for resume");
                monoio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            State::Stopping => {
                info!("Application stopping - breaking loop");
                break;
            }
            State::Error(ref msg) => {
                error!("Application in error state: {}", msg);
                break;
            }
            _ => {
                warn!("Unexpected state in main loop: {:?}", current_state);
                break;
            }
        }

        // Update state machine
        if let Err(e) = state_machine.machine().update() {
            error!("State machine update error: {:?}", e);
            break;
        }
    }

    Ok(())
}
