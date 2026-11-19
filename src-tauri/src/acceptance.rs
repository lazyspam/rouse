//! End to end without the window: scan a real folder, import it, start a whole group with
//! the real login environment, watch output and ports, stop it, check nothing is left.
//! `ROUSE_ACCEPT=~/code/shop cargo test --lib acceptance -- --ignored --nocapture`

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use crate::procs::{Event, Manager, Spec, State};
    use crate::store::{ImportItem, NewService, Store};
    use crate::{env, scan};

    fn port_free(port: u16) -> bool {
        std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(200)).is_err()
    }

    #[test]
    #[ignore]
    fn scan_import_start_stop_group() {
        let root = std::env::var("ROUSE_ACCEPT").expect("set ROUSE_ACCEPT");
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("t.db")).unwrap();

        let found = scan::scan(std::path::Path::new(&root));
        let items: Vec<ImportItem> = found
            .iter()
            .map(|c| ImportItem {
                path: c.path.clone(),
                name: c.name.clone(),
                groups: c.groups.clone(),
                group_paths: c.group_paths.clone(),
                services: c.suggestions.iter().filter(|s| s.recommended).map(|s| NewService { name: s.name.clone(), command: s.command.clone() }).collect(),
            })
            .collect();
        let imported = store.import(&items).unwrap();
        println!("imported {imported:?}");
        let tree = store.tree().unwrap();
        for s in &tree.services {
            println!("  service {} `{}`", s.name, s.command);
        }
        let group = tree.groups.iter().find(|g| g.parent_id.is_none()).unwrap();

        let events: Arc<Mutex<Vec<Event>>> = Arc::default();
        let sink_events = events.clone();
        let procs = Manager::new(
            env::login_env(Duration::from_secs(10)),
            Arc::new(move |e| sink_events.lock().unwrap().push(e)),
        );

        let ids = store.group_services(group.id).unwrap();
        for id in &ids {
            let svc = store.service(*id).unwrap();
            let spec = (svc.command, store.project_dir(*id).unwrap());
            procs.start(*id, &Spec::new(spec.0, spec.1), (120, 32)).unwrap();
        }

        // Give dev servers time to boot, then report what each did.
        let deadline = Instant::now() + Duration::from_secs(45);
        while Instant::now() < deadline {
            let states = procs.states();
            let settled = ids.iter().all(|id| matches!(states.get(id), Some(State::Running { port: Some(_), .. }) | Some(State::Exited { .. })));
            if settled {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let states = procs.states();
        let mut ports = Vec::new();
        for id in &ids {
            let name = &tree.services.iter().find(|s| s.id == *id).unwrap().command;
            let out = procs.output(*id);
            let tail: String = out.lines().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" | ");
            println!("{name}: {:?}\n    output tail: {}", states.get(id), tail.chars().take(300).collect::<String>());
            if let Some(State::Running { port: Some(p), .. }) = states.get(id) {
                ports.push(*p);
                assert!(!port_free(*p), "port {p} reported but not listening");
            }
        }
        let batches = events.lock().unwrap().iter().filter(|e| matches!(e, Event::Output { .. })).count();
        println!("output batches sent: {batches}; listening ports: {ports:?}");
        assert!(batches > 0);

        let pids = procs.running_pids();
        let stopped_at = Instant::now();
        procs.stop_all();
        println!("stop_all took {:?}", stopped_at.elapsed());
        std::thread::sleep(Duration::from_millis(500));
        for p in &ports {
            assert!(port_free(*p), "port {p} still taken after stop");
        }
        for pid in pids {
            let leftover = std::process::Command::new("pgrep").args(["-g", &pid.to_string()]).output().unwrap();
            assert!(leftover.stdout.is_empty(), "process group {pid} still has: {}", String::from_utf8_lossy(&leftover.stdout));
        }
        println!("all ports free, no process left in any group");
    }

    /// Import a folder's recommended commands into a real rouse database.
    /// `ROUSE_ACCEPT=<folder> ROUSE_DB=<db> cargo test --lib seed_db -- --ignored`
    #[test]
    #[ignore]
    fn seed_db() {
        let root = std::env::var("ROUSE_ACCEPT").expect("set ROUSE_ACCEPT");
        let db = std::env::var("ROUSE_DB").expect("set ROUSE_DB");
        let store = Store::open(std::path::Path::new(&db)).unwrap();
        store.add_root(&root).unwrap();
        let items: Vec<ImportItem> = scan::scan(std::path::Path::new(&root))
            .into_iter()
            .map(|c| ImportItem {
                services: c.suggestions.iter().filter(|s| s.recommended).map(|s| NewService { name: s.name.clone(), command: s.command.clone() }).collect(),
                path: c.path,
                name: c.name,
                groups: c.groups,
                group_paths: c.group_paths,
            })
            .collect();
        println!("{:?}", store.import(&items).unwrap());
    }
}
