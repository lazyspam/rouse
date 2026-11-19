//! Cleaning up after a crash.
//!
//! If rouse dies without stopping its services (crash, kill -9), their dev servers keep
//! running and keep their ports, so the next Start fails. rouse records each running
//! process group with its start time; on the next launch it stops the ones still alive.
//! The start time check makes sure a reused PID is never killed by mistake.

use std::time::Duration;

/// Start time of a process in clock ticks since boot (field 22 of /proc/<pid>/stat).
pub fn start_ticks(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name (field 2) may contain spaces; fields after it follow ") ".
    let rest = stat.rsplit_once(") ")?.1;
    rest.split_whitespace().nth(19)?.parse().ok()
}

/// Stop recorded process groups that are still the same processes. Returns how many.
#[cfg(unix)]
pub fn reap(recorded: &[(u32, u64)]) -> usize {
    let alive: Vec<u32> = recorded.iter().filter(|(pid, ticks)| start_ticks(*pid) == Some(*ticks)).map(|(pid, _)| *pid).collect();
    for pid in &alive {
        unsafe { libc::kill(-(*pid as i32), libc::SIGTERM) };
    }
    if !alive.is_empty() {
        std::thread::sleep(Duration::from_secs(2));
        for pid in &alive {
            unsafe { libc::kill(-(*pid as i32), libc::SIGKILL) };
        }
    }
    alive.len()
}

#[cfg(not(unix))]
pub fn reap(_recorded: &[(u32, u64)]) -> usize {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_own_start_time() {
        assert!(start_ticks(std::process::id()).is_some());
        assert!(start_ticks(u32::MAX).is_none());
    }

    #[test]
    fn reaps_a_live_group_but_not_a_mismatched_one() {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("sleep").arg("300").process_group(0).spawn().unwrap();
        let pid = child.id();
        let ticks = start_ticks(pid).unwrap();
        assert_eq!(reap(&[(pid, ticks + 1)]), 0, "wrong start time must be left alone");
        assert_eq!(reap(&[(pid, ticks)]), 1);
        assert!(child.wait().is_ok());
    }
}
