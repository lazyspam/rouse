//! Running commands.
//!
//! Each service runs in its own pseudo-terminal, so tools see a TTY: colours stay, watch modes
//! redraw properly, and nothing buffers its output until exit. portable-pty starts the child
//! with `setsid()`, so the child leads its own process group and Stop can signal the whole
//! tree (`kill(-pgid)`): `pnpm dev` -> node -> esbuild all go together.
//!
//! Per command (see `overrides`): the shell, the stop signal and grace period, and
//! auto-restart. A crash with auto-restart on waits 1, 2, 4… seconds (at most 30) before the
//! next run, and gives up after `MAX_CRASHES` within `CRASH_WINDOW`. Restarts keep the
//! output, so the crash stays readable above the new run.
//!
//! Output is kept in a ring buffer per service (replayed when the window shows a service) and
//! handed to `sink` in batches every `FLUSH`, so a noisy build doesn't flood the UI.
//!
//! This module has no Tauri types; the app wires `sink` to window events.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use regex::Regex;
use serde::{Deserialize, Serialize};

pub type ServiceId = i64;

const FLUSH: Duration = Duration::from_millis(40);
/// Bytes of output kept per service.
const BUFFER: usize = 1 << 20;
/// Auto-restart gives up after this many crashes within the window.
const MAX_CRASHES: usize = 5;
const CRASH_WINDOW: Duration = Duration::from_secs(120);
/// First restart delay; doubles per crash, capped at 30 s.
#[cfg(not(test))]
const BACKOFF_BASE_MS: u64 = 1000;
#[cfg(test)]
const BACKOFF_BASE_MS: u64 = 40;

#[derive(Debug, Clone, Serialize, PartialEq)]
// `rename_all` renames the variants; the fields inside them need `rename_all_fields`.
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "state")]
pub enum State {
    Running {
        pid: u32,
        started_at: u64,
        port: Option<u16>,
        /// Automatic restarts since the user started it.
        restarts: u32,
    },
    Stopping {
        pid: u32,
    },
    /// Not started yet: waiting for these dependencies to be ready.
    Waiting {
        on: Vec<String>,
    },
    /// Crashed with auto-restart on; the next run starts at `at` (epoch ms).
    Restarting {
        attempt: u32,
        at: u64,
    },
    /// Ended by itself. `code` is None when killed by a signal.
    Exited {
        code: Option<u32>,
        ended_at: u64,
        /// Auto-restart stopped trying after too many crashes.
        gave_up: bool,
    },
    /// Ended because the user stopped it.
    Stopped,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Event {
    Output { id: ServiceId, data: String },
    Status { id: ServiceId, status: State },
}

pub type Sink = Arc<dyn Fn(Event) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum StopSignal {
    #[default]
    #[serde(rename = "SIGTERM")]
    Term,
    #[serde(rename = "SIGINT")]
    Int,
    #[serde(rename = "SIGHUP")]
    Hup,
}

#[derive(Debug, Clone)]
pub struct Spec {
    pub command: String,
    pub cwd: String,
    /// Extra variables on top of the base environment.
    pub env: HashMap<String, String>,
    pub shell: String,
    pub stop_signal: StopSignal,
    pub grace: Duration,
    pub auto_restart: bool,
}

impl Spec {
    /// A spec with the default behaviour: `/bin/sh`, SIGTERM, 5 s grace, no auto-restart.
    #[cfg(test)]
    pub fn new(command: impl Into<String>, cwd: impl Into<String>) -> Self {
        Spec {
            command: command.into(),
            cwd: cwd.into(),
            env: HashMap::new(),
            shell: default_shell().into(),
            stop_signal: StopSignal::Term,
            grace: Duration::from_secs(5),
            auto_restart: false,
        }
    }
}

struct Proc {
    pid: u32,
    state: State,
    spec: Spec,
    size: (u16, u16),
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    stop_requested: Arc<AtomicBool>,
    /// Signalled by the waiter thread when the child exits.
    exited: Arc<(Mutex<bool>, Condvar)>,
}

#[derive(Default)]
struct Output {
    buffer: Vec<u8>,
    /// Not yet sent to the sink.
    pending: Vec<u8>,
}

struct Inner {
    procs: Mutex<HashMap<ServiceId, Proc>>,
    /// Kept after a process ends, so its last output can still be read.
    output: Mutex<HashMap<ServiceId, Output>>,
    /// The last state of services that are no longer running (or waiting to restart).
    finished: Mutex<HashMap<ServiceId, State>>,
    /// Bumped by every user start or stop; a pending auto-restart only runs if it's unchanged.
    generation: Mutex<HashMap<ServiceId, u64>>,
    crashes: Mutex<HashMap<ServiceId, Vec<Instant>>>,
    /// When the user last typed Ctrl-C into a service; an exit right after isn't a crash.
    interrupted: Mutex<HashMap<ServiceId, Instant>>,
    env: HashMap<String, String>,
    sink: Sink,
}

#[derive(Clone)]
pub struct Manager(Arc<Inner>);

impl Manager {
    pub fn new(env: HashMap<String, String>, sink: Sink) -> Self {
        let m = Manager(Arc::new(Inner {
            procs: Mutex::default(),
            output: Mutex::default(),
            finished: Mutex::default(),
            generation: Mutex::default(),
            crashes: Mutex::default(),
            interrupted: Mutex::default(),
            env,
            sink,
        }));
        let weak = Arc::downgrade(&m.0);
        std::thread::spawn(move || {
            while let Some(inner) = weak.upgrade() {
                Manager(inner).flush();
                std::thread::sleep(FLUSH);
            }
        });
        m
    }

    fn bump(&self, id: ServiceId) -> u64 {
        let mut g = self.0.generation.lock().unwrap();
        let n = g.entry(id).or_default();
        *n += 1;
        *n
    }

    pub fn generation(&self, id: ServiceId) -> u64 {
        self.0.generation.lock().unwrap().get(&id).copied().unwrap_or(0)
    }

    pub fn is_running(&self, id: ServiceId) -> bool {
        self.0.procs.lock().unwrap().contains_key(&id)
    }

    /// Show a service as waiting for its dependencies. Returns the generation to check
    /// afterwards: if it changed, the user pressed Stop and the start is cancelled.
    pub fn mark_waiting(&self, id: ServiceId, on: Vec<String>) -> u64 {
        let generation = self.bump(id);
        let state = State::Waiting { on };
        self.0.finished.lock().unwrap().insert(id, state.clone());
        (self.0.sink)(Event::Status { id, status: state });
        generation
    }

    /// Leave the waiting state without starting (a dependency failed).
    pub fn clear_waiting(&self, id: ServiceId) {
        let mut finished = self.0.finished.lock().unwrap();
        if matches!(finished.get(&id), Some(State::Waiting { .. })) {
            finished.insert(id, State::Stopped);
            drop(finished);
            (self.0.sink)(Event::Status { id, status: State::Stopped });
        }
    }

    /// Start `spec` as service `id`. Starting a running service does nothing.
    pub fn start(&self, id: ServiceId, spec: &Spec, size: (u16, u16)) -> Result<(), String> {
        if self.is_running(id) {
            return Ok(());
        }
        self.bump(id);
        self.0.crashes.lock().unwrap().remove(&id);
        self.launch(id, spec.clone(), size, 0)
    }

    fn launch(&self, id: ServiceId, spec: Spec, size: (u16, u16), restarts: u32) -> Result<(), String> {
        let mut procs = self.0.procs.lock().unwrap();
        if procs.contains_key(&id) {
            return Ok(());
        }
        if !std::path::Path::new(&spec.cwd).is_dir() {
            return Err(format!("Folder not found: {}", spec.cwd));
        }

        let pair = native_pty_system()
            .openpty(PtySize { cols: size.0, rows: size.1, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("Could not open a terminal: {e}"))?;

        let mut cmd = CommandBuilder::new(&spec.shell);
        // No `exec` prefix: it would break compound commands (`a && b`, `x; exit 3`).
        cmd.args(shell_args(&spec.shell, &spec.command));
        cmd.cwd(&spec.cwd);
        cmd.env_clear();
        for (k, v) in self.0.env.iter().chain(spec.env.iter()) {
            cmd.env(k, v);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        if !spec.env.contains_key("FORCE_COLOR") {
            cmd.env("FORCE_COLOR", "1");
        }

        let mut child = pair.slave.spawn_command(cmd).map_err(|e| format!("Could not start with {}: {e}", spec.shell))?;
        drop(pair.slave);
        let pid = child.process_id().unwrap_or(0);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let state = State::Running { pid, started_at: now(), port: None, restarts };
        let stop_requested = Arc::new(AtomicBool::new(false));
        let exited = Arc::new((Mutex::new(false), Condvar::new()));
        let command = spec.command.clone();
        // A Stop after this point changes the generation and so cancels any auto-restart.
        let launch_generation = self.generation(id);
        procs.insert(
            id,
            Proc {
                pid,
                state: state.clone(),
                spec,
                size,
                master: pair.master,
                writer,
                stop_requested: stop_requested.clone(),
                exited: exited.clone(),
            },
        );
        drop(procs);

        {
            let mut output = self.0.output.lock().unwrap();
            let out = output.entry(id).or_default();
            // A fresh start begins a clean log; an automatic restart keeps the crash above it.
            if restarts == 0 {
                out.buffer.clear();
                out.pending.clear();
            }
            let banner = if restarts == 0 {
                format!("\x1b[2m$ {command}\x1b[0m\r\n")
            } else {
                format!("\r\n\x1b[33m↻ restart {restarts}\x1b[0m \x1b[2m$ {command}\x1b[0m\r\n")
            };
            out.buffer.extend_from_slice(banner.as_bytes());
            out.pending.extend_from_slice(banner.as_bytes());
        }
        self.0.finished.lock().unwrap().remove(&id);
        (self.0.sink)(Event::Status { id, status: state });

        // Reader: PTY output into the buffer, port detection on the way.
        let me = self.clone();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            let mut seen = String::new();
            let mut port_found = false;
            while let Ok(n) = reader.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                me.append(id, &chunk[..n]);
                if !port_found {
                    seen.push_str(&String::from_utf8_lossy(&chunk[..n]));
                    if let Some(port) = detect_port(&seen) {
                        port_found = true;
                        me.set_port(id, port);
                    } else if seen.len() > 64 * 1024 {
                        seen.drain(..seen.len() - 4096);
                    }
                }
            }
        });

        // Waiter: owns the child, records how it ended, schedules an auto-restart.
        let me = self.clone();
        std::thread::spawn(move || {
            let status = child.wait();
            let code = status.ok().map(|s| s.exit_code());
            let proc = me.0.procs.lock().unwrap().remove(&id);
            let stopped = stop_requested.load(Ordering::SeqCst) || me.generation(id) != launch_generation;
            // Ctrl-C typed into the terminal is the user stopping it, not a crash.
            let interrupted = me.0.interrupted.lock().unwrap().remove(&id).is_some_and(|t| t.elapsed() < Duration::from_secs(3));
            let state = match &proc {
                _ if stopped => State::Stopped,
                Some(p) if p.spec.auto_restart && code != Some(0) && !interrupted => me.plan_restart(id, p, restarts, launch_generation),
                _ => State::Exited { code, ended_at: now(), gave_up: false },
            };
            me.0.finished.lock().unwrap().insert(id, state.clone());
            me.flush();
            (me.0.sink)(Event::Status { id, status: state });
            let (lock, cvar) = &*exited;
            *lock.lock().unwrap() = true;
            cvar.notify_all();
        });
        Ok(())
    }

    /// After a crash with auto-restart on: either a Restarting state with a timer that
    /// relaunches, or Exited with `gave_up` once the crash budget is spent.
    fn plan_restart(&self, id: ServiceId, p: &Proc, restarts: u32, generation: u64) -> State {
        let recent = {
            let mut crashes = self.0.crashes.lock().unwrap();
            let list = crashes.entry(id).or_default();
            list.push(Instant::now());
            list.retain(|t| t.elapsed() < CRASH_WINDOW);
            list.len()
        };
        if recent >= MAX_CRASHES {
            let note = format!("\r\n\x1b[31m✕ crashed {recent} times in {} s, not restarting again\x1b[0m\r\n", CRASH_WINDOW.as_secs());
            self.append(id, note.as_bytes());
            return State::Exited { code: None, ended_at: now(), gave_up: true };
        }
        let delay = Duration::from_millis((BACKOFF_BASE_MS << (recent - 1).min(5)).min(30_000));
        let attempt = restarts + 1;
        let note = format!("\r\n\x1b[33m↻ crashed, restarting in {:.1} s\x1b[0m\r\n", delay.as_secs_f32());
        self.append(id, note.as_bytes());

        let (spec, size) = (p.spec.clone(), p.size);
        let me = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            // A user start or stop in the meantime cancels this restart.
            if me.generation(id) == generation && !me.is_running(id)
                && let Err(e) = me.launch(id, spec, size, attempt) {
                    let state = State::Exited { code: None, ended_at: now(), gave_up: true };
                    me.append(id, format!("\r\n\x1b[31m✕ {e}\x1b[0m\r\n").as_bytes());
                    me.0.finished.lock().unwrap().insert(id, state.clone());
                    (me.0.sink)(Event::Status { id, status: state });
                }
        });
        State::Restarting { attempt, at: now() + delay.as_millis() as u64 }
    }

    /// Stop the service's whole process group with its stop signal, then SIGKILL after its
    /// grace period. Returns once it has exited. Also cancels a pending auto-restart.
    pub fn stop(&self, id: ServiceId) {
        self.bump(id);
        let found = {
            let mut procs = self.0.procs.lock().unwrap();
            procs.get_mut(&id).map(|p| {
                p.stop_requested.store(true, Ordering::SeqCst);
                p.state = State::Stopping { pid: p.pid };
                (p.pid, p.exited.clone(), p.spec.stop_signal, p.spec.grace)
            })
        };
        let Some((pid, exited, signal, grace)) = found else {
            // Not running: a pending restart or dependency wait is cancelled by the bump.
            let mut finished = self.0.finished.lock().unwrap();
            if matches!(finished.get(&id), Some(State::Restarting { .. } | State::Waiting { .. })) {
                finished.insert(id, State::Stopped);
                drop(finished);
                (self.0.sink)(Event::Status { id, status: State::Stopped });
            }
            return;
        };
        (self.0.sink)(Event::Status { id, status: State::Stopping { pid } });
        signal_group(pid, signal.into());
        if !wait_exit(&exited, grace) {
            signal_group(pid, Signal::Kill);
            wait_exit(&exited, Duration::from_secs(2));
        }
        // Anything the leader left behind in its group (it may have exited first).
        signal_group(pid, Signal::Kill);
    }

    /// Stop everything, in parallel. Used on quit and when rouse is told to exit.
    pub fn stop_all(&self) {
        let mut ids: Vec<_> = self.0.procs.lock().unwrap().keys().copied().collect();
        ids.extend(
            self.0.finished.lock().unwrap().iter().filter(|(_, s)| matches!(s, State::Restarting { .. } | State::Waiting { .. })).map(|(id, _)| *id),
        );
        let threads: Vec<_> = ids
            .into_iter()
            .map(|id| {
                let me = self.clone();
                std::thread::spawn(move || me.stop(id))
            })
            .collect();
        for t in threads {
            let _ = t.join();
        }
    }

    #[cfg(test)]
    pub fn state(&self, id: ServiceId) -> Option<State> {
        if let Some(p) = self.0.procs.lock().unwrap().get(&id) {
            return Some(p.state.clone());
        }
        self.0.finished.lock().unwrap().get(&id).cloned()
    }

    /// Every known state, running or finished.
    pub fn states(&self) -> HashMap<ServiceId, State> {
        let mut all = self.0.finished.lock().unwrap().clone();
        for (id, p) in self.0.procs.lock().unwrap().iter() {
            all.insert(*id, p.state.clone());
        }
        all
    }

    /// Process groups still running, for crash recovery (see `orphans`).
    pub fn running_pids(&self) -> Vec<u32> {
        self.0.procs.lock().unwrap().values().map(|p| p.pid).collect()
    }

    /// The buffered output, as text the terminal view can replay.
    pub fn output(&self, id: ServiceId) -> String {
        let output = self.0.output.lock().unwrap();
        output.get(&id).map(|o| String::from_utf8_lossy(&o.buffer).into_owned()).unwrap_or_default()
    }

    pub fn clear_output(&self, id: ServiceId) {
        if let Some(o) = self.0.output.lock().unwrap().get_mut(&id) {
            o.buffer.clear();
            o.pending.clear();
        }
    }

    /// Keystrokes from the terminal view (Ctrl-C, answers to prompts).
    pub fn write(&self, id: ServiceId, data: &[u8]) {
        if data.contains(&0x03) {
            self.0.interrupted.lock().unwrap().insert(id, Instant::now());
        }
        if let Some(p) = self.0.procs.lock().unwrap().get_mut(&id) {
            let _ = p.writer.write_all(data);
            let _ = p.writer.flush();
        }
    }

    pub fn resize(&self, id: ServiceId, cols: u16, rows: u16) {
        if let Some(p) = self.0.procs.lock().unwrap().get(&id) {
            let _ = p.master.resize(PtySize { cols, rows, pixel_width: 0, pixel_height: 0 });
        }
    }

    fn append(&self, id: ServiceId, data: &[u8]) {
        let mut output = self.0.output.lock().unwrap();
        let out = output.entry(id).or_default();
        out.buffer.extend_from_slice(data);
        if out.buffer.len() > BUFFER {
            let cut = out.buffer.len() - BUFFER;
            out.buffer.drain(..cut);
        }
        out.pending.extend_from_slice(data);
    }

    fn set_port(&self, id: ServiceId, port: u16) {
        let mut procs = self.0.procs.lock().unwrap();
        if let Some(p) = procs.get_mut(&id)
            && let State::Running { port: slot, .. } = &mut p.state {
                *slot = Some(port);
                let status = p.state.clone();
                drop(procs);
                (self.0.sink)(Event::Status { id, status });
            }
    }

    /// Send pending output, keeping incomplete UTF-8 sequences for the next batch.
    fn flush(&self) {
        let batches: Vec<(ServiceId, String)> = {
            let mut output = self.0.output.lock().unwrap();
            output
                .iter_mut()
                .filter(|(_, o)| !o.pending.is_empty())
                .map(|(id, o)| {
                    let valid = match std::str::from_utf8(&o.pending) {
                        Ok(_) => o.pending.len(),
                        Err(e) if e.error_len().is_none() => e.valid_up_to(),
                        Err(_) => o.pending.len(),
                    };
                    let bytes: Vec<u8> = o.pending.drain(..valid).collect();
                    (*id, String::from_utf8_lossy(&bytes).into_owned())
                })
                .filter(|(_, s)| !s.is_empty())
                .collect()
        };
        for (id, data) in batches {
            (self.0.sink)(Event::Output { id, data });
        }
    }
}

enum Signal {
    Term,
    Int,
    Hup,
    Kill,
}

impl From<StopSignal> for Signal {
    fn from(s: StopSignal) -> Self {
        match s {
            StopSignal::Term => Signal::Term,
            StopSignal::Int => Signal::Int,
            StopSignal::Hup => Signal::Hup,
        }
    }
}

#[cfg(unix)]
fn signal_group(pid: u32, signal: Signal) {
    if pid == 0 {
        return;
    }
    let sig = match signal {
        Signal::Term => libc::SIGTERM,
        Signal::Int => libc::SIGINT,
        Signal::Hup => libc::SIGHUP,
        Signal::Kill => libc::SIGKILL,
    };
    // The child is a session (and process group) leader, so -pid is its whole tree.
    unsafe {
        libc::kill(-(pid as i32), sig);
    }
}

/// Windows has no process groups or signals: `taskkill /T` ends the whole tree, politely
/// first (console tools often refuse that), then with `/F` once the grace period is over.
#[cfg(windows)]
fn signal_group(pid: u32, signal: Signal) {
    use std::os::windows::process::CommandExt;
    if pid == 0 {
        return;
    }
    let mut cmd = std::process::Command::new("taskkill");
    cmd.args(["/PID", &pid.to_string(), "/T"]);
    if matches!(signal, Signal::Kill) {
        cmd.arg("/F");
    }
    // CREATE_NO_WINDOW: no console flashing up on every Stop.
    cmd.creation_flags(0x0800_0000).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let _ = cmd.status();
}

/// The shell commands run in when no override sets one.
pub fn default_shell() -> &'static str {
    if cfg!(windows) { "cmd.exe" } else { "/bin/sh" }
}

/// How each shell takes a command line: `cmd /C`, `pwsh -Command`, everything else `-c`.
fn shell_args(shell: &str, command: &str) -> Vec<String> {
    // Split on both separators so Windows paths parse the same everywhere.
    let file = shell.rsplit(['/', '\\']).next().unwrap_or(shell).to_ascii_lowercase();
    let name = file.strip_suffix(".exe").unwrap_or(&file);
    match name {
        "cmd" => vec!["/C".into(), command.into()],
        "powershell" | "pwsh" => vec!["-NoProfile".into(), "-Command".into(), command.into()],
        _ => vec!["-c".into(), command.into()],
    }
}

fn wait_exit(exited: &(Mutex<bool>, Condvar), timeout: Duration) -> bool {
    let (lock, cvar) = exited;
    let deadline = Instant::now() + timeout;
    let mut done = lock.lock().unwrap();
    while !*done {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        done = cvar.wait_timeout(done, left).unwrap().0;
    }
    true
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// The port a dev server announces, e.g. `http://localhost:5173/`.
///
/// A local `http(s)://` URL wins. A bare `host:port` counts only on a line that announces
/// listening, so `ECONNREFUSED 127.0.0.1:5432` or "connecting to localhost:6379" never
/// becomes the service's link.
pub fn detect_port(text: &str) -> Option<u16> {
    use std::sync::OnceLock;
    static ANSI: OnceLock<Regex> = OnceLock::new();
    static URL: OnceLock<Regex> = OnceLock::new();
    static BARE: OnceLock<Regex> = OnceLock::new();
    static ANNOUNCE: OnceLock<Regex> = OnceLock::new();
    static NOISE: OnceLock<Regex> = OnceLock::new();
    let ansi = ANSI.get_or_init(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").unwrap());
    let url = URL.get_or_init(|| Regex::new(r"(?i)https?://(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1?\]):(\d{2,5})\b").unwrap());
    let bare = BARE.get_or_init(|| Regex::new(r"(?i)(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1?\]|\*):(\d{2,5})\b").unwrap());
    let announce = ANNOUNCE.get_or_init(|| Regex::new(r"(?i)listen|running on|started on|serving|server (?:is )?(?:running|started|ready)|available on|ready on").unwrap());
    let noise = NOISE.get_or_init(|| Regex::new(r"(?i)error|refused|failed|connect(?:ing|ed)? to|timeout|database|redis|postgres|mongo").unwrap());

    let plain = ansi.replace_all(text, "");
    for line in plain.lines() {
        if noise.is_match(line) {
            continue;
        }
        if let Some(c) = url.captures(line) {
            return c[1].parse().ok();
        }
        if announce.is_match(line)
            && let Some(c) = bare.captures(line) {
                return c[1].parse().ok();
            }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn manager() -> (Manager, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let sink: Sink = Arc::new(move |e| {
            let _ = tx.lock().unwrap().send(e);
        });
        (Manager::new(std::env::vars().collect(), sink), rx)
    }

    fn spec(command: &str) -> Spec {
        Spec::new(command, std::env::temp_dir().display().to_string())
    }

    fn alive(pid: i32) -> bool {
        // Zombies count as gone.
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => !stat.split(") ").nth(1).is_some_and(|rest| rest.starts_with('Z')),
            Err(_) => false,
        }
    }

    fn wait_until(mut f: impl FnMut() -> bool, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        f()
    }

    #[test]
    fn captures_output_with_a_tty() {
        let (m, _rx) = manager();
        m.start(1, &spec("[ -t 1 ] && echo is-a-tty; echo done"), (80, 24)).unwrap();
        assert!(wait_until(|| matches!(m.state(1), Some(State::Exited { .. })), Duration::from_secs(5)));
        let out = m.output(1);
        assert!(out.contains("is-a-tty"), "{out}");
        assert!(matches!(m.state(1), Some(State::Exited { code: Some(0), .. })));
    }

    #[test]
    fn stop_kills_grandchildren() {
        let (m, _rx) = manager();
        m.start(2, &spec("sh -c 'sleep 300 & echo $! > /tmp/rouse-test-grandchild; wait'"), (80, 24)).unwrap();
        assert!(wait_until(|| std::path::Path::new("/tmp/rouse-test-grandchild").exists(), Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(200));
        let grandchild: i32 = std::fs::read_to_string("/tmp/rouse-test-grandchild").unwrap().trim().parse().unwrap();
        let _ = std::fs::remove_file("/tmp/rouse-test-grandchild");
        assert!(alive(grandchild));
        m.stop(2);
        assert!(wait_until(|| !alive(grandchild), Duration::from_secs(3)), "grandchild {grandchild} survived");
        assert_eq!(m.state(2), Some(State::Stopped));
    }

    #[test]
    fn stop_escalates_when_sigterm_is_ignored() {
        let (m, _rx) = manager();
        m.start(3, &spec("trap '' TERM; while true; do sleep 1; done"), (80, 24)).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let started = Instant::now();
        m.stop(3);
        assert!(started.elapsed() >= Duration::from_secs(5), "should have waited the grace period");
        assert_eq!(m.state(3), Some(State::Stopped));
    }

    #[test]
    fn exit_code_and_double_start() {
        let (m, _rx) = manager();
        m.start(4, &spec("sleep 0.5; exit 3"), (80, 24)).unwrap();
        // A second start while running is a no-op.
        m.start(4, &spec("echo should-not-run"), (80, 24)).unwrap();
        assert!(wait_until(|| matches!(m.state(4), Some(State::Exited { .. })), Duration::from_secs(5)));
        assert!(matches!(m.state(4), Some(State::Exited { code: Some(3), .. })));
        assert!(!m.output(4).contains("should-not-run"));
    }

    #[test]
    fn detects_port_and_emits_batched_output() {
        let (m, rx) = manager();
        m.start(5, &spec("printf '\\033[32m  ➜  Local:   http://localhost:5173/\\033[0m\\n'; sleep 1"), (80, 24)).unwrap();
        assert!(wait_until(
            || matches!(m.state(5), Some(State::Running { port: Some(5173), .. }) | Some(State::Exited { .. })),
            Duration::from_secs(3)
        ));
        let events: Vec<Event> = rx.try_iter().collect();
        assert!(events.iter().any(|e| matches!(e, Event::Status { status: State::Running { port: Some(5173), .. }, .. })));
        assert!(events.iter().any(|e| matches!(e, Event::Output { data, .. } if data.contains("localhost:5173"))));
    }

    #[test]
    fn port_patterns() {
        assert_eq!(detect_port("Nest application started on http://127.0.0.1:3000"), Some(3000));
        assert_eq!(detect_port("Server listening on [::]:8080"), Some(8080));
        assert_eq!(detect_port("compiled successfully"), None);
        // A backend that logs its database before its own address.
        let backend = "Error: connect ECONNREFUSED 127.0.0.1:5432\nConnecting to redis localhost:6379\nServer ready on http://localhost:3000";
        assert_eq!(detect_port(backend), Some(3000));
        assert_eq!(detect_port("ECONNREFUSED 127.0.0.1:5432"), None);
    }

    #[test]
    fn missing_folder_is_an_error() {
        let (m, _rx) = manager();
        let mut s = spec("true");
        s.cwd = "/definitely/not/here".into();
        assert!(m.start(6, &s, (80, 24)).is_err());
    }
}

#[cfg(test)]
mod wire {
    use super::*;

    /// The window reads these exact keys (src/services/tauri.ts).
    #[test]
    fn state_json_matches_the_frontend() {
        let running = serde_json::to_value(State::Running { pid: 7, started_at: 1, port: Some(5173), restarts: 0 }).unwrap();
        assert_eq!(running, serde_json::json!({ "state": "running", "pid": 7, "startedAt": 1, "port": 5173, "restarts": 0 }));
        let exited = serde_json::to_value(State::Exited { code: Some(1), ended_at: 2, gave_up: false }).unwrap();
        assert_eq!(exited, serde_json::json!({ "state": "exited", "code": 1, "endedAt": 2, "gaveUp": false }));
        let restarting = serde_json::to_value(State::Restarting { attempt: 2, at: 9 }).unwrap();
        assert_eq!(restarting, serde_json::json!({ "state": "restarting", "attempt": 2, "at": 9 }));
        assert_eq!(serde_json::to_value(StopSignal::Int).unwrap(), serde_json::json!("SIGINT"));
        let event = serde_json::to_value(Event::Output { id: 3, data: "x".into() }).unwrap();
        assert_eq!(event, serde_json::json!({ "kind": "output", "id": 3, "data": "x" }));
    }
}

#[cfg(test)]
mod behaviour {
    use super::*;

    fn manager() -> Manager {
        Manager::new(std::env::vars().collect(), Arc::new(|_| {}))
    }

    fn spec(command: &str) -> Spec {
        Spec::new(command, std::env::temp_dir().display().to_string())
    }

    fn wait_for(m: &Manager, id: ServiceId, timeout: Duration, f: impl Fn(&State) -> bool) -> Option<State> {
        let deadline = Instant::now() + timeout;
        loop {
            let st = m.states().get(&id).cloned();
            if let Some(s) = &st && f(s) {
                return st;
            }
            if Instant::now() > deadline {
                return st;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn custom_stop_signal_is_used() {
        let m = manager();
        // Ignores SIGTERM, exits cleanly on SIGINT.
        let mut s = spec("trap '' TERM; trap 'exit 0' INT; while true; do sleep 0.1; done");
        s.stop_signal = StopSignal::Int;
        m.start(1, &s, (80, 24)).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let t = Instant::now();
        m.stop(1);
        assert!(t.elapsed() < Duration::from_secs(2), "SIGINT should stop it without waiting the grace period");
    }

    #[test]
    fn short_grace_escalates_sooner() {
        let m = manager();
        let mut s = spec("trap '' TERM; while true; do sleep 0.1; done");
        s.grace = Duration::from_millis(500);
        m.start(2, &s, (80, 24)).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let t = Instant::now();
        m.stop(2);
        assert!(t.elapsed() < Duration::from_secs(2));
        assert_eq!(m.states().get(&2), Some(&State::Stopped));
    }

    #[test]
    fn shell_override() {
        let m = manager();
        let mut s = spec("echo \"shell=$0\"");
        s.shell = "/bin/bash".into();
        m.start(3, &s, (80, 24)).unwrap();
        wait_for(&m, 3, Duration::from_secs(3), |s| matches!(s, State::Exited { .. }));
        assert!(m.output(3).contains("shell=/bin/bash"), "{}", m.output(3));
    }

    #[test]
    fn auto_restart_backs_off_then_gives_up() {
        let m = manager();
        let mut s = spec("echo run; exit 1");
        s.auto_restart = true;
        m.start(4, &s, (80, 24)).unwrap();
        let end = wait_for(&m, 4, Duration::from_secs(10), |s| matches!(s, State::Exited { gave_up: true, .. }));
        assert!(matches!(end, Some(State::Exited { gave_up: true, .. })), "{end:?}");
        let out = m.output(4);
        // Count the command's own output lines, not the `$ echo run` banners.
        assert_eq!(out.matches("run\r\n").count(), MAX_CRASHES, "one run per allowed crash:\n{out}");
        assert!(out.contains("restart 4") && out.contains("not restarting again"));
    }

    #[test]
    fn stop_cancels_a_pending_restart() {
        let m = manager();
        let mut s = spec("sleep 0.2; exit 1");
        s.auto_restart = true;
        m.start(5, &s, (80, 24)).unwrap();
        wait_for(&m, 5, Duration::from_secs(3), |s| matches!(s, State::Restarting { .. }));
        m.stop(5);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(m.states().get(&5), Some(&State::Stopped));
        assert!(!m.is_running(5));
    }

    #[test]
    fn clean_exit_is_not_restarted() {
        let m = manager();
        let mut s = spec("true");
        s.auto_restart = true;
        m.start(6, &s, (80, 24)).unwrap();
        let end = wait_for(&m, 6, Duration::from_secs(2), |s| matches!(s, State::Exited { .. }));
        assert!(matches!(end, Some(State::Exited { code: Some(0), gave_up: false, .. })));
    }

    #[test]
    fn ctrl_c_is_not_a_crash() {
        let m = manager();
        let mut s = spec("trap 'exit 130' INT; while true; do sleep 0.1; done");
        s.auto_restart = true;
        m.start(7, &s, (80, 24)).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        m.write(7, b"\x03");
        let end = wait_for(&m, 7, Duration::from_secs(3), |s| matches!(s, State::Exited { .. } | State::Restarting { .. }));
        assert!(matches!(end, Some(State::Exited { gave_up: false, .. })), "{end:?}");
    }

    #[test]
    fn waiting_can_be_cancelled_by_stop() {
        let m = manager();
        let generation = m.mark_waiting(8, vec!["db".into()]);
        assert_eq!(m.states().get(&8), Some(&State::Waiting { on: vec!["db".into()] }));
        m.stop(8);
        assert_ne!(m.generation(8), generation);
        assert_eq!(m.states().get(&8), Some(&State::Stopped));
    }

    #[test]
    fn shells_get_their_own_command_flag() {
        assert_eq!(shell_args("/bin/zsh", "x"), ["-c", "x"]);
        assert_eq!(shell_args("C:\\Windows\\System32\\cmd.exe", "x"), ["/C", "x"]);
        assert_eq!(shell_args("pwsh", "x"), ["-NoProfile", "-Command", "x"]);
    }
}
