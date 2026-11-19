//! What the window can ask for. Thin wrappers over `store`, `scan`, `overrides` and `procs`.
//! Anything that can block (stopping waits up to the grace period, starting waits for
//! dependencies, scanning reads disks) runs off the main thread.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager as _, Runtime, State as TauriState};

use crate::overrides::{self, Effective, OneOff, Overrides};
use crate::procs::{Manager, ServiceId, State};
use crate::scan::{self, Candidate};
use crate::store::{Id, ImportItem, ImportResult, Service, Store, Tree};

pub struct AppState {
    pub store: Store,
    pub procs: Manager,
}

/// Terminal size for services started without a view open (group start, tray).
const DEFAULT_SIZE: (u16, u16) = (120, 32);

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn get_tree(state: TauriState<AppState>) -> Res<Tree> {
    state.store.tree().map_err(err)
}

#[tauri::command]
pub fn get_states(state: TauriState<AppState>) -> HashMap<ServiceId, State> {
    state.procs.states()
}

#[tauri::command]
pub async fn scan_folder(state: TauriState<'_, AppState>, path: String) -> Res<Vec<Candidate>> {
    let root = Path::new(&path);
    if !root.is_dir() {
        return Err(format!("Not a folder: {path}"));
    }
    state.store.add_root(&path).map_err(err)?;
    let root = root.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || scan::scan(&root)).await.map_err(err)
}

#[tauri::command]
pub fn import_projects(state: TauriState<AppState>, items: Vec<ImportItem>) -> Res<ImportResult> {
    state.store.import(&items).map_err(err)
}

/// How long a dependency may take to become ready.
const READY_TIMEOUT: Duration = Duration::from_secs(90);
/// A dependency without any port counts as ready after running this long.
const READY_WITHOUT_PORT: Duration = Duration::from_secs(3);

/// The cascade resolved for one service: command, folder, env, behaviour.
fn effective(state: &AppState, id: ServiceId, once: Option<&OneOff>) -> Res<Effective> {
    let service = state.store.service(id).map_err(err)?;
    let dir = state.store.project_dir(id).map_err(err)?;
    let cwd = match &service.cwd {
        Some(sub) => Path::new(&dir).join(sub).display().to_string(),
        None => dir.clone(),
    };
    let mut levels = state.store.levels(id).map_err(err)?;
    // Variables saved by the first command editor live on the service row; overrides win.
    if let Some(own) = levels.last_mut() {
        for (k, v) in service.env {
            own.overrides.env.entry(k).or_insert(v);
        }
    }
    let script = scan::script_body(Path::new(&dir), &service.command);
    Ok(overrides::resolve(&levels, &service.command, Path::new(&dir), &cwd, script.as_deref(), once))
}

/// Something accepts connections on the port, over IPv4 or IPv6 loopback.
fn port_in_use(port: u16) -> bool {
    let v4: std::net::SocketAddr = ([127, 0, 0, 1], port).into();
    let v6: std::net::SocketAddr = (std::net::Ipv6Addr::LOCALHOST, port).into();
    [v4, v6].iter().any(|addr| std::net::TcpStream::connect_timeout(addr, Duration::from_millis(300)).is_ok())
}

/// A group-start failure as one readable line: the `PORT_BUSY:<port>:` prefix the window
/// uses for its "start anyway" prompt is dropped; every other message stays whole.
fn plain_error(e: &str) -> &str {
    match e.strip_prefix("PORT_BUSY:") {
        Some(rest) => rest.split_once(':').map_or(rest, |(_, msg)| msg),
        None => e,
    }
}

/// Start one service. A busy port refuses with `PORT_BUSY:<port>:<message>` unless `force`.
fn start_one<R: Runtime>(app: &AppHandle<R>, id: ServiceId, size: (u16, u16), force: bool, once: Option<&OneOff>) -> Res<()> {
    let state = app.state::<AppState>();
    if state.procs.is_running(id) {
        return Ok(());
    }
    let eff = effective(&state, id, once)?;
    if let Some(p) = &eff.port
        && !force
        && port_in_use(p.value)
    {
        return Err(format!("PORT_BUSY:{0}:Port {0} is already in use.", p.value));
    }
    state.procs.start(id, &eff.spec(), size)?;
    crate::record_live(app);
    Ok(())
}

/// Start a service after its dependencies: each is started (if it isn't running) and
/// waited for until ready. Loops are reported instead of followed.
fn start_tree<R: Runtime>(app: &AppHandle<R>, id: ServiceId, size: (u16, u16), force: bool, once: Option<&OneOff>, chain: &mut Vec<ServiceId>) -> Res<()> {
    let state = app.state::<AppState>();
    let name = |id| state.store.service(id).map(|s| s.name).unwrap_or_else(|_| format!("#{id}"));
    if chain.contains(&id) {
        let path: Vec<String> = chain.iter().chain([&id]).map(|i| name(*i)).collect();
        return Err(format!("Dependency loop: {}", path.join(" → ")));
    }
    chain.push(id);
    let deps: Vec<ServiceId> = effective(&state, id, None)?.depends_on.into_iter().filter(|d| state.store.service(*d).is_ok()).collect();
    if !deps.is_empty() {
        // Show "Waiting for db…" until the dependencies are ready; Stop cancels.
        let generation = state.procs.mark_waiting(id, deps.iter().map(|d| name(*d)).collect());
        let cancelled = || state.procs.generation(id) != generation;
        for dep in deps {
            let result = (|| {
                if !state.procs.is_running(dep) {
                    // Already up outside rouse (your own Postgres on its port): nothing to start.
                    let external = effective(&state, dep, None)?.port.is_some_and(|p| port_in_use(p.value));
                    if external {
                        return Ok(());
                    }
                    start_tree(app, dep, DEFAULT_SIZE, force, None, chain)?;
                }
                wait_ready(app, dep, &cancelled)
            })();
            if let Err(e) = result {
                state.procs.clear_waiting(id);
                return Err(e);
            }
        }
        if cancelled() {
            return Err(format!("{} was stopped before it started", name(id)));
        }
    }
    chain.pop();
    start_one(app, id, size, force, once)
}

/// Ready = its port answers (the configured one, else the one it printed), or, with no
/// port at all, it has been running for a few seconds.
fn wait_ready<R: Runtime>(app: &AppHandle<R>, id: ServiceId, cancelled: &dyn Fn() -> bool) -> Res<()> {
    let state = app.state::<AppState>();
    let name = state.store.service(id).map(|s| s.name).unwrap_or_default();
    let configured = effective(&state, id, None)?.port.map(|p| p.value);
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        match state.procs.states().get(&id) {
            Some(State::Running { port, started_at, .. }) => match configured.or(*port) {
                Some(p) if port_in_use(p) => return Ok(()),
                Some(_) => {}
                None if now_ms().saturating_sub(*started_at) >= READY_WITHOUT_PORT.as_millis() as u64 => return Ok(()),
                None => {}
            },
            Some(State::Restarting { .. }) | Some(State::Stopping { .. }) => {}
            _ => return Err(format!("{name} stopped before it was ready")),
        }
        if cancelled() {
            return Err("Cancelled".into());
        }
        if Instant::now() > deadline {
            return Err(format!("{name} wasn't ready after {} s", READY_TIMEOUT.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Run blocking work off the async runtime's threads.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(err)?
}

#[tauri::command]
pub async fn start_service(
    app: AppHandle,
    id: ServiceId,
    cols: Option<u16>,
    rows: Option<u16>,
    force: Option<bool>,
    once: Option<OneOff>,
) -> Res<()> {
    let size = match (cols, rows) {
        (Some(c), Some(r)) if c > 0 && r > 0 => (c, r),
        _ => DEFAULT_SIZE,
    };
    blocking(move || start_tree(&app, id, size, force.unwrap_or(false), once.as_ref(), &mut Vec::new())).await
}

#[tauri::command]
pub async fn stop_service(app: AppHandle, state: TauriState<'_, AppState>, id: ServiceId) -> Res<()> {
    state.procs.stop(id);
    crate::record_live(&app);
    Ok(())
}

#[tauri::command]
pub async fn restart_service(app: AppHandle, id: ServiceId) -> Res<()> {
    blocking(move || {
        app.state::<AppState>().procs.stop(id);
        start_one(&app, id, DEFAULT_SIZE, true, None)
    })
    .await
}

/// Start every service in the list (with its dependencies). Returns the failures, one
/// line each, and keeps going past them.
fn start_many<R: Runtime>(app: &AppHandle<R>, ids: Vec<ServiceId>) -> Vec<String> {
    let state = app.state::<AppState>();
    ids.into_iter()
        .filter_map(|id| {
            start_tree(app, id, DEFAULT_SIZE, false, None, &mut Vec::new()).err().map(|e| {
                let name = state.store.service(id).map(|s| s.name).unwrap_or_default();
                format!("{name}: {}", plain_error(&e))
            })
        })
        .collect()
}

fn stop_many<R: Runtime>(state: &AppState, app: &AppHandle<R>, ids: Vec<ServiceId>) {
    let threads: Vec<_> = ids
        .into_iter()
        .map(|id| {
            let procs = state.procs.clone();
            std::thread::spawn(move || procs.stop(id))
        })
        .collect();
    for t in threads {
        let _ = t.join();
    }
    crate::record_live(app);
}

#[tauri::command]
pub async fn start_group(app: AppHandle, id: Id) -> Res<Vec<String>> {
    blocking(move || {
        let ids = app.state::<AppState>().store.group_services(id).map_err(err)?;
        Ok(start_many(&app, ids))
    })
    .await
}

#[tauri::command]
pub async fn stop_group(app: AppHandle, state: TauriState<'_, AppState>, id: Id) -> Res<()> {
    stop_many(&state, &app, state.store.group_services(id).map_err(err)?);
    Ok(())
}

#[tauri::command]
pub async fn start_project(app: AppHandle, id: Id) -> Res<Vec<String>> {
    blocking(move || {
        let ids = app.state::<AppState>().store.project_services(id).map_err(err)?;
        Ok(start_many(&app, ids))
    })
    .await
}

#[tauri::command]
pub async fn stop_project(app: AppHandle, state: TauriState<'_, AppState>, id: Id) -> Res<()> {
    stop_many(&state, &app, state.store.project_services(id).map_err(err)?);
    Ok(())
}

#[tauri::command]
pub async fn stop_all(app: AppHandle, state: TauriState<'_, AppState>) -> Res<()> {
    state.procs.stop_all();
    crate::record_live(&app);
    Ok(())
}

fn check_level(level: &str) -> Res<()> {
    match level {
        "global" | "group" | "project" | "service" => Ok(()),
        _ => Err(format!("Unknown level {level}")),
    }
}

#[tauri::command]
pub fn get_overrides(state: TauriState<AppState>, level: String, id: Id) -> Res<Overrides> {
    check_level(&level)?;
    state.store.overrides(&level, id).map_err(err)
}

#[tauri::command]
pub fn set_overrides(state: TauriState<AppState>, level: String, id: Id, overrides: Overrides) -> Res<()> {
    check_level(&level)?;
    let mut o = overrides;
    o.env.retain(|k, _| !k.trim().is_empty());
    if level == "service" {
        o.depends_on.retain(|d| *d != id);
    } else {
        // Ports and dependencies only mean something on a single command.
        o.port = None;
        o.depends_on.clear();
    }
    state.store.set_overrides(&level, id, &o).map_err(err)
}

/// Each project's logo as a data URL, read fresh from its index.html or favicon files.
#[tauri::command]
pub async fn get_project_icons(state: TauriState<'_, AppState>) -> Res<HashMap<Id, String>> {
    let projects: Vec<(Id, String)> = state.store.tree().map_err(err)?.projects.into_iter().map(|p| (p.id, p.path)).collect();
    blocking(move || Ok(projects.into_iter().filter_map(|(id, path)| crate::icons::project_icon(Path::new(&path)).map(|url| (id, url))).collect())).await
}

/// What a command would run with right now, every value with its source.
#[tauri::command]
pub fn get_effective(state: TauriState<AppState>, id: ServiceId) -> Res<Effective> {
    effective(&state, id, None)
}

/// Save a one-off run as its own command in the same project: the base command plus the
/// arguments, with the original's overrides and the extra variables.
#[tauri::command]
pub fn save_variant(state: TauriState<AppState>, id: ServiceId, name: String, args: String, env: BTreeMap<String, String>) -> Res<Id> {
    let base = state.store.service(id).map_err(err)?;
    let command = if args.trim().is_empty() { base.command.clone() } else { overrides::append_args(&base.command, args.trim()) };
    let new_id = state.store.add_service(base.project_id, name.trim(), &command).map_err(err)?;
    let mut o = state.store.overrides("service", id).map_err(err)?;
    o.env.extend(base.env);
    o.env.extend(env);
    state.store.set_overrides("service", new_id, &o).map_err(err)?;
    if base.cwd.is_some() {
        state.store.update_service(&Service { id: new_id, project_id: base.project_id, name: name.trim().into(), command, cwd: base.cwd, env: HashMap::new() }).map_err(err)?;
    }
    Ok(new_id)
}

#[tauri::command]
pub fn get_output(state: TauriState<AppState>, id: ServiceId) -> String {
    state.procs.output(id)
}

#[tauri::command]
pub fn clear_output(state: TauriState<AppState>, id: ServiceId) {
    state.procs.clear_output(id)
}

#[tauri::command]
pub fn write_input(state: TauriState<AppState>, id: ServiceId, data: String) {
    state.procs.write(id, data.as_bytes())
}

#[tauri::command]
pub fn resize_terminal(state: TauriState<AppState>, id: ServiceId, cols: u16, rows: u16) {
    state.procs.resize(id, cols, rows)
}

#[tauri::command]
pub fn create_group(state: TauriState<AppState>, name: String, parent_id: Option<Id>) -> Res<Id> {
    state.store.create_group(name.trim(), parent_id).map_err(err)
}

#[tauri::command]
pub fn rename_group(state: TauriState<AppState>, id: Id, name: String) -> Res<()> {
    state.store.rename_group(id, name.trim()).map_err(err)
}

#[tauri::command]
pub fn move_group(state: TauriState<AppState>, id: Id, parent_id: Option<Id>) -> Res<()> {
    state.store.move_group(id, parent_id)
}

#[tauri::command]
pub fn delete_group(state: TauriState<AppState>, id: Id) -> Res<()> {
    state.store.delete_group(id).map_err(err)
}

#[tauri::command]
pub fn set_collapsed(state: TauriState<AppState>, id: Id, collapsed: bool) -> Res<()> {
    state.store.set_collapsed(id, collapsed).map_err(err)
}

#[tauri::command]
pub fn add_project(state: TauriState<AppState>, path: String, group_id: Option<Id>) -> Res<Id> {
    let name = Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or("project").to_string();
    state.store.add_project(&path, &name, group_id).map_err(err)
}

#[tauri::command]
pub fn rename_project(state: TauriState<AppState>, id: Id, name: String) -> Res<()> {
    state.store.rename_project(id, name.trim()).map_err(err)
}

#[tauri::command]
pub fn move_project(state: TauriState<AppState>, id: Id, group_id: Option<Id>) -> Res<()> {
    state.store.move_project(id, group_id).map_err(err)
}

#[tauri::command]
pub async fn delete_project(app: AppHandle, state: TauriState<'_, AppState>, id: Id) -> Res<()> {
    stop_many(&state, &app, state.store.project_services(id).map_err(err)?);
    state.store.delete_project(id).map_err(err)
}

#[tauri::command]
pub fn add_service(state: TauriState<AppState>, project_id: Id, name: String, command: String) -> Res<Id> {
    state.store.add_service(project_id, name.trim(), command.trim()).map_err(err)
}

#[tauri::command]
pub fn update_service(state: TauriState<AppState>, service: Service) -> Res<()> {
    state.store.update_service(&service).map_err(err)
}

#[tauri::command]
pub async fn delete_service(app: AppHandle, state: TauriState<'_, AppState>, id: ServiceId) -> Res<()> {
    state.procs.stop(id);
    crate::record_live(&app);
    state.store.delete_service(id).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tauri::test::{mock_app, MockRuntime};

    fn app() -> (tauri::App<MockRuntime>, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(&tmp.path().join("t.db")).unwrap();
        let procs = Manager::new(std::env::vars().collect(), Arc::new(|_| {}));
        let app = mock_app();
        app.manage(AppState { store, procs });
        (app, tmp)
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    #[test]
    fn dependencies_start_first_and_are_waited_for() {
        let (app, tmp) = app();
        let h = app.handle();
        let state = h.state::<AppState>();
        let dir = tmp.path().display().to_string();
        let p = state.store.add_project(&dir, "p", None).unwrap();
        let port = free_port();
        // "db" opens its port after a second; "api" records whether the port was up when it began.
        let db = state.store.add_service(p, "db", &format!("sleep 1; python3 -m http.server {port} --bind 127.0.0.1")).unwrap();
        let api = state.store.add_service(p, "api", &format!("(exec 3<>/dev/tcp/127.0.0.1/{port}) 2>/dev/null && echo db-was-ready || echo db-not-ready; sleep 30")).unwrap();
        let mut o = Overrides::default();
        o.port = Some(port);
        state.store.set_overrides("service", db, &o).unwrap();
        let mut o = Overrides::default();
        o.depends_on = vec![db];
        o.shell = Some("/bin/bash".into());
        state.store.set_overrides("service", api, &o).unwrap();

        start_tree(h, api, DEFAULT_SIZE, false, None, &mut Vec::new()).unwrap();
        assert!(state.procs.is_running(db));
        std::thread::sleep(Duration::from_millis(500));
        assert!(state.procs.output(api).contains("db-was-ready"), "{}", state.procs.output(api));
        state.procs.stop_all();
    }

    #[test]
    fn busy_port_refuses_unless_forced() {
        let (app, tmp) = app();
        let h = app.handle();
        let state = h.state::<AppState>();
        let p = state.store.add_project(&tmp.path().display().to_string(), "p", None).unwrap();
        let s = state.store.add_service(p, "web", "sleep 30").unwrap();
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = held.local_addr().unwrap().port();
        let mut o = Overrides::default();
        o.port = Some(port);
        state.store.set_overrides("service", s, &o).unwrap();

        let e = start_tree(h, s, DEFAULT_SIZE, false, None, &mut Vec::new()).unwrap_err();
        assert!(e.starts_with(&format!("PORT_BUSY:{port}:")), "{e}");
        assert!(!state.procs.is_running(s));
        start_tree(h, s, DEFAULT_SIZE, true, None, &mut Vec::new()).unwrap();
        assert!(state.procs.is_running(s));
        state.procs.stop_all();
    }

    #[test]
    fn dependency_loops_are_reported() {
        let (app, tmp) = app();
        let h = app.handle();
        let state = h.state::<AppState>();
        let p = state.store.add_project(&tmp.path().display().to_string(), "p", None).unwrap();
        let a = state.store.add_service(p, "a", "sleep 30").unwrap();
        let b = state.store.add_service(p, "b", "sleep 30").unwrap();
        for (from, to) in [(a, b), (b, a)] {
            let mut o = Overrides::default();
            o.depends_on = vec![to];
            state.store.set_overrides("service", from, &o).unwrap();
        }
        let e = start_tree(h, a, DEFAULT_SIZE, false, None, &mut Vec::new()).unwrap_err();
        assert_eq!(e, "Dependency loop: a → b → a");
        assert!(!state.procs.is_running(a) && !state.procs.is_running(b));
    }

    #[test]
    fn one_off_run_and_cascade_reach_the_process() {
        let (app, tmp) = app();
        let h = app.handle();
        let state = h.state::<AppState>();
        let g = state.store.create_group("g", None).unwrap();
        let p = state.store.add_project(&tmp.path().display().to_string(), "p", Some(g)).unwrap();
        let s = state.store.add_service(p, "show", "echo \"G=$G P=$P ONCE=$ONCE\"").unwrap();
        let mut o = Overrides::default();
        o.env.insert("G".into(), "group".into());
        o.env.insert("P".into(), "group".into());
        state.store.set_overrides("group", g, &o).unwrap();
        let mut o = Overrides::default();
        o.env.insert("P".into(), "project".into());
        state.store.set_overrides("project", p, &o).unwrap();
        let once = OneOff { args: "extra".into(), env: BTreeMap::from([("ONCE".into(), "yes".into())]) };
        start_tree(h, s, DEFAULT_SIZE, false, Some(&once), &mut Vec::new()).unwrap();
        std::thread::sleep(Duration::from_millis(600));
        let out = state.procs.output(s);
        assert!(out.contains("G=group P=project ONCE=yes extra"), "{out}");
    }

    #[test]
    fn group_errors_keep_their_wording() {
        assert_eq!(plain_error("PORT_BUSY:5173:Port 5173 is already in use."), "Port 5173 is already in use.");
        assert_eq!(plain_error("Folder not found: /x"), "Folder not found: /x");
        assert_eq!(plain_error("Dependency loop: a → b → a"), "Dependency loop: a → b → a");
    }

    #[test]
    fn ipv6_only_listener_counts_as_in_use() {
        let Ok(l) = std::net::TcpListener::bind("[::1]:0") else { return }; // no IPv6 here
        assert!(port_in_use(l.local_addr().unwrap().port()));
    }

    #[test]
    fn dependency_already_running_outside_rouse_is_ready() {
        let (app, tmp) = app();
        let h = app.handle();
        let state = h.state::<AppState>();
        let p = state.store.add_project(&tmp.path().display().to_string(), "p", None).unwrap();
        let db = state.store.add_service(p, "db", "echo should-not-start; sleep 30").unwrap();
        let api = state.store.add_service(p, "api", "sleep 30").unwrap();
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut o = Overrides::default();
        o.port = Some(held.local_addr().unwrap().port());
        state.store.set_overrides("service", db, &o).unwrap();
        let mut o = Overrides::default();
        o.depends_on = vec![db];
        state.store.set_overrides("service", api, &o).unwrap();
        start_tree(h, api, DEFAULT_SIZE, false, None, &mut Vec::new()).unwrap();
        assert!(!state.procs.is_running(db), "an external db must not be started again");
        assert!(state.procs.is_running(api));
        state.procs.stop_all();
    }
}
