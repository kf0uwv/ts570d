use emulator::emulator::Emulator;

fn main() {
    let tui = std::env::args().any(|a| a == "--tui");

    let mut emu = match Emulator::new() {
        Ok(e) => e,
        Err(err) => {
            eprintln!("Failed to start emulator: {err}");
            std::process::exit(1);
        }
    };

    // Set up Ctrl-C handler for graceful shutdown.
    ctrlc::set_handler(|| {
        // Restore terminal in case --tui is active.
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::LeaveAlternateScreen
        );
        println!("\nEmulator shutting down.");
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    let result = if tui {
        emu.run_with_tui()
    } else {
        emu.run()
    };

    if let Err(err) = result {
        eprintln!("Emulator error: {err}");
        std::process::exit(1);
    }
}
