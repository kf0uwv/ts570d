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

//! Refusing to take a serial port out from under a running station.
//!
//! # Why this exists
//!
//! On this bench the CAT port is also the **PTT line**: WSJT-X keys the
//! radio by asserting DTR on the same `/dev/ttyUSB0` it uses for rig
//! control. A second program that opens that port forces DTR to a state of
//! its own choosing and takes the line away mid-QSO.
//!
//! The bench record has warned about this since 2026-09-07 (item 33, trap
//! 2): *"`ts570d server` restarted three times mid-session, each time
//! seizing `/dev/ttyUSB0` and the sound card and asserting DTR. It has to
//! stay stopped while WSJT-X owns the port."* It was then done to a live
//! station several more times the same afternoon while testing an
//! unrelated feature. A rule that depends on remembering is not a rule, so
//! this checks.
//!
//! # Why `/proc` rather than a lock file
//!
//! The classic `/var/lock/LCK..ttyUSB0` convention only works if every
//! program observes it, and the one that matters here does not. An
//! advisory `flock` has the same problem. Reading `/proc/<pid>/fd` finds
//! whoever actually has the device open, whatever conventions they keep.
//!
//! Linux-only, and that is fine: on Windows the `CreateFile` open fails
//! with a sharing violation, which already produces a refusal.

#[cfg(target_os = "linux")]
use std::path::Path;

/// A process holding the port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortHolder {
    pub pid: u32,
    /// The process's own name, for a message an operator can act on.
    pub name: String,
}

impl std::fmt::Display for PortHolder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (pid {})", self.name, self.pid)
    }
}

/// Who has `path` open, if anyone.
///
/// Compares resolved paths, so a symlinked device name still matches the
/// node a holder actually opened.
#[cfg(target_os = "linux")]
pub fn holder_of(path: &Path) -> Option<PortHolder> {
    let target = std::fs::canonicalize(path).ok()?;
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let pid: u32 = match entry.file_name().to_str().and_then(|s| s.parse().ok()) {
            Some(pid) => pid,
            None => continue,
        };
        // Another user's process is unreadable, and that is not an error
        // worth reporting: it is simply not something we can see.
        let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if std::fs::read_link(fd.path()).is_ok_and(|l| l == target) {
                return Some(PortHolder {
                    pid,
                    name: process_name(pid),
                });
            }
        }
    }
    None
}

/// Who would lose their transmitter if this program opened `port`.
///
/// [`holder_of`] answers the raw question -- who has this node open. This
/// answers the one that should decide a refusal, and the two differ on a
/// pseudo-terminal.
///
/// A PTY always has a holder: whoever opened the master, which for the
/// emulator is the emulator itself. That is the pairing working as
/// designed, not contention. Nor does the hazard apply -- the reason this
/// guard exists is that on a real adapter DTR is the PTT line, and a PTY
/// has no DTR, no PA and no transmitter to unkey. Refusing here would
/// mean no server could ever be run against the emulator, which is the
/// only way to test anything without putting the station on the air.
#[cfg(target_os = "linux")]
pub fn contention_on(port: &Path) -> Option<PortHolder> {
    if is_pty(&port.to_string_lossy()) {
        return None;
    }
    holder_of(port)
}

#[cfg(not(target_os = "linux"))]
pub fn contention_on(port: &std::path::Path) -> Option<PortHolder> {
    holder_of(port)
}

/// No `/proc` to read; the open itself is the check.
#[cfg(not(target_os = "linux"))]
pub fn holder_of(_path: &std::path::Path) -> Option<PortHolder> {
    None
}

/// The binary behind a `/proc/<pid>/exe` link, with the kernel's
/// ` (deleted)` marker removed.
///
/// After a rebuild the kernel reports a running process's exe as
/// `.../ts570d (deleted)`, because the inode it is executing no longer has
/// that name. Comparing the raw links then says two processes running the
/// same program are running different ones -- which is how three servers
/// came to be running at once on 2026-09-07, each pulling DTR low on open
/// and unkeying the transmitter mid-transmission. The same marker had
/// already defeated the kill loop in `scratchpad/srv.sh` earlier the same
/// evening.
#[cfg(target_os = "linux")]
fn running_binary<P: AsRef<Path>>(link: P) -> Option<String> {
    let path = std::fs::read_link(link).ok()?;
    let text = path.to_string_lossy();
    Some(
        text.strip_suffix(" (deleted)")
            .unwrap_or(text.as_ref())
            .to_string(),
    )
}

#[cfg(target_os = "linux")]
fn process_name(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Another instance of this program already running as a server.
///
/// `holder_of` asks "who has this device node open", which is the right
/// question until the adapter re-enumerates. Then the running server holds
/// a handle to the *old* node, the new node has nobody on it, and a second
/// server starts happily. Observed 2026-09-07: three servers at once, each
/// opening the port with `initial_dtr: false` -- and DTR is the PTT line,
/// so every start unkeyed a transmitter mid-transmission.
///
/// So this asks the other question too: is another one of us already
/// being a server at all, whatever node it thinks it has.
#[cfg(target_os = "linux")]
pub fn another_server(port: &str) -> Option<PortHolder> {
    let me = std::process::id();
    let exe = running_binary("/proc/self/exe")?;
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let pid: u32 = match entry.file_name().to_str().and_then(|s| s.parse().ok()) {
            Some(p) if p != me => p,
            _ => continue,
        };
        if running_binary(entry.path().join("exe")) != Some(exe.clone()) {
            continue;
        }
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let args: Vec<&[u8]> = cmdline.split(|b| *b == 0).collect();
        // The first argument after the program name. A TUI or a
        // `calibrate` run is not a competing server.
        if args.get(1) != Some(&b"server".as_slice()) {
            continue;
        }
        let theirs = port_argument(&args);
        if !ports_collide(port, theirs.as_deref()) {
            continue;
        }
        return Some(PortHolder {
            pid,
            name: process_name(pid),
        });
    }
    None
}

/// The `--port` a command line asked for.
#[cfg(target_os = "linux")]
fn port_argument(args: &[&[u8]]) -> Option<String> {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if *arg == b"--port" {
            return args.next().map(|v| String::from_utf8_lossy(v).into_owned());
        }
        if let Some(rest) = arg.strip_prefix(b"--port=".as_slice()) {
            return Some(String::from_utf8_lossy(rest).into_owned());
        }
    }
    None
}

/// Whether two servers are contending for one physical link.
///
/// The subject of this guard is a single USB adapter whose device *name*
/// is unstable: when it re-enumerates, the running server keeps a handle
/// to the old node and a second server starting on the new node sees no
/// holder at all. So two real device nodes are treated as the same
/// adapter even when they are spelled differently -- conservative, and
/// right on a bench with one radio on it.
///
/// A pseudo-terminal is the exception, and not a special case bolted on:
/// a PTY is created by whoever opened its master and cannot be a
/// re-enumeration of anything. An emulator's PTY is genuinely a different
/// link from `/dev/ttyUSB0`, and refusing to run a server against the
/// emulator while the station's real server is up would make it
/// impossible to test anything without taking the station off the air --
/// which is the failure this guard exists to prevent, arrived at from the
/// other direction.
#[cfg(target_os = "linux")]
fn ports_collide(mine: &str, theirs: Option<&str>) -> bool {
    // Nothing on their command line to compare. Assume the worst: this is
    // the re-enumeration case the guard was written for.
    let Some(theirs) = theirs else { return true };
    if mine == theirs {
        return true;
    }
    !(is_pty(mine) || is_pty(theirs))
}

#[cfg(target_os = "linux")]
fn is_pty(port: &str) -> bool {
    port.starts_with("/dev/pts/")
}

#[cfg(not(target_os = "linux"))]
pub fn another_server(_port: &str) -> Option<PortHolder> {
    None
}

/// What to tell an operator who is already running one.
pub fn already_running(other: &PortHolder) -> String {
    format!(
        "another `ts570d server` is already running as {other}.\n\
         \n\
         Two servers both own the serial port, and both set DTR low when they open\n\
         it -- which on this station unkeys the transmitter, mid-transmission if the\n\
         timing is unlucky. They also interleave CAT traffic on one link.\n\
         \n\
         Stop it first, or pass --force if you are certain this is safe."
    )
}

/// What to tell an operator who is about to lose their transmitter.
pub fn refusal(port: &str, holder: &PortHolder) -> String {
    format!(
        "{port} is already open by {holder}.\n\
         \n\
         On this radio the CAT port is also the PTT line, so opening it here would\n\
         take the port away from {} and force DTR to a state of this program's\n\
         choosing -- mid-transmission, if the timing is unlucky.\n\
         \n\
         Stop {} first, or pass --force if you are certain this is safe.",
        holder.name, holder.name
    )
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn an_open_file_is_found_and_attributed() {
        // The check has to actually find a holder, so the test takes one:
        // this process opens a file and must find itself.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("port");
        let _held = std::fs::File::create(&path).expect("create");

        let holder = holder_of(&path).expect("should find this process");
        assert_eq!(holder.pid, std::process::id());
        assert!(!holder.name.is_empty());
    }

    #[test]
    fn a_file_nobody_has_open_has_no_holder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("port");
        std::fs::write(&path, b"").expect("write");
        assert_eq!(holder_of(&path), None);
    }

    #[test]
    fn a_path_that_does_not_exist_has_no_holder() {
        // Not an error: a serial adapter that has not enumerated yet is
        // absent, not contended, and the open will say so far better.
        assert_eq!(holder_of(Path::new("/dev/definitely-not-here")), None);
    }

    #[test]
    fn a_symlinked_device_name_still_matches() {
        // Device paths are routinely symlinks -- /dev/serial/by-id/... is
        // the stable way to name an adapter that keeps changing ttyUSBn.
        let dir = tempfile::tempdir().expect("tempdir");
        let real = dir.path().join("ttyUSB0");
        let link = dir.path().join("by-id-cable");
        let _held = std::fs::File::create(&real).expect("create");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        assert_eq!(
            holder_of(&link).map(|h| h.pid),
            Some(std::process::id()),
            "a symlink must resolve to the node the holder opened"
        );
    }

    #[test]
    fn a_rebuilt_binary_is_still_the_same_program() {
        // The kernel marks a running process's exe as "(deleted)" once the
        // file has been replaced. Comparing raw links then treats two
        // instances of the same program as different programs -- which let
        // three servers run at once, each pulling DTR low on open and
        // unkeying the transmitter mid-transmission.
        let dir = tempfile::tempdir().expect("tempdir");
        let real = dir.path().join("ts570d");
        std::fs::write(&real, b"").expect("write");
        let link = dir.path().join("exe");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        let seen = running_binary(&link).expect("should resolve");
        assert_eq!(seen, real.to_string_lossy());
        assert!(!seen.ends_with(" (deleted)"));
    }

    #[test]
    fn a_missing_link_resolves_to_nothing() {
        // Another user's process is unreadable, and that is not an error:
        // it is simply not something we can see.
        assert_eq!(running_binary("/proc/nonexistent/exe"), None);
    }

    #[test]
    fn a_pty_with_its_own_emulator_on_the_far_end_is_not_contention() {
        // A PTY always has a holder -- whoever opened the master. That is
        // the pairing working, and a PTY has no DTR to unkey anything
        // with.
        assert_eq!(contention_on(Path::new("/dev/pts/19")), None);
    }

    #[test]
    fn a_real_port_still_reports_its_holder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ttyUSB0");
        let _held = std::fs::File::create(&path).expect("create");
        assert_eq!(
            contention_on(&path).map(|h| h.pid),
            Some(std::process::id()),
            "the guard must still fire on a real device node"
        );
    }

    #[test]
    fn the_same_port_named_twice_collides() {
        assert!(ports_collide("/dev/ttyUSB0", Some("/dev/ttyUSB0")));
    }

    #[test]
    fn two_real_device_nodes_are_assumed_to_be_one_re_enumerated_adapter() {
        // The case the guard exists for: the adapter came back as ttyUSB1
        // while the running server still holds a handle to ttyUSB0, so
        // `holder_of` finds nobody on the new node.
        assert!(ports_collide("/dev/ttyUSB1", Some("/dev/ttyUSB0")));
    }

    #[test]
    fn a_pty_never_collides_with_a_real_adapter() {
        // An emulator's PTY is created fresh by whoever opened its
        // master; it cannot be a re-enumeration of a USB adapter. Testing
        // against the emulator must not require taking the station off
        // the air.
        assert!(!ports_collide("/dev/pts/19", Some("/dev/ttyUSB0")));
        assert!(!ports_collide("/dev/ttyUSB0", Some("/dev/pts/19")));
    }

    #[test]
    fn two_different_ptys_do_not_collide() {
        assert!(!ports_collide("/dev/pts/19", Some("/dev/pts/20")));
    }

    #[test]
    fn the_same_pty_twice_still_collides() {
        assert!(ports_collide("/dev/pts/19", Some("/dev/pts/19")));
    }

    #[test]
    fn a_server_whose_port_cannot_be_read_is_assumed_to_collide() {
        // Silence is not evidence of safety.
        assert!(ports_collide("/dev/ttyUSB0", None));
    }

    #[test]
    fn the_port_argument_is_found_in_either_spelling() {
        let split: Vec<&[u8]> = vec![b"ts570d", b"server", b"--port", b"/dev/ttyUSB0"];
        assert_eq!(port_argument(&split).as_deref(), Some("/dev/ttyUSB0"));
        let joined: Vec<&[u8]> = vec![b"ts570d", b"server", b"--port=/dev/pts/9"];
        assert_eq!(port_argument(&joined).as_deref(), Some("/dev/pts/9"));
        let absent: Vec<&[u8]> = vec![b"ts570d", b"server", b"--rigctl-port", b"4532"];
        assert_eq!(port_argument(&absent), None);
        // `--port` last, with nothing after it.
        let dangling: Vec<&[u8]> = vec![b"ts570d", b"server", b"--port"];
        assert_eq!(port_argument(&dangling), None);
    }

    #[test]
    fn a_similarly_named_flag_is_not_the_serial_port() {
        let other: Vec<&[u8]> = vec![b"ts570d", b"server", b"--console-port", b"7400"];
        assert_eq!(port_argument(&other), None);
    }

    #[test]
    fn the_refusal_says_who_and_what_to_do() {
        let text = refusal(
            "/dev/ttyUSB0",
            &PortHolder {
                pid: 4242,
                name: "wsjtx".to_string(),
            },
        );
        assert!(text.contains("wsjtx"), "must name the holder: {text}");
        assert!(text.contains("4242"), "and its pid");
        assert!(text.contains("PTT"), "must say why it matters");
        assert!(text.contains("--force"), "and how to override");
    }
}
