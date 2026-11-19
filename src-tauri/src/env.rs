//! The environment commands run in.
//!
//! Started from the app menu, rouse inherits the desktop session's environment, which lacks
//! what shell rc files add (nvm, pnpm, cargo, asdf on PATH). So once at startup we ask the
//! user's login shell for its environment, the way VS Code does, and run every command with
//! it. rc files can hang or print noise, so the capture has a timeout and a start marker.

use std::collections::HashMap;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MARKER: &str = "__ROUSE_ENV_START__";

/// The login shell's environment, or this process's own if the shell fails or takes longer
/// than `timeout`.
pub fn login_env(timeout: Duration) -> HashMap<String, String> {
    capture(timeout).unwrap_or_else(|| std::env::vars().collect())
}

fn capture(timeout: Duration) -> Option<HashMap<String, String>> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
    // -l reads profile files, -i reads rc files (where nvm usually lives).
    let mut child = Command::new(&shell)
        .args(["-l", "-i", "-c", &format!("printf '{MARKER}\\0'; env -0")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < timeout => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let out = reader.join().ok()?;
    parse(&out)
}

/// `NAME=value` pairs separated by NUL, after the marker.
fn parse(out: &[u8]) -> Option<HashMap<String, String>> {
    let text = String::from_utf8_lossy(out);
    let start = text.find(MARKER)? + MARKER.len();
    let vars: HashMap<String, String> = text[start..]
        .split('\0')
        .filter_map(|pair| pair.split_once('='))
        .filter(|(k, _)| !k.is_empty() && !k.contains('\n'))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    vars.contains_key("PATH").then_some(vars)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_after_marker_and_ignores_rc_noise() {
        let out = b"welcome to zsh!\n__ROUSE_ENV_START__\0PATH=/a:/b\0FOO=x=y\0";
        let env = parse(out).unwrap();
        assert_eq!(env["PATH"], "/a:/b");
        assert_eq!(env["FOO"], "x=y");
    }

    #[test]
    fn no_marker_means_no_env() {
        assert!(parse(b"PATH=/a").is_none());
    }

    #[test]
    fn captures_a_real_login_shell() {
        let env = login_env(Duration::from_secs(10));
        assert!(env.contains_key("PATH"));
    }
}
