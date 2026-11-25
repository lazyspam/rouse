//! Resource use: CPU and memory per running service, for rouse itself, and for the machine.
//!
//! A service is its whole process tree: the shell, `pnpm`, node, esbuild… On unix every
//! process in the service's session counts too (portable-pty starts the child with `setsid()`,
//! so the session id is the child's pid), which catches daemons that re-parented themselves
//! to init. Windows has no sessions, so there it is the descendants only.
//!
//! CPU is top-style: 100% is one core, so a busy tree can go past 100%. Memory is resident
//! (RSS) summed over the processes, which counts shared pages more than once, so it reads a
//! little high for big trees.
//!
//! This module has no Tauri types; the app samples on a timer and emits the result.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::procs::ServiceId;

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Percent of one core.
    pub cpu: f32,
    /// Resident memory in bytes.
    pub memory: u64,
    /// Processes counted.
    pub processes: u32,
}

impl Usage {
    fn add(&mut self, cpu: f32, memory: u64) {
        self.cpu += cpu;
        self.memory += memory;
        self.processes += 1;
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub services: HashMap<ServiceId, Usage>,
    /// rouse's own processes (the app and its webview helpers), services left out.
    pub app: Usage,
    /// Whole-machine CPU, 0–100 over all cores.
    pub system_cpu: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub cores: u32,
}

/// Keeps one `System` between samples: CPU use is the difference between two refreshes, so
/// the first sample reads 0 for everything.
pub struct Sampler {
    sys: System,
    me: Pid,
}

impl Sampler {
    pub fn new() -> Option<Self> {
        let sys = System::new().ok()?;
        Some(Sampler { sys, me: Pid::from_u32(std::process::id()) })
    }

    /// One sample. `services` are the running services and their leader pids.
    pub fn sample(&mut self, services: &[(ServiceId, u32)]) -> Snapshot {
        // CPU and memory only: no command lines, environments or per-thread entries (threads
        // listed as processes would each report the whole process's memory).
        let kind = ProcessRefreshKind::nothing().with_cpu().with_memory().without_tasks();
        self.sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();

        let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
        for (pid, p) in self.sys.processes() {
            if let Some(parent) = p.parent() {
                children.entry(parent).or_default().push(*pid);
            }
        }
        let tree = |root: Pid| -> HashSet<Pid> {
            let mut seen = HashSet::new();
            let mut stack = vec![root];
            while let Some(pid) = stack.pop() {
                if self.sys.process(pid).is_some() && seen.insert(pid) {
                    stack.extend(children.get(&pid).into_iter().flatten().copied());
                }
            }
            seen
        };

        // Each service's processes, by session (unix) and by descent.
        let mut owner: HashMap<Pid, ServiceId> = HashMap::new();
        for &(id, pid) in services {
            if pid != 0 {
                owner.extend(tree(Pid::from_u32(pid)).into_iter().map(|p| (p, id)));
            }
        }
        #[cfg(unix)]
        if !services.is_empty() {
            let leaders: HashMap<Pid, ServiceId> = services.iter().map(|(id, pid)| (Pid::from_u32(*pid), *id)).collect();
            for (p, proc_) in self.sys.processes() {
                if let Some(id) = proc_.session_id().and_then(|s| leaders.get(&s)) {
                    owner.insert(*p, *id);
                }
            }
        }

        let mut out: HashMap<ServiceId, Usage> = services.iter().map(|(id, _)| (*id, Usage::default())).collect();
        for (pid, id) in &owner {
            if let (Some(p), Some(u)) = (self.sys.process(*pid), out.get_mut(id)) {
                u.add(p.cpu_usage(), p.memory());
            }
        }

        let mut app = Usage::default();
        for pid in tree(self.me) {
            if owner.contains_key(&pid) {
                continue;
            }
            if let Some(p) = self.sys.process(pid) {
                app.add(p.cpu_usage(), p.memory());
            }
        }

        Snapshot {
            services: out,
            app,
            system_cpu: self.sys.global_cpu_usage(),
            memory_used: self.sys.used_memory(),
            memory_total: self.sys.total_memory(),
            cores: self.sys.cpus().len().max(1) as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procs::{Manager, Spec};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn counts_a_service_tree() {
        let m = Manager::new(std::env::vars().collect(), Arc::new(|_| {}));
        let spec = Spec::new("sleep 30 & sleep 30 & wait", std::env::temp_dir().display().to_string());
        m.start(1, &spec, (80, 24)).unwrap();
        std::thread::sleep(Duration::from_millis(400));
        let mut s = Sampler::new().unwrap();
        s.sample(&m.live_pids());
        let snap = s.sample(&m.live_pids());
        m.stop(1);
        let u = snap.services[&1];
        assert!(u.processes >= 3, "shell and both sleeps: {u:?}");
        assert!(u.memory > 0);
        // The service's processes are rouse's children here too, but not counted as rouse's.
        assert!(snap.app.processes >= 1);
        assert!(snap.memory_total >= snap.memory_used && snap.memory_total > 0);
    }

    #[test]
    fn json_matches_the_frontend() {
        let u = Usage { cpu: 1.5, memory: 2, processes: 3 };
        assert_eq!(serde_json::to_value(u).unwrap(), serde_json::json!({ "cpu": 1.5, "memory": 2, "processes": 3 }));
        let snap = Snapshot { services: HashMap::new(), app: u, system_cpu: 0.0, memory_used: 1, memory_total: 2, cores: 4 };
        let v = serde_json::to_value(snap).unwrap();
        for key in ["services", "app", "systemCpu", "memoryUsed", "memoryTotal", "cores"] {
            assert!(v.get(key).is_some(), "{key}");
        }
    }
}
