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

use emulator::acc2::Acc2Faults;
use emulator::emulator::Emulator;
use emulator::logger::BackgroundLogger;
use emulator::port::{self, PortMode};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let tui = args.iter().any(|a| a == "--tui");
    let background = args.iter().any(|a| a == "--background");

    // --tui and --background are mutually exclusive.
    if tui && background {
        eprintln!("Error: --tui and --background are mutually exclusive");
        std::process::exit(1);
    }

    // Parse optional --log-file <path> (only meaningful with --background).
    let log_file: Option<String> = {
        let mut lf = None;
        let mut it = args.iter().peekable();
        while let Some(arg) = it.next() {
            if arg == "--log-file" {
                lf = it.next().cloned();
            }
        }
        lf
    };

    // The virtual hardware this radio presents, and the band behind it.
    //
    // `--com` and `--acc2-audio` are the ACC2-IF's two connectors;
    // `--if-out` is the SMA on the radio's IF output -- the CN4 header on
    // a TS-570D. The flag names the signal rather than the connector,
    // matching the control program's own flag. All optional: an emulator with none of them
    // is still a radio with a CAT port, which is what most tests want.
    let mut tap_addr = None;
    let mut com_addr = None;
    let mut audio_addr = None;
    let mut faults = Acc2Faults::default();
    let mut tap_seed = 0x5713_0DEFu64;
    {
        let mut it = args.iter().peekable();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--if-out" => tap_addr = it.next().cloned(),
                "--com" => com_addr = it.next().cloned(),
                "--acc2-audio" => audio_addr = it.next().cloned(),
                "--acc2-fault" => match it.next().map(String::as_str) {
                    Some("pin13-bonded") => faults.pin13_bonded_to_braid = true,
                    Some("phantom-keying") => faults.phantom_keying = true,
                    Some(other) => {
                        eprintln!(
                            "Unknown --acc2-fault {other:?} (want pin13-bonded or phantom-keying)"
                        );
                        std::process::exit(2);
                    }
                    None => {
                        eprintln!("--acc2-fault needs a fault name");
                        std::process::exit(2);
                    }
                },
                "--seed" => {
                    if let Some(v) = it.next().and_then(|v| v.parse().ok()) {
                        tap_seed = v;
                    }
                }
                _ => {}
            }
        }
    }

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

    // Always print the PTY slave path as the FIRST line in KEY=VALUE format
    // for virtual mode, so scripts can parse it regardless of run mode.
    let slave_path = match (&mode, slave_path_opt) {
        (PortMode::Virtual, Some(ref path)) => {
            println!("PTY_SLAVE={path}");
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

    // Optional spoofed CN4 tap, presenting itself as an RTL-SDR over
    // rtl_tcp. This is hardware, not a service: the control program
    // connects to it exactly as it would to a real dongle.
    //
    // A fixed default seed rather than a random one. The band should look
    // the same every run unless somebody asks otherwise -- "the signal
    // that was here yesterday" is useful while debugging a console.
    // One band, shared by everything that renders it. The tap and the ACC2
    // receive-audio pin must describe the same radio, or a console's
    // waterfall and its AF display would disagree about what is on the air.
    let band = emulator::tap::band_for(tap_seed);

    // The S-meter follows that same band, whether or not anything is
    // watching the IF output: a radio's meter is a property of the radio,
    // not of somebody having a waterfall open. Without this the needle sat
    // at a constant 10 while the spectrum showed a band full of signals,
    // and a console could not be tested against it at all.
    emulator::meter::spawn(emu.radio(), band.clone());

    if let Some(addr) = tap_addr {
        match emulator::tap::serve_band(emu.radio(), &addr, band.clone()) {
            Ok(bound) => println!("IF_OUT={bound}"),
            Err(err) => {
                eprintln!("Failed to serve the IF output on {addr}: {err}");
                std::process::exit(1);
            }
        }
    }

    // The ACC2 socket. Faults are set before anything is plugged into it,
    // because SN-1's whole character is that the damage happens on seating.
    let acc2 = emu.acc2();
    acc2.lock().expect("acc2 lock").set_faults(faults);

    // The radio's COM port, as an RFC 2217 device server. A pseudo-terminal
    // carries no modem control lines at all, so this is the only endpoint
    // on which the DTR that keys this station's PTT exists.
    if let Some(addr) = com_addr {
        match emulator::com::serve(emu.radio(), acc2.clone(), &addr) {
            Ok(bound) => println!("COM_PORT={bound}"),
            Err(err) => {
                eprintln!("Failed to serve the radio's COM port on {addr}: {err}");
                std::process::exit(1);
            }
        }
    }

    // ACC2 pins 3 and 11 -- the pair that goes to the sound device.
    if let Some(addr) = audio_addr {
        match emulator::acc2_audio::serve(emu.radio(), band, &addr) {
            Ok(bound) => println!("ACC2_AUDIO={bound}"),
            Err(err) => {
                eprintln!("Failed to serve the ACC2 audio pair on {addr}: {err}");
                std::process::exit(1);
            }
        }
    }

    // Seat the plug once its connectors are being served. Under SN-1 this
    // is the moment a bonded pin 13 keys the radio -- which is the point of
    // being able to reproduce it.
    let seated = acc2.lock().expect("acc2 lock").seat(true);
    if let Some(keying) = seated {
        emulator::com::apply_keying(&emu.radio(), keying);
    }

    // Set up Ctrl-C handler for graceful shutdown.
    ctrlc::set_handler(|| {
        // Restore terminal in case --tui is active.
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
        println!("\nEmulator shutting down.");
        std::process::exit(0);
    })
    .expect("Error setting Ctrl-C handler");

    let result = if tui {
        emu.run_with_tui()
    } else if background {
        // Build the logger: file if --log-file was given, otherwise stdout.
        let logger = if let Some(ref path) = log_file {
            match BackgroundLogger::file(path) {
                Ok(l) => l,
                Err(err) => {
                    eprintln!("Failed to open log file '{path}': {err}");
                    std::process::exit(1);
                }
            }
        } else {
            BackgroundLogger::stdout()
        };
        emu.run_background(logger)
    } else {
        emu.run()
    };

    if let Err(err) = result {
        eprintln!("Emulator error: {err}");
        std::process::exit(1);
    }
}
