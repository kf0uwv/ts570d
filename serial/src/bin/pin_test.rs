//! RS-232 cable pin tester
//!
//! Tests each signal pin on a null-modem cable between two serial ports.
//!
//! Usage:
//!   cargo run --bin pin-test -- <source-port> <dest-port>
//!
//! Example:
//!   cargo run --bin pin-test -- /dev/ttyUSB0 /dev/ttyUSB1

use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::thread;
use std::time::Duration;

use thiserror::Error;

// ── Error type ───────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
enum PinTestError {
    #[error("failed to open {path}: {source}")]
    OpenFailed {
        path: String,
        source: std::io::Error,
    },

    #[error("termios config failed for {path}: {msg}")]
    Termios { path: String, msg: String },

    #[error("fcntl failed for {path}: {msg}")]
    Fcntl { path: String, msg: String },

    #[error("ioctl failed: {0}")]
    Ioctl(String),
}

// ── Test result ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Pass,
    Fail(String),
    Warn(String),
}

struct TestResult {
    label: &'static str,
    outcome: Outcome,
}

// ── Port open + configure ─────────────────────────────────────────────────────

/// Open a serial port with O_RDWR | O_NOCTTY | O_NONBLOCK, configure raw termios
/// (9600 baud, 8N2, no flow control), then switch to blocking with VMIN=0, VTIME=5.
fn open_port(path: &str) -> Result<std::fs::File, PinTestError> {
    use libc::{
        B9600, CS8, CSTOPB, F_GETFL, F_SETFL, HUPCL, O_NONBLOCK, PARENB, TCSANOW, VMIN, VTIME,
    };

    // Open the device
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_RDWR | libc::O_NOCTTY | O_NONBLOCK)
        .open(path)
        .map_err(|e| PinTestError::OpenFailed {
            path: path.to_string(),
            source: e,
        })?;

    let fd = file.as_raw_fd();

    // ── Configure termios ────────────────────────────────────────────────────
    // SAFETY: fd is a valid open file descriptor obtained from the file above.
    let mut tios: libc::termios = unsafe { std::mem::zeroed() };

    let rc = unsafe { libc::tcgetattr(fd, &mut tios) };
    if rc != 0 {
        return Err(PinTestError::Termios {
            path: path.to_string(),
            msg: format!("tcgetattr failed: errno={}", errno()),
        });
    }

    // Apply raw mode (clears all special processing, echo, signals, etc.)
    // SAFETY: &mut tios is a valid pointer to a termios struct.
    unsafe { libc::cfmakeraw(&mut tios) };

    // Clear character size bits, then set CS8
    tios.c_cflag &= !(libc::CSIZE as libc::tcflag_t);
    tios.c_cflag |= CS8 as libc::tcflag_t;

    // 2 stop bits (CSTOPB)
    tios.c_cflag |= CSTOPB as libc::tcflag_t;

    // No parity (clear PARENB)
    tios.c_cflag &= !(PARENB as libc::tcflag_t);

    // No hardware flow control (clear CRTSCTS)
    tios.c_cflag &= !libc::CRTSCTS;

    // No software flow control
    tios.c_iflag &= !(libc::IXON | libc::IXOFF | libc::IXANY);

    // Enable receiver, ignore modem control lines for local loopback
    tios.c_cflag |= libc::CREAD | libc::CLOCAL;

    // Clear HUPCL so we don't drop DTR/RTS on close
    tios.c_cflag &= !(HUPCL as libc::tcflag_t);

    // VMIN=0, VTIME=5: read returns after up to 500 ms even with no data
    tios.c_cc[VMIN] = 0;
    tios.c_cc[VTIME] = 5;

    // Set baud rate to 9600
    // SAFETY: cfsetispeed/cfsetospeed operate on the termios struct pointer.
    let rc = unsafe { libc::cfsetispeed(&mut tios, B9600 as libc::speed_t) };
    if rc != 0 {
        return Err(PinTestError::Termios {
            path: path.to_string(),
            msg: format!("cfsetispeed failed: errno={}", errno()),
        });
    }
    let rc = unsafe { libc::cfsetospeed(&mut tios, B9600 as libc::speed_t) };
    if rc != 0 {
        return Err(PinTestError::Termios {
            path: path.to_string(),
            msg: format!("cfsetospeed failed: errno={}", errno()),
        });
    }

    // Apply termios settings immediately
    let rc = unsafe { libc::tcsetattr(fd, TCSANOW, &tios) };
    if rc != 0 {
        return Err(PinTestError::Termios {
            path: path.to_string(),
            msg: format!("tcsetattr failed: errno={}", errno()),
        });
    }

    // ── Switch to blocking mode ───────────────────────────────────────────────
    // Get current flags, then clear O_NONBLOCK.
    // SAFETY: fd is valid.
    let flags = unsafe { libc::fcntl(fd, F_GETFL, 0) };
    if flags == -1 {
        return Err(PinTestError::Fcntl {
            path: path.to_string(),
            msg: format!("F_GETFL failed: errno={}", errno()),
        });
    }
    let rc = unsafe { libc::fcntl(fd, F_SETFL, flags & !O_NONBLOCK) };
    if rc == -1 {
        return Err(PinTestError::Fcntl {
            path: path.to_string(),
            msg: format!("F_SETFL failed: errno={}", errno()),
        });
    }

    // Flush any stale data in the port buffers before testing.
    // SAFETY: fd is valid.
    unsafe { libc::tcflush(fd, libc::TCIOFLUSH) };

    Ok(file)
}

// ── Modem-control helpers ─────────────────────────────────────────────────────

fn tiocmbis(fd: std::os::unix::io::RawFd, bits: libc::c_int) -> Result<(), PinTestError> {
    // SAFETY: fd is valid, &bits is a valid pointer to the modem bits integer.
    let rc = unsafe { libc::ioctl(fd, libc::TIOCMBIS, &bits) };
    if rc == -1 {
        return Err(PinTestError::Ioctl(format!(
            "TIOCMBIS bits=0x{:x} errno={}",
            bits,
            errno()
        )));
    }
    Ok(())
}

fn tiocmbic(fd: std::os::unix::io::RawFd, bits: libc::c_int) -> Result<(), PinTestError> {
    // SAFETY: fd is valid.
    let rc = unsafe { libc::ioctl(fd, libc::TIOCMBIC, &bits) };
    if rc == -1 {
        return Err(PinTestError::Ioctl(format!(
            "TIOCMBIC bits=0x{:x} errno={}",
            bits,
            errno()
        )));
    }
    Ok(())
}

fn tiocmget(fd: std::os::unix::io::RawFd) -> Result<libc::c_int, PinTestError> {
    let mut bits: libc::c_int = 0;
    // SAFETY: fd is valid, &mut bits is a valid pointer.
    let rc = unsafe { libc::ioctl(fd, libc::TIOCMGET, &mut bits) };
    if rc == -1 {
        return Err(PinTestError::Ioctl(format!("TIOCMGET errno={}", errno())));
    }
    Ok(bits)
}

fn sleep_ms(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

fn errno() -> i32 {
    // SAFETY: errno is always set by the OS; reading it is safe.
    unsafe { *libc::__errno_location() }
}

// ── TXD→RXD test ─────────────────────────────────────────────────────────────

fn test_txd_rxd(
    label: &'static str,
    tx_fd: std::os::unix::io::RawFd,
    rx_fd: std::os::unix::io::RawFd,
) -> TestResult {
    use std::io::{Read, Write};

    // Flush RX before test
    unsafe { libc::tcflush(rx_fd, libc::TCIFLUSH) };

    let to_send = [0xAAu8, 0x55u8];

    // Write via the raw fd wrapped in a ManuallyDrop File (no double-close)
    let mut tx_file = unsafe { std::mem::ManuallyDrop::new(std::fs::File::from_raw_fd(tx_fd)) };
    let mut rx_file = unsafe { std::mem::ManuallyDrop::new(std::fs::File::from_raw_fd(rx_fd)) };

    if let Err(e) = tx_file.write_all(&to_send) {
        return TestResult {
            label,
            outcome: Outcome::Fail(format!("write failed: {}", e)),
        };
    }

    let mut buf = [0u8; 2];
    let outcome = match rx_file.read_exact(&mut buf) {
        Ok(()) if buf == to_send => Outcome::Pass,
        Ok(()) => Outcome::Fail(format!(
            "data mismatch: sent {:02X?}, got {:02X?}",
            to_send, buf
        )),
        Err(e) => Outcome::Fail(format!("read failed (timeout or error): {}", e)),
    };

    TestResult { label, outcome }
}

/// Re-export from_raw_fd for use in ManuallyDrop pattern above.
use std::os::unix::io::FromRawFd;

// ── Modem-control test (FAIL on error) ────────────────────────────────────────

fn test_modem_bit(
    label: &'static str,
    assert_fd: std::os::unix::io::RawFd,
    read_fd: std::os::unix::io::RawFd,
    assert_bit: libc::c_int, // bit to drive on assert_fd (e.g. TIOCM_RTS)
    check_bit: libc::c_int,  // bit to read on read_fd   (e.g. TIOCM_CTS)
    warn_only: bool,
) -> TestResult {
    let make_outcome = |msg: String| -> Outcome {
        if warn_only {
            Outcome::Warn(msg)
        } else {
            Outcome::Fail(msg)
        }
    };

    // 1. Clear the assert bit to establish a known baseline.
    if let Err(e) = tiocmbic(assert_fd, assert_bit) {
        return TestResult {
            label,
            outcome: make_outcome(format!("TIOCMBIC (clear) failed: {}", e)),
        };
    }
    sleep_ms(50);

    // 2. Verify baseline: check_bit should be clear on read_fd.
    let bits = match tiocmget(read_fd) {
        Ok(b) => b,
        Err(e) => {
            return TestResult {
                label,
                outcome: make_outcome(format!("TIOCMGET (baseline) failed: {}", e)),
            }
        }
    };
    if bits & check_bit != 0 {
        return TestResult {
            label,
            outcome: make_outcome(format!(
                "baseline check failed: bit 0x{:x} already set (bits=0x{:x})",
                check_bit, bits
            )),
        };
    }

    // 3. Assert the bit.
    if let Err(e) = tiocmbis(assert_fd, assert_bit) {
        return TestResult {
            label,
            outcome: make_outcome(format!("TIOCMBIS (set) failed: {}", e)),
        };
    }
    sleep_ms(50);

    // 4. Verify set: check_bit should now be set on read_fd.
    let bits = match tiocmget(read_fd) {
        Ok(b) => b,
        Err(e) => {
            return TestResult {
                label,
                outcome: make_outcome(format!("TIOCMGET (after set) failed: {}", e)),
            }
        }
    };
    if bits & check_bit == 0 {
        let suffix = if warn_only {
            " — DTR not required by TS-570D"
        } else {
            ""
        };
        return TestResult {
            label,
            outcome: make_outcome(format!(
                "no response — bit 0x{:x} not set after asserting (bits=0x{:x}){}",
                check_bit, bits, suffix
            )),
        };
    }

    // 5. Clear the bit again and verify cleared.
    if let Err(e) = tiocmbic(assert_fd, assert_bit) {
        return TestResult {
            label,
            outcome: make_outcome(format!("TIOCMBIC (clear after test) failed: {}", e)),
        };
    }
    sleep_ms(50);

    let bits = match tiocmget(read_fd) {
        Ok(b) => b,
        Err(e) => {
            return TestResult {
                label,
                outcome: make_outcome(format!("TIOCMGET (after clear) failed: {}", e)),
            }
        }
    };
    if bits & check_bit != 0 {
        return TestResult {
            label,
            outcome: make_outcome(format!(
                "bit 0x{:x} still set after TIOCMBIC (bits=0x{:x})",
                check_bit, bits
            )),
        };
    }

    TestResult {
        label,
        outcome: Outcome::Pass,
    }
}

// ── Print helpers ─────────────────────────────────────────────────────────────

fn print_result(r: &TestResult) {
    const LABEL_WIDTH: usize = 22;
    let label = r.label;
    match &r.outcome {
        Outcome::Pass => {
            println!("  {:<width$} ... PASS", label, width = LABEL_WIDTH);
        }
        Outcome::Fail(msg) => {
            println!("  {:<width$} ... FAIL", label, width = LABEL_WIDTH);
            println!("      detail: {}", msg);
        }
        Outcome::Warn(msg) => {
            println!(
                "  {:<width$} ... WARN  ({})",
                label,
                msg,
                width = LABEL_WIDTH
            );
        }
    }
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <source-port> <dest-port>", args[0]);
        eprintln!("Example: {} /dev/ttyUSB0 /dev/ttyUSB1", args[0]);
        std::process::exit(1);
    }

    let port_a = &args[1];
    let port_b = &args[2];

    println!("Pin test: {} → {}", port_a, port_b);
    println!();

    // Open both ports
    let file_a = match open_port(port_a) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };
    let file_b = match open_port(port_b) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };

    let fd_a = file_a.as_raw_fd();
    let fd_b = file_b.as_raw_fd();

    // TIOCM_RTS and TIOCM_DTR may not be in libc directly on all versions;
    // use the standard Linux values.
    const TIOCM_RTS: libc::c_int = 0x004;
    const TIOCM_CTS: libc::c_int = 0x020;
    const TIOCM_DTR: libc::c_int = 0x002;
    const TIOCM_DSR: libc::c_int = 0x100;
    const TIOCM_CAR: libc::c_int = 0x040; // DCD / Carrier Detect

    // Run the 7 tests
    let results: Vec<TestResult> = vec![
        test_txd_rxd("TXD→RXD (A→B)", fd_a, fd_b),
        test_txd_rxd("TXD→RXD (B→A)", fd_b, fd_a),
        test_modem_bit("RTS→CTS (A→B)", fd_a, fd_b, TIOCM_RTS, TIOCM_CTS, false),
        test_modem_bit("RTS→CTS (B→A)", fd_b, fd_a, TIOCM_RTS, TIOCM_CTS, false),
        test_modem_bit("DTR→DSR (A→B)", fd_a, fd_b, TIOCM_DTR, TIOCM_DSR, true),
        test_modem_bit("DTR→DSR (B→A)", fd_b, fd_a, TIOCM_DTR, TIOCM_DSR, true),
        test_modem_bit("DTR→DCD (A→B)", fd_a, fd_b, TIOCM_DTR, TIOCM_CAR, true),
    ];

    // Print results
    for r in &results {
        print_result(r);
    }

    // Summary
    let passed = results
        .iter()
        .filter(|r| r.outcome == Outcome::Pass)
        .count();
    let failed = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Fail(_)))
        .count();
    let warned = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Warn(_)))
        .count();

    println!();
    println!(
        "Result: {} passed, {} failed, {} warnings",
        passed, failed, warned
    );

    if failed > 0 {
        std::process::exit(1);
    }
}
