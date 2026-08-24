//! `ts570d-line` -- bench helper for the serial modem-control lines.
//!
//! Drives DTR or RTS on a serial port, and reads back CTS/DSR/DCD, without
//! any CAT traffic. Built for troubleshooting a DTR-keyed PTT interface
//! (e.g. an ACC2 opto coupler whose LED hangs off the DTR pin): `hold`
//! asserts the line until Enter is pressed so the operator can watch the
//! radio/DMM, `pulse` keys it for a fixed number of milliseconds, and
//! `status` reports the input lines.
//!
//! Persistent "set and exit" is deliberately not offered: on Linux the tty
//! layer (HUPCL, left at the driver default by `configure_termios`) may drop
//! DTR/RTS when the last close happens, so a state set by an exiting process
//! is not trustworthy. `hold` is the honest primitive.
//!
//! Usage:
//!   ts570d-line <port> status
//!   ts570d-line <port> dtr|rts hold
//!   ts570d-line <port> dtr|rts pulse <ms>
//!
//! Example (Linux):   ts570d-line /dev/ttyUSB0 dtr hold
//! Example (Windows): ts570d-line COM3 dtr pulse 500
//!
//! Like every `SerialPort::open` caller, on Linux this must run inside an
//! active monoio runtime (the fd is registered with io_uring at open); on
//! Windows open is a plain synchronous call, so `main` is synchronous there
//! -- nothing here ever awaits.

use cat_transport_core::ModemControlLines;
use cat_transport_serial::{FlowControl, Parity, SerialConfig, SerialPort};

/// Which output line the action drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Line {
    Dtr,
    Rts,
}

impl Line {
    fn name(self) -> &'static str {
        match self {
            Line::Dtr => "DTR",
            Line::Rts => "RTS",
        }
    }
}

/// What to do once the port is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Assert the line, wait for Enter, deassert.
    Hold(Line),
    /// Assert the line for N milliseconds, then deassert.
    Pulse(Line, u64),
    /// Print CTS/DSR/DCD and exit.
    Status,
}

const USAGE: &str = "Usage:\n  \
     ts570d-line <port> status\n  \
     ts570d-line <port> dtr|rts hold\n  \
     ts570d-line <port> dtr|rts pulse <ms>";

/// Parse `argv` (program name already stripped). Pure so it can be unit
/// tested on every platform.
fn parse_args(argv: &[String]) -> Result<(String, Action), String> {
    let port = argv.first().ok_or_else(|| USAGE.to_string())?.clone();
    let second = argv.get(1).ok_or_else(|| USAGE.to_string())?;

    if second == "status" {
        if argv.len() > 2 {
            return Err(USAGE.to_string());
        }
        return Ok((port, Action::Status));
    }

    let line = match second.as_str() {
        "dtr" => Line::Dtr,
        "rts" => Line::Rts,
        other => return Err(format!("unknown line {other:?} (want dtr or rts)\n{USAGE}")),
    };

    match argv.get(2).map(String::as_str) {
        Some("hold") if argv.len() == 3 => Ok((port, Action::Hold(line))),
        Some("pulse") => {
            let ms = argv
                .get(3)
                .ok_or_else(|| format!("pulse needs a duration in ms\n{USAGE}"))?;
            let ms: u64 = ms
                .parse()
                .map_err(|_| format!("pulse duration must be a number, got {ms:?}\n{USAGE}"))?;
            if argv.len() > 4 {
                return Err(USAGE.to_string());
            }
            Ok((port, Action::Pulse(line, ms)))
        }
        _ => Err(USAGE.to_string()),
    }
}

/// Open `port` with everything deasserted and no CAT assumptions: this tool
/// manages the lines itself and never exchanges data, mirroring pin-test's
/// known-clear baseline.
fn open_quiet(port: &str) -> Result<SerialPort, String> {
    SerialPort::open(
        port,
        SerialConfig {
            baud_rate: 9600,
            data_bits: 8,
            stop_bits: 2,
            parity: Parity::None,
            flow_control: FlowControl::None,
            initial_rts: false,
            initial_dtr: false,
        },
    )
    .map_err(|e| format!("could not open {port}: {e}"))
}

fn set_line(port: &SerialPort, line: Line, asserted: bool) -> Result<(), String> {
    let r = match line {
        Line::Dtr => port.set_dtr(asserted),
        Line::Rts => port.set_rts(asserted),
    };
    r.map_err(|e| format!("set_{} failed: {e}", line.name().to_lowercase()))
}

fn print_status(port: &SerialPort) {
    // Input lines are best-effort: a USB adapter that doesn't wire one
    // reports an error, which is itself useful bench information.
    for (name, read) in [
        ("CTS", port.read_cts()),
        ("DSR", port.read_dsr()),
        ("DCD", port.read_dcd()),
    ] {
        match read {
            Ok(v) => println!("{name}: {}", if v { "asserted" } else { "deasserted" }),
            Err(e) => println!("{name}: unreadable ({e})"),
        }
    }
}

fn run(port_path: &str, action: Action) -> i32 {
    let port = match open_quiet(port_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };

    let result = match action {
        Action::Status => {
            print_status(&port);
            Ok(())
        }
        Action::Pulse(line, ms) => set_line(&port, line, true).and_then(|()| {
            println!("{} asserted for {ms} ms", line.name());
            std::thread::sleep(std::time::Duration::from_millis(ms));
            // `.map` not `.inspect`: Result::inspect is 1.76+, MSRV is 1.75.
            set_line(&port, line, false).map(|()| println!("{} released", line.name()))
        }),
        Action::Hold(line) => set_line(&port, line, true).and_then(|()| {
            println!("{} asserted -- press Enter to release", line.name());
            print_status(&port);
            let mut buf = String::new();
            let _ = std::io::stdin().read_line(&mut buf);
            // `.map` not `.inspect`: Result::inspect is 1.76+, MSRV is 1.75.
            set_line(&port, line, false).map(|()| println!("{} released", line.name()))
        }),
    };

    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn real_main() -> i32 {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&argv) {
        Ok((port, action)) => run(&port, action),
        Err(msg) => {
            eprintln!("{msg}");
            2
        }
    }
}

/// On Linux `SerialPort::open` registers the fd with io_uring, so the whole
/// (entirely synchronous) body runs inside a monoio runtime.
#[cfg(target_os = "linux")]
#[monoio::main]
async fn main() {
    std::process::exit(real_main());
}

#[cfg(not(target_os = "linux"))]
fn main() {
    std::process::exit(real_main());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_status() {
        let (port, action) = parse_args(&args(&["/dev/ttyUSB0", "status"])).unwrap();
        assert_eq!(port, "/dev/ttyUSB0");
        assert_eq!(action, Action::Status);
    }

    #[test]
    fn parses_hold_and_pulse_for_both_lines() {
        assert_eq!(
            parse_args(&args(&["COM3", "dtr", "hold"])).unwrap().1,
            Action::Hold(Line::Dtr)
        );
        assert_eq!(
            parse_args(&args(&["COM3", "rts", "hold"])).unwrap().1,
            Action::Hold(Line::Rts)
        );
        assert_eq!(
            parse_args(&args(&["COM3", "dtr", "pulse", "500"]))
                .unwrap()
                .1,
            Action::Pulse(Line::Dtr, 500)
        );
    }

    #[test]
    fn rejects_bad_input() {
        // No args, unknown line, missing/garbage pulse duration, trailing junk.
        assert!(parse_args(&args(&[])).is_err());
        assert!(parse_args(&args(&["/dev/ttyUSB0"])).is_err());
        assert!(parse_args(&args(&["/dev/ttyUSB0", "dcd", "hold"])).is_err());
        assert!(parse_args(&args(&["/dev/ttyUSB0", "dtr", "pulse"])).is_err());
        assert!(parse_args(&args(&["/dev/ttyUSB0", "dtr", "pulse", "abc"])).is_err());
        assert!(parse_args(&args(&["/dev/ttyUSB0", "dtr", "hold", "x"])).is_err());
        assert!(parse_args(&args(&["/dev/ttyUSB0", "status", "x"])).is_err());
    }
}
