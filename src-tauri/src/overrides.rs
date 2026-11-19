//! Overrides: settings that cascade from the whole app down to one command.
//!
//! Levels, outermost first: global → each group from the top → the project → the command.
//! For every setting the closest level that sets it wins, and the result remembers where
//! each value came from, so the editor can say "SIGINT, from group shop".
//!
//! Environment: each level contributes its .env file (if any) and then its own variables;
//! later levels override earlier ones. A port, when set, becomes `PORT` and, where rouse can
//! tell the tool from the package.json script, its own flag (`--port`, `-p`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::procs::{Spec, StopSignal};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Overrides {
    pub env: BTreeMap<String, String>,
    /// A .env file, relative to the project folder (or absolute, or `~/…`).
    pub env_file: Option<String>,
    /// Commands only.
    pub port: Option<u16>,
    pub shell: Option<String>,
    pub stop_signal: Option<StopSignal>,
    pub grace_seconds: Option<u32>,
    pub auto_restart: Option<bool>,
    /// Commands only: services to start (and wait for) first.
    pub depends_on: Vec<i64>,
}

/// One level of the cascade, as the resolver sees it.
pub struct Level {
    /// Shown as the source: "global", "group shop", "project shop-api", "this command".
    pub label: String,
    pub overrides: Overrides,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Sourced<T> {
    pub value: T,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    pub key: String,
    pub value: String,
    pub source: String,
}

/// What a command will actually run with.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Effective {
    /// The saved command plus any port flag and one-off arguments.
    pub command: String,
    pub cwd: String,
    /// Variables rouse adds on top of the login shell's environment.
    pub env: Vec<EnvVar>,
    pub port: Option<Sourced<u16>>,
    /// How the port is passed besides PORT, e.g. "--port 5174".
    pub port_flag: Option<String>,
    pub shell: Sourced<String>,
    pub stop_signal: Sourced<StopSignal>,
    pub grace_seconds: Sourced<u32>,
    pub auto_restart: Sourced<bool>,
    pub depends_on: Vec<i64>,
    /// Problems worth showing, like an env file that doesn't exist.
    pub warnings: Vec<String>,
}

impl Effective {
    pub fn spec(&self) -> Spec {
        Spec {
            command: self.command.clone(),
            cwd: self.cwd.clone(),
            env: self.env.iter().map(|v| (v.key.clone(), v.value.clone())).collect(),
            shell: self.shell.value.clone(),
            stop_signal: self.stop_signal.value,
            grace: Duration::from_secs(self.grace_seconds.value as u64),
            auto_restart: self.auto_restart.value,
        }
    }
}

/// Extra arguments and variables for a single run ("Start with…"), not saved.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OneOff {
    pub args: String,
    pub env: BTreeMap<String, String>,
}

/// Resolve the cascade for one command.
/// `script` is the package.json script body behind the command, when there is one.
pub fn resolve(levels: &[Level], command: &str, project_dir: &Path, cwd: &str, script: Option<&str>, once: Option<&OneOff>) -> Effective {
    fn pick<T: Clone>(levels: &[Level], get: impl Fn(&Overrides) -> Option<T>, default: T) -> Sourced<T> {
        levels
            .iter()
            .rev()
            .find_map(|l| get(&l.overrides).map(|value| Sourced { value, source: l.label.clone() }))
            .unwrap_or(Sourced { value: default, source: "default".into() })
    }

    let mut warnings = Vec::new();
    let mut env: BTreeMap<String, EnvVar> = BTreeMap::new();
    for level in levels {
        if let Some(file) = level.overrides.env_file.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
            let path = expand(file, project_dir);
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    for (key, value) in parse_dotenv(&text) {
                        env.insert(key.clone(), EnvVar { key, value, source: format!("{} ({file})", level.label) });
                    }
                }
                Err(_) => warnings.push(format!("Env file not found: {} (set on {})", path.display(), level.label)),
            }
        }
        for (key, value) in &level.overrides.env {
            env.insert(key.clone(), EnvVar { key: key.clone(), value: value.clone(), source: level.label.clone() });
        }
    }

    let port = levels.iter().rev().find_map(|l| l.overrides.port.map(|value| Sourced { value, source: l.label.clone() }));
    let mut command = command.trim().to_string();
    let mut port_flag = None;
    if let Some(p) = &port {
        env.insert("PORT".into(), EnvVar { key: "PORT".into(), value: p.value.to_string(), source: p.source.clone() });
        if let Some(flag) = port_flag_for(&command, script, p.value) {
            command = append_args(&command, &flag);
            port_flag = Some(flag);
        }
    }

    if let Some(once) = once {
        if !once.args.trim().is_empty() {
            command = append_args(&command, once.args.trim());
        }
        for (key, value) in &once.env {
            env.insert(key.clone(), EnvVar { key: key.clone(), value: value.clone(), source: "this run".into() });
        }
    }

    let depends_on = levels.last().map(|l| l.overrides.depends_on.clone()).unwrap_or_default();
    Effective {
        command,
        cwd: cwd.to_string(),
        env: env.into_values().collect(),
        port,
        port_flag,
        shell: pick(levels, |o| o.shell.clone().filter(|s| !s.trim().is_empty()), "/bin/sh".to_string()),
        stop_signal: pick(levels, |o| o.stop_signal, StopSignal::Term),
        grace_seconds: pick(levels, |o| o.grace_seconds, 5),
        auto_restart: pick(levels, |o| o.auto_restart, false),
        depends_on,
        warnings,
    }
}

fn expand(file: &str, project_dir: &Path) -> PathBuf {
    if let Some(rest) = file.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME") {
            return Path::new(&home).join(rest);
        }
    let path = Path::new(file);
    if path.is_absolute() { path.to_path_buf() } else { project_dir.join(path) }
}

/// `KEY=value` lines: `#` comments, optional `export `, single or double quotes
/// (double quotes understand `\n` and `\"`), inline ` #` comments on unquoted values.
pub fn parse_dotenv(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let value = value.trim();
        let value = if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            inner.replace("\\n", "\n").replace("\\\"", "\"")
        } else if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
            inner.to_string()
        } else {
            value.split(" #").next().unwrap_or("").trim().to_string()
        };
        out.push((key.to_string(), value));
    }
    out
}

/// The tool behind a command: the command itself, or the package.json script it runs.
fn port_flag_for(command: &str, script: Option<&str>, port: u16) -> Option<String> {
    let body = script.unwrap_or(command);
    let has = |needle: &str| body.split(|c: char| c.is_whitespace() || c == '&' || c == ';').any(|w| w == needle);
    if has("vite") || has("astro") || has("nuxt") || has("nuxi") || body.contains("webpack serve") || has("svelte-kit") {
        Some(format!("--port {port}"))
    } else if has("next") || has("storybook") || body.contains("start-storybook") {
        Some(format!("-p {port}"))
    } else if body.contains("manage.py runserver") {
        Some(port.to_string())
    } else {
        // Nest, Express, CRA and most servers read PORT, which is always set.
        None
    }
}

/// Add arguments so they reach the script: npm needs `--`; pnpm, yarn and bun pass them on.
pub fn append_args(command: &str, args: &str) -> String {
    let words: Vec<&str> = command.split_whitespace().collect();
    if words.first() == Some(&"npm") && words.get(1) == Some(&"run") && !words.contains(&"--") {
        format!("{command} -- {args}")
    } else {
        format!("{command} {args}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(label: &str, f: impl FnOnce(&mut Overrides)) -> Level {
        let mut o = Overrides::default();
        f(&mut o);
        Level { label: label.into(), overrides: o }
    }

    #[test]
    fn closest_level_wins_and_sources_are_kept() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".env.local"), "# comment\nexport API_URL=\"http://x\\ny\"\nDEBUG=1 # inline\nNAME='a b'\n").unwrap();
        let levels = [
            level("global", |o| {
                o.env.insert("NODE_ENV".into(), "development".into());
                o.grace_seconds = Some(10);
            }),
            level("group shop", |o| {
                o.env_file = Some(".env.local".into());
                o.stop_signal = Some(StopSignal::Int);
                o.env.insert("DEBUG".into(), "0".into());
            }),
            level("project api", |o| o.auto_restart = Some(true)),
            level("this command", |o| {
                o.env.insert("NODE_ENV".into(), "test".into());
                o.depends_on = vec![7];
            }),
        ];
        let e = resolve(&levels, "pnpm start:dev", tmp.path(), "/x", None, None);
        let get = |k: &str| e.env.iter().find(|v| v.key == k).unwrap().clone();
        assert_eq!(get("NODE_ENV").value, "test");
        assert_eq!(get("NODE_ENV").source, "this command");
        assert_eq!(get("API_URL").value, "http://x\ny");
        assert_eq!(get("API_URL").source, "group shop (.env.local)");
        // The group's own variable is applied after its file.
        assert_eq!(get("DEBUG").value, "0");
        assert_eq!(get("NAME").value, "a b");
        assert_eq!(e.grace_seconds, Sourced { value: 10, source: "global".into() });
        assert_eq!(e.stop_signal.value, StopSignal::Int);
        assert_eq!(e.auto_restart, Sourced { value: true, source: "project api".into() });
        assert_eq!(e.shell, Sourced { value: "/bin/sh".into(), source: "default".into() });
        assert_eq!(e.depends_on, [7]);
        assert!(e.warnings.is_empty());
    }

    #[test]
    fn missing_env_file_is_a_warning() {
        let levels = [level("project p", |o| o.env_file = Some(".env.nope".into()))];
        let e = resolve(&levels, "pnpm dev", Path::new("/tmp"), "/tmp", None, None);
        assert_eq!(e.warnings.len(), 1);
    }

    #[test]
    fn port_sets_env_and_tool_flag() {
        let levels = [level("this command", |o| o.port = Some(5174))];
        let vite = resolve(&levels, "pnpm dev", Path::new("/p"), "/p", Some("vite"), None);
        assert_eq!(vite.command, "pnpm dev --port 5174");
        assert!(vite.env.iter().any(|v| v.key == "PORT" && v.value == "5174"));
        let next = resolve(&levels, "npm run dev", Path::new("/p"), "/p", Some("next dev --turbo"), None);
        assert_eq!(next.command, "npm run dev -- -p 5174");
        // Nest reads PORT; no flag is added.
        let nest = resolve(&levels, "pnpm start:dev", Path::new("/p"), "/p", Some("nest start --watch"), None);
        assert_eq!(nest.command, "pnpm start:dev");
        assert_eq!(nest.port_flag, None);
    }

    #[test]
    fn one_off_args_and_env() {
        let once = OneOff { args: "--inspect".into(), env: BTreeMap::from([("LOG".into(), "debug".into())]) };
        let e = resolve(&[], "npm run start", Path::new("/p"), "/p", None, Some(&once));
        assert_eq!(e.command, "npm run start -- --inspect");
        assert_eq!(e.env[0], EnvVar { key: "LOG".into(), value: "debug".into(), source: "this run".into() });
    }

    /// The exact shapes src/services/tauri.ts sends and reads.
    #[test]
    fn wire_format_matches_the_frontend() {
        let from_ui = serde_json::json!({
            "env": { "A": "1" }, "envFile": ".env.local", "port": 5174, "shell": null,
            "stopSignal": "SIGINT", "graceSeconds": 12, "autoRestart": true, "dependsOn": [3]
        });
        let o: Overrides = serde_json::from_value(from_ui.clone()).unwrap();
        assert_eq!(o.stop_signal, Some(StopSignal::Int));
        assert_eq!(serde_json::to_value(&o).unwrap(), from_ui);
        // Older or partial payloads fill in defaults.
        let partial: Overrides = serde_json::from_value(serde_json::json!({ "graceSeconds": 3 })).unwrap();
        assert_eq!(partial.grace_seconds, Some(3));

        let once: OneOff = serde_json::from_value(serde_json::json!({ "args": "--x", "env": { "B": "2" } })).unwrap();
        let e = resolve(&[Level { label: "this command".into(), overrides: o }], "pnpm dev", Path::new("/p"), "/p", Some("vite"), Some(&once));
        let v = serde_json::to_value(&e).unwrap();
        for key in ["command", "cwd", "env", "port", "portFlag", "shell", "stopSignal", "graceSeconds", "autoRestart", "dependsOn", "warnings"] {
            assert!(v.get(key).is_some(), "missing {key} in {v}");
        }
        assert_eq!(v["port"], serde_json::json!({ "value": 5174, "source": "this command" }));
        assert_eq!(v["stopSignal"]["value"], "SIGINT");
        assert_eq!(v["command"], "pnpm dev --port 5174 --x");
    }
}
