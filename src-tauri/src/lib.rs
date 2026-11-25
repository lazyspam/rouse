//! rouse: remembers the dev commands of your projects and starts them from one window.
//!
//! - `scan` finds projects and suggests commands; `store` remembers what you keep.
//! - `procs` runs each command in a PTY and stops its whole process tree.
//! - `stats` samples CPU and memory per service, emitted every couple of seconds.
//! - `env` gives commands your login shell's environment (nvm, pnpm on PATH).
//! - `orphans` cleans up after a crash. Quitting, the tray's Quit, and SIGTERM at logout all
//!   stop every running service first.

mod acceptance;
mod commands;
mod env;
mod icons;
mod orphans;
mod overrides;
mod procs;
mod scan;
mod stats;
mod store;
mod tray;

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, RunEvent, Runtime};

use commands::AppState;
use procs::Sink;

/// How often resource use is sampled while the window shows.
const STATS_EVERY: Duration = Duration::from_secs(2);

/// Remember the running process groups, so a crash can be cleaned up next launch.
pub(crate) fn record_live<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    let live: Vec<(u32, u64)> =
        state.procs.running_pids().into_iter().filter_map(|pid| orphans::start_ticks(pid).map(|t| (pid, t))).collect();
    let _ = state.store.set_live(&live);
}

pub fn run() {
    let mut builder = tauri::Builder::default();
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)));
    }

    let app = builder
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = store::Store::open(&dir.join("rouse.db"))?;

            let leftovers = store.take_live().unwrap_or_default();
            if !leftovers.is_empty() {
                let stopped = orphans::reap(&leftovers);
                if stopped > 0 {
                    eprintln!("rouse: stopped {stopped} process group(s) left by a previous run");
                }
            }

            let handle = app.handle().clone();
            let sink: Sink = Arc::new(move |event| {
                // Every start and end, automatic restarts included, updates the crash record.
                if let procs::Event::Status { status: procs::State::Running { .. } | procs::State::Exited { .. } | procs::State::Stopped, .. } = &event
                    && handle.try_state::<AppState>().is_some()
                {
                    record_live(&handle);
                }
                let _ = handle.emit("rouse://event", event);
            });
            let procs = procs::Manager::new(env::login_env(Duration::from_secs(8)), sink);
            app.manage(AppState { store, procs });

            // Resource use, only while the window is showing (rouse lives in the tray).
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let Some(mut sampler) = stats::Sampler::new() else { return };
                loop {
                    let visible = handle.get_webview_window("main").and_then(|w| w.is_visible().ok()).unwrap_or(false);
                    if visible {
                        let live = handle.state::<AppState>().procs.live_pids();
                        let _ = handle.emit("rouse://stats", sampler.sample(&live));
                    }
                    std::thread::sleep(STATS_EVERY);
                }
            });

            // Logout and `kill` send SIGTERM/SIGHUP: stop services before going.
            #[cfg(unix)]
            {
                use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    if let Ok(mut signals) = signal_hook::iterator::Signals::new([SIGTERM, SIGINT, SIGHUP])
                        && signals.forever().next().is_some() {
                            handle.state::<AppState>().procs.stop_all();
                            let _ = handle.state::<AppState>().store.set_live(&[]);
                            std::process::exit(0);
                        }
                });
            }

            let handle = app.handle().clone();
            app.handle().run_on_main_thread(move || {
                if let Err(e) = tray::create(&handle) {
                    eprintln!("rouse: tray init failed: {e}");
                }
                tray::show_main(&handle);
            })?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing hides to the tray; services keep running.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_tree,
            commands::get_states,
            commands::scan_folder,
            commands::import_projects,
            commands::start_service,
            commands::stop_service,
            commands::restart_service,
            commands::start_group,
            commands::stop_group,
            commands::start_project,
            commands::stop_project,
            commands::stop_all,
            commands::get_output,
            commands::clear_output,
            commands::write_input,
            commands::resize_terminal,
            commands::create_group,
            commands::rename_group,
            commands::move_group,
            commands::delete_group,
            commands::set_collapsed,
            commands::add_project,
            commands::rename_project,
            commands::move_project,
            commands::delete_project,
            commands::add_service,
            commands::update_service,
            commands::delete_service,
            commands::get_overrides,
            commands::set_overrides,
            commands::get_effective,
            commands::save_variant,
            commands::get_project_icons,
        ])
        .build(tauri::generate_context!())
        .expect("error while building rouse");

    app.run(|app, event| {
        if let RunEvent::ExitRequested { .. } = event {
            // Quit from the tray or the window: nothing keeps running behind us.
            app.state::<AppState>().procs.stop_all();
            let _ = app.state::<AppState>().store.set_live(&[]);
        }
    });
}
