//! io_uring-based serial communication implementation
//!
//! This module provides high-performance asynchronous serial communication
//! using Linux io_uring interface for zero-copy operations via monoio.

use std::fmt;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream as StdUnixStream;

use async_trait::async_trait;
use monoio::buf::VecBuf;
use monoio::io::{AsyncReadRent, AsyncWriteRent};
use monoio::net::UnixStream;
use nix::sys::termios::{
    cfmakeraw, cfsetispeed, cfsetospeed, tcgetattr, tcsetattr, BaudRate, SetArg,
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

/// Configure a file descriptor for raw serial communication (8N1, no flow control).
///
/// Applies: raw mode (cfmakeraw), correct baud rate, applies with TCSANOW.
fn configure_termios(fd: &OwnedFd, baud: u32) -> Result<(), SerialError> {
    let baud_rate = baud_rate_from_u32(baud)?;

    let mut termios = tcgetattr(fd)
        .map_err(|e| SerialError::InvalidConfig(format!("tcgetattr failed: {}", e)))?;

    // Raw mode: disable all special processing (canonical mode, echo, signals, flow ctrl)
    cfmakeraw(&mut termios);

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
/// I/O is performed via `writev`/`readv` on `monoio::net::UnixStream`, which
/// translate to `IORING_OP_WRITEV`/`IORING_OP_READV` io_uring operations.
/// Unlike `IORING_OP_SEND`/`IORING_OP_RECV` (socket-only), the vectored variants
/// work on any file descriptor including serial ports and PTY devices.
pub struct SerialPort {
    /// monoio streaming I/O wrapper around the real file descriptor
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
    /// Open a serial device at `path` with the given `baud` rate.
    ///
    /// Must be called within an active monoio runtime context — the fd registration
    /// with io_uring happens inside `UnixStream::from_std`.
    ///
    /// Configures the port for 8N1 raw mode with no flow control via termios.
    pub fn open(path: &str, baud: u32) -> crate::SerialResult<Self> {
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

        // Configure termios (raw mode, 8N1, no flow control, correct baud)
        configure_termios(&owned_fd, baud)?;

        // Wrap in monoio::net::UnixStream for AsyncReadRent + AsyncWriteRent.
        // We use writev/readv (IORING_OP_WRITEV / IORING_OP_READV) which work
        // on any fd type — serial port, PTY, pipe, etc.
        // UnixStream::from_std registers the fd with the active io_uring driver,
        // so this call must happen within a monoio runtime.
        // SAFETY: owned_fd is a valid open file descriptor.
        let std_unix: StdUnixStream = unsafe { StdUnixStream::from_raw_fd(owned_fd.into_raw_fd()) };
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
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
        // Build a single-segment VecBuf for readv; initialize with zeros so iov_len is correct
        let read_buf: VecBuf = vec![vec![0u8; buf.len()]].into();
        let (result, read_buf) = self.stream.readv(read_buf).await;
        let n = result?;
        // Extract the raw buffers back and copy data to caller
        let vecs: Vec<Vec<u8>> = read_buf.into();
        if n > 0 {
            buf[..n].copy_from_slice(&vecs[0][..n]);
        }
        Ok(n)
    }

    /// Flush the serial port output buffer.
    ///
    /// For io_uring-backed serial ports, writes are submitted directly to the
    /// kernel; there is no userspace buffer to drain. Returns `Ok(())`.
    async fn flush(&mut self) -> Result<(), TransportError> {
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
            let port = SerialPort::open(&slave, 9600).expect("SerialPort::open failed");
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
            let mut port = SerialPort::open(&slave, 9600).expect("SerialPort::open failed");
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
            let mut port = SerialPort::open(&slave, 9600).expect("SerialPort::open failed");
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
        let err =
            make_runtime().block_on(async { SerialPort::open("/dev/ttyDOESNOTEXIST99999", 9600) });
        assert!(
            matches!(err, Err(SerialError::DeviceNotFound(_))),
            "Expected DeviceNotFound, got {:?}",
            err
        );
    }
}
