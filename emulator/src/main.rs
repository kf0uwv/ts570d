use emulator::emulator::Emulator;
use emulator::port::{self, PortMode};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let tui = args.iter().any(|a| a == "--tui");

    // Determine port mode from --port argument.
    let mode = port::parse_port_arg(args.into_iter());

    // Open the port and print status.
    let (serial_port, slave_path_opt) = match port::open_port(&mode) {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("Failed to open port: {err}");
            std::process::exit(1);
        }
    };

    let slave_path = match (&mode, slave_path_opt) {
        (PortMode::Virtual, Some(ref path)) => {
            println!("PTY slave: {path}");
            path.clone()
        }
        (PortMode::Physical(ref path), None) => {
            println!("Connected to {path}");
            path.clone()
        }
        // Fallback (should not occur).
        (_, Some(path)) => path,
        (_, None) => String::new(),
    };

    let mut emu = Emulator::from_port(serial_port, slave_path);

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
