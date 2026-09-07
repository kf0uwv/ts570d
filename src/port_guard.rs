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

/// No `/proc` to read; the open itself is the check.
#[cfg(not(target_os = "linux"))]
pub fn holder_of(_path: &std::path::Path) -> Option<PortHolder> {
    None
}

#[cfg(target_os = "linux")]
fn process_name(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
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
