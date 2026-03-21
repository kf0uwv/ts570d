use emulator::emulator::Emulator;

fn main() {
    let mut emu = match Emulator::new() {
        Ok(e) => e,
        Err(err) => {
            eprintln!("Failed to start emulator: {err}");
            std::process::exit(1);
        }
    };

    // Set up Ctrl-C handler for graceful shutdown.
    ctrlc::set_handler(|| {
        println!("\nEmulator shutting down.");
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    if let Err(err) = emu.run() {
        eprintln!("Emulator error: {err}");
        std::process::exit(1);
    }
}
