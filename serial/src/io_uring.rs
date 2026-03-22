//! io_uring-based serial communication implementation
//!
//! This module provides high-performance asynchronous serial communication
//! using Linux io_uring interface for zero-copy operations via monoio.

use std::fmt;
use std::io::ErrorKind;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream as StdUnixStream;

use async_trait::async_trait;
use monoio::buf::VecBuf;
use monoio::io::{AsyncReadRent, AsyncWriteRent};
use monoio::net::UnixStream;
use nix::sys::termios::{
    cfmakeraw, cfsetispeed, cfsetospeed, tcgetattr, tcsetattr, tcdrain, BaudRate, ControlFlags,
    InputFlags, SetArg,
};

use framework::errors::TransportError;
use framework::transport::Transport;

use crate::SerialError;

/// Serial port configuration
#[derive(Debug, Clone)]
pub struct SerialConfig {
    pub baud_rate: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: Parity,
    pub flow_control: FlowControl,
}

#[derive(Debug, Clone)]
pub enum Parity {
    None,
    Even,
    Odd,
}

#[derive(Debug, Clone)]
pub enum FlowControl {
    None,
    Software,
    Hardware,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            baud_rate: 9600,
            data_bits: 8,
            stop_bits: 1,
            parity: Parity::None,
            flow_control: FlowControl::None,
        }
    }
}

/// Map a u32 baud rate to the nix `BaudRate` enum.
fn baud_rate_from_u32(baud: u32) -> Result<BaudRate, SerialError> {
    match baud {
        1200 => Ok(BaudRate::B1200),
        2400 => Ok(BaudRate::B2400),
        4800 => Ok(BaudRate::B4800),
        9600 => Ok(BaudRate::B9600),
        19200 => Ok(BaudRate::B19200),
        38400 => Ok(BaudRate::B38400),
        57600 => Ok(BaudRate::B57600),
        115200 => Ok(BaudRate::B115200),
        230400 => Ok(BaudRate::B230400),
        other => Err(SerialError::InvalidConfig(format!(
            "Unsupported baud rate: {}",
            other
        ))),
    }
}

/// Configure a file descriptor for raw serial communication according to `config`.
///
/// Applies: raw mode (cfmakeraw), data bits, stop bits, parity, flow control,
/// correct baud rate, and commits with TCSANOW.
fn configure_termios(fd: &OwnedFd, config: &SerialConfig) -> Result<(), SerialError> {
    let baud_rate = baud_rate_from_u32(config.baud_rate)?;

    let mut termios = tcgetattr(fd)
        .map_err(|e| SerialError::InvalidConfig(format!("tcgetattr failed: {}", e)))?;

    // Raw mode: disable all special processing (canonical mode, echo, signals, flow ctrl).
    // cfmakeraw clears CSIZE and sets CS8 among other things; we apply our own data-bits
    // setting afterwards to override.
    cfmakeraw(&mut termios);

    // --- data bits (character size) ---
    // Clear the CSIZE mask first, then set the requested width.
    termios.control_flags &= !ControlFlags::CSIZE;
    let cs = match config.data_bits {
        5 => ControlFlags::CS5,
        6 => ControlFlags::CS6,
        7 => ControlFlags::CS7,
        _ => ControlFlags::CS8, // 8 is the default and most common
    };
    termios.control_flags |= cs;

    // --- stop bits ---
    if config.stop_bits >= 2 {
        termios.control_flags |= ControlFlags::CSTOPB;
    } else {
        termios.control_flags &= !ControlFlags::CSTOPB;
    }

    // --- parity ---
    match config.parity {
        Parity::None => {
            termios.control_flags &= !(ControlFlags::PARENB | ControlFlags::PARODD);
        }
        Parity::Even => {
            termios.control_flags |= ControlFlags::PARENB;
            termios.control_flags &= !ControlFlags::PARODD;
        }
        Parity::Odd => {
            termios.control_flags |= ControlFlags::PARENB | ControlFlags::PARODD;
        }
    }

    // --- flow control ---
    match config.flow_control {
        FlowControl::None => {
            termios.control_flags &= !ControlFlags::CRTSCTS;
            termios.input_flags &= !(InputFlags::IXON | InputFlags::IXOFF);
        }
        FlowControl::Hardware => {
            termios.control_flags |= ControlFlags::CRTSCTS;
            termios.input_flags &= !(InputFlags::IXON | InputFlags::IXOFF);
        }
        FlowControl::Software => {
            termios.control_flags &= !ControlFlags::CRTSCTS;
            termios.input_flags |= InputFlags::IXON | InputFlags::IXOFF;
        }
    }

    // Set input and output baud rate
    cfsetispeed(&mut termios, baud_rate)
        .map_err(|e| SerialError::InvalidConfig(format!("cfsetispeed failed: {}", e)))?;
    cfsetospeed(&mut termios, baud_rate)
        .map_err(|e| SerialError::InvalidConfig(format!("cfsetospeed failed: {}", e)))?;

    // Apply immediately (TCSANOW: change occurs immediately)
    tcsetattr(fd, SetArg::TCSANOW, &termios)
        .map_err(|e| SerialError::InvalidConfig(format!("tcsetattr failed: {}", e)))?;

    Ok(())
}

/// Asynchronous serial port using monoio (io_uring).
///
/// Open a real serial device or PTY slave with [`SerialPort::open`], then use
/// the [`Transport`] implementation to send and receive bytes asynchronously.
///
/// ## Runtime Requirement
///
/// [`SerialPort::open`] must be called within an active monoio runtime context
/// because `UnixStream::from_std` registers the fd with the io_uring driver.
/// In practice this means calling `open()` from within `block_on` or an async task.
///
/// ## Implementation Notes
///
/// The fd is wrapped in `monoio::net::UnixStream` via `StdUnixStream::from_raw_fd`.
/// This is **sound** because:
/// - The fd is a valid, open, readable/writable file descriptor (serial port or PTY).
/// - I/O is performed exclusively via `writev`/`readv`, which submit
///   `IORING_OP_WRITEV`/`IORING_OP_READV` to io_uring.  These operations are
///   fd-type-agnostic — the kernel only requires that the fd is open and
///   supports vectored I/O, which both serial ports and PTY devices do.
/// - `IORING_OP_SEND`/`IORING_OP_RECV` (socket-only) are **never** used.
/// - The `UnixStream` type is used solely as a handle that exposes the
///   `AsyncReadRent` / `AsyncWriteRent` interface backed by readv/writev;
///   no Unix-domain-socket semantics are exercised.
pub struct SerialPort {
    /// monoio streaming I/O wrapper around the real file descriptor.
    ///
    /// Held as `UnixStream` for its `AsyncReadRent` / `AsyncWriteRent` impls
    /// (backed by IORING_OP_READV / IORING_OP_WRITEV, which are fd-type-agnostic).
    stream: UnixStream,
    /// Device path, kept for diagnostics
    path: String,
}

impl fmt::Debug for SerialPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SerialPort")
            .field("path", &self.path)
            .field("fd", &self.stream.as_raw_fd())
            .finish()
    }
}

impl SerialPort {
    /// Open a serial device at `path` with the given `config`.
    ///
    /// Must be called within an active monoio runtime context — the fd registration
    /// with io_uring happens inside `UnixStream::from_std`.
    ///
    /// Configures the port for raw mode according to all fields of `config`:
    /// baud rate, data bits (5–8), stop bits (1–2), parity, and flow control.
    ///
    /// `SerialConfig::default()` gives 9600 baud, 8N1, no flow control.
    pub fn open(path: &str, config: SerialConfig) -> crate::SerialResult<Self> {
        // Open the device:
        // O_NOCTTY: don't become controlling terminal
        // O_NONBLOCK: avoid blocking if no carrier detect (DCD) signal
        let std_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
            .open(path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    SerialError::DeviceNotFound(path.to_string())
                } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                    SerialError::PermissionDenied(path.to_string())
                } else {
                    SerialError::Io(e)
                }
            })?;

        // Convert std::fs::File to OwnedFd for termios configuration.
        // termios must be configured before handing the fd to monoio.
        let owned_fd: OwnedFd = std_file.into();

        // Configure termios with all SerialConfig fields.
        configure_termios(&owned_fd, &config)?;

        // Wrap in monoio::net::UnixStream for AsyncReadRent + AsyncWriteRent.
        //
        // SAFETY: `owned_fd` is a valid open file descriptor (serial port or PTY).
        // We consume it via `into_raw_fd()` (preventing double-close) and hand
        // the raw fd to `StdUnixStream::from_raw_fd`.  The stream is then used
        // exclusively with `writev`/`readv` (IORING_OP_WRITEV / IORING_OP_READV),
        // which are fd-type-agnostic and do not require a Unix domain socket.
        // `UnixStream::from_std` registers the fd with the active io_uring driver.
        let raw_fd = owned_fd.into_raw_fd();
        let std_unix: StdUnixStream = unsafe { StdUnixStream::from_raw_fd(raw_fd) };
        std_unix.set_nonblocking(true).map_err(SerialError::Io)?;
        let stream = UnixStream::from_std(std_unix).map_err(SerialError::Io)?;

        Ok(Self {
            stream,
            path: path.to_string(),
        })
    }

    /// Get the device path this port was opened on.
    pub fn path(&self) -> &str {
        &self.path
    }
}

#[async_trait(?Send)]
impl Transport for SerialPort {
    /// Write `data` to the serial port. Returns the number of bytes written.
    ///
    /// Uses `IORING_OP_WRITEV` (via `writev`) which works on any fd type,
    /// unlike `IORING_OP_SEND` which is socket-only.
    async fn write(&mut self, data: &[u8]) -> Result<usize, TransportError> {
        // Build a single-segment VecBuf for writev
        let buf: VecBuf = vec![data.to_vec()].into();
        let (result, _buf) = self.stream.writev(buf).await;
        let n = result?;
        Ok(n)
    }

    /// Read bytes from the serial port into `buf`. Returns the number of bytes read.
    ///
    /// Uses `IORING_OP_READV` (via `readv`) which works on any fd type,
    /// unlike `IORING_OP_RECV` which is socket-only.
    ///
    /// ## Non-blocking behaviour
    ///
    /// The fd is opened with `O_NONBLOCK`.  When no data is available, io_uring
    /// completes the `readv` immediately with `EAGAIN`, which surfaces as
    /// `ErrorKind::WouldBlock`.  This method maps that to `Ok(0)` — the standard
    /// non-blocking read contract meaning "no data yet, try again later".
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
        // Build a single-segment VecBuf for readv; initialize with zeros so iov_len is correct
        let read_buf: VecBuf = vec![vec![0u8; buf.len()]].into();
        let (result, read_buf) = self.stream.readv(read_buf).await;
        match result {
            Ok(n) => {
                // Extract the raw buffers back and copy data to caller
                let vecs: Vec<Vec<u8>> = read_buf.into();
                if n > 0 {
                    buf[..n].copy_from_slice(&vecs[0][..n]);
                }
                Ok(n)
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                // EAGAIN: fd is non-blocking and no data is available yet.
                // Return 0 — caller should retry.
                Ok(0)
            }
            Err(e) => Err(TransportError::Io(e)),
        }
    }

    /// Flush the serial port output buffer.
    ///
    /// Calls `tcdrain(2)` which blocks until all output queued in the kernel
    /// serial driver has been physically transmitted.  For PTY devices this
    /// returns immediately (the kernel has no transmit queue to drain).
    ///
    /// Note: `tcdrain` is a synchronous syscall.  It only blocks during the
    /// flush path, which is intentionally short — this is acceptable in an
    /// async context where callers invoke `flush` deliberately.
    async fn flush(&mut self) -> Result<(), TransportError> {
        let raw_fd = self.stream.as_raw_fd();
        // SAFETY: raw_fd is valid for the lifetime of self.stream.
        // BorrowedFd::borrow_raw does not take ownership; we hold the fd
        // alive via self.stream for the duration of this call.
        let borrowed = unsafe { BorrowedFd::borrow_raw(raw_fd) };
        tcdrain(borrowed)
            .map_err(|e| TransportError::Io(std::io::Error::from_raw_os_error(e as i32)))?;
        Ok(())
    }
}

impl AsRawFd for SerialPort {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        self.stream.as_raw_fd()
    }
}

#[cfg(test)]
mod tests {
    use emulator::pty::PtyPair;
    use framework::transport::Transport;
    use monoio::RuntimeBuilder;

    use super::*;

    /// Helper: write bytes to a raw fd (master side of PTY) synchronously.
    fn write_to_master(master_fd: std::os::fd::RawFd, data: &[u8]) {
        let written =
            unsafe { libc::write(master_fd, data.as_ptr() as *const libc::c_void, data.len()) };
        assert!(written > 0, "write to master failed: errno={}", unsafe {
            *libc::__errno_location()
        });
    }

    /// Helper: read bytes from a raw fd (master side of PTY) synchronously.
    fn read_from_master(master_fd: std::os::fd::RawFd, max_len: usize) -> Vec<u8> {
        let mut buf = vec![0u8; max_len];
        // Brief pause to let the kernel process io_uring writes before reading
        std::thread::sleep(std::time::Duration::from_millis(20));
        let n = unsafe { libc::read(master_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        assert!(n > 0, "read from master returned {}", n);
        buf.truncate(n as usize);
        buf
    }

    /// Build a monoio IoUring runtime for test use.
    fn make_runtime() -> monoio::Runtime<monoio::IoUringDriver> {
        RuntimeBuilder::<monoio::IoUringDriver>::new()
            .build()
            .expect("monoio runtime build failed")
    }

    /// `SerialPort::open` must be called inside a monoio runtime because
    /// `UnixStream::from_std` registers the fd with the io_uring driver.
    #[test]
    fn test_serial_port_open_on_pty_slave() {
        let pair = PtyPair::new().expect("PTY creation failed");
        let slave = pair.slave_path().to_string();

        make_runtime().block_on(async {
            let port = SerialPort::open(&slave, SerialConfig::default())
                .expect("SerialPort::open failed");
            assert_eq!(port.path(), slave);
        });
    }

    #[test]
    fn test_transport_read_from_master() {
        let pair = PtyPair::new().expect("PTY creation failed");
        let slave = pair.slave_path().to_string();

        // Write test data to master BEFORE starting the runtime so it is
        // already in the PTY buffer when the async read begins.
        let expected = b"FA;";
        write_to_master(pair.master_raw_fd(), expected);

        let result = make_runtime().block_on(async {
            let mut port = SerialPort::open(&slave, SerialConfig::default())
                .expect("SerialPort::open failed");
            let mut buf = vec![0u8; 64];
            let n = port.read(&mut buf).await.expect("Transport::read failed");
            buf.truncate(n);
            buf
        });

        assert_eq!(result, expected, "Read data did not match written data");
    }

    #[test]
    fn test_transport_write_to_master() {
        let pair = PtyPair::new().expect("PTY creation failed");
        let slave = pair.slave_path().to_string();

        let expected = b"ID020;";

        // Write from SerialPort (slave side) via async Transport::write
        make_runtime().block_on(async {
            let mut port = SerialPort::open(&slave, SerialConfig::default())
                .expect("SerialPort::open failed");
            let n = port.write(expected).await.expect("Transport::write failed");
            assert_eq!(n, expected.len());
        });

        // Read back from master (synchronous, outside the runtime)
        let received = read_from_master(pair.master_raw_fd(), 64);
        assert_eq!(&received, expected, "Master received unexpected data");
    }

    #[test]
    fn test_unsupported_baud_rate_error() {
        // baud_rate_from_u32 is a pure function — no runtime needed
        let err = baud_rate_from_u32(12345).expect_err("Expected error for invalid baud rate");
        assert!(
            matches!(err, SerialError::InvalidConfig(_)),
            "Expected InvalidConfig, got {:?}",
            err
        );
    }

    #[test]
    fn test_open_nonexistent_device_error() {
        // Device-not-found happens before UnixStream::from_std, so we still need
        // a runtime (open() may call from_std if path exists), but here the path
        // doesn't exist so the error occurs in OpenOptions::open before that.
        let err = make_runtime().block_on(async {
            SerialPort::open("/dev/ttyDOESNOTEXIST99999", SerialConfig::default())
        });
        assert!(
            matches!(err, Err(SerialError::DeviceNotFound(_))),
            "Expected DeviceNotFound, got {:?}",
            err
        );
    }

    /// Full-duplex roundtrip test: write from slave via Transport, read from master;
    /// then write from master, read from slave via Transport.
    #[test]
    fn test_transport_roundtrip() {
        let pair = PtyPair::new().expect("PTY creation failed");
        let slave = pair.slave_path().to_string();
        let master_fd = pair.master_raw_fd();

        // --- slave → master direction ---
        let slave_to_master_msg = b"FA;";
        make_runtime().block_on(async {
            let mut port = SerialPort::open(&slave, SerialConfig::default())
                .expect("SerialPort::open failed");
            let n = port
                .write(slave_to_master_msg)
                .await
                .expect("Transport::write failed");
            assert_eq!(n, slave_to_master_msg.len(), "write returned wrong byte count");
        });
        let received_on_master = read_from_master(master_fd, 64);
        assert_eq!(
            &received_on_master, slave_to_master_msg,
            "master did not receive slave's write"
        );

        // --- master → slave direction ---
        let master_to_slave_msg = b"FA00014000000;";
        write_to_master(master_fd, master_to_slave_msg);
        let received_on_slave = make_runtime().block_on(async {
            let mut port = SerialPort::open(&slave, SerialConfig::default())
                .expect("SerialPort::open failed");
            let mut buf = vec![0u8; 64];
            let n = port.read(&mut buf).await.expect("Transport::read failed");
            buf.truncate(n);
            buf
        });
        assert_eq!(
            &received_on_slave, master_to_slave_msg,
            "slave did not receive master's write"
        );
    }
}
