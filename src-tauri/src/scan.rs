//! Finding projects and the commands that start them.
//!
//! Reads files only: nothing from a repo is ever executed while scanning. Walks a folder,
//! prunes dependency and build output, doesn't follow symlinks, and treats a folder as a
//! project when it holds a known marker (package.json, compose file, Cargo.toml, Makefile,
//! go.mod, manage.py). Each project gets suggested commands; the best one is pre-selected.
//! Groups come from the folders between the scan root and the project.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

const MAX_DEPTH: usize = 6;

/// Folders never worth descending into.
const PRUNE: &[&str] = &[
    "node_modules", "target", "dist", "dist-ssr", "build", "out", "coverage", "vendor", "venv",
    "__pycache__", "bower_components", "Pods", "DerivedData", "gen", "generated",
];

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub name: String,
    pub command: String,
    /// Pre-selected in the import list.
    pub recommended: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub path: String,
    pub name: String,
    /// Folders from the scan root (included) down to the project's parent.
    pub groups: Vec<String>,
    /// Absolute paths of those folders, same order, for stable group identity.
    pub group_paths: Vec<String>,
    /// What was found: "node", "compose", "rust", "make", "go", "django".
    pub kinds: Vec<String>,
    pub suggestions: Vec<Suggestion>,
}

pub fn scan(root: &Path) -> Vec<Candidate> {
    let mut out = Vec::new();
    walk(root, root, 0, &mut out);
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<Candidate>) {
    if let Some(c) = inspect(root, dir) {
        out.push(c);
    }
    if depth >= MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut subdirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        // file_type() does not follow symlinks, so links are skipped.
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            !name.starts_with('.') && !PRUNE.contains(&name)
        })
        .collect();
    subdirs.sort();
    for sub in subdirs {
        walk(root, &sub, depth + 1, out);
    }
}

fn inspect(root: &Path, dir: &Path) -> Option<Candidate> {
    let mut kinds = Vec::new();
    let mut suggestions = Vec::new();

    if let Some(pkg) = read_json(&dir.join("package.json")) {
        let pm = package_manager(root, dir);
        let scripts = node_scripts(&pkg, &pm);
        if !scripts.is_empty() {
            kinds.push("node".to_string());
            suggestions.extend(scripts);
        }
    }

    // Docker finds these names by itself, so no `-f` is needed.
    if ["compose.yaml", "compose.yml", "docker-compose.yml", "docker-compose.yaml"].iter().any(|f| dir.join(f).is_file()) {
        kinds.push("compose".to_string());
        // Attached, not -d: the service then runs as long as the containers do.
        suggestions.push(Suggestion { name: "compose".into(), command: "docker compose up".into(), recommended: suggestions.is_empty() });
    }

    // A Tauri app's Rust half is started through the frontend's `tauri dev`.
    let in_tauri = dir.file_name().is_some_and(|n| n == "src-tauri");
    if !in_tauri && dir.join("Cargo.toml").is_file() && (dir.join("src/main.rs").is_file() || cargo_has_bin(dir)) {
        kinds.push("rust".to_string());
        suggestions.push(Suggestion { name: "run".into(), command: "cargo run".into(), recommended: suggestions.is_empty() });
    }

    if let Ok(make) = fs::read_to_string(dir.join("Makefile")) {
        let targets = make_targets(&make);
        if !targets.is_empty() {
            kinds.push("make".to_string());
            let first = suggestions.is_empty();
            for (i, t) in targets.into_iter().enumerate() {
                suggestions.push(Suggestion { command: format!("make {t}"), name: t, recommended: first && i == 0 });
            }
        }
    }

    if dir.join("go.mod").is_file() && dir.join("main.go").is_file() {
        kinds.push("go".to_string());
        suggestions.push(Suggestion { name: "run".into(), command: "go run .".into(), recommended: suggestions.is_empty() });
    }

    if dir.join("manage.py").is_file() {
        kinds.push("django".to_string());
        suggestions.push(Suggestion {
            name: "runserver".into(),
            command: "python manage.py runserver".into(),
            recommended: suggestions.is_empty(),
        });
    }

    if suggestions.is_empty() {
        return None;
    }

    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("project").to_string();
    let mut groups = Vec::new();
    let mut group_paths = Vec::new();
    // Folders from the root down to the parent: root/work/shop for root/work/shop/api.
    let mut current = dir.parent();
    while let Some(p) = current {
        if !p.starts_with(root) {
            break;
        }
        groups.push(p.file_name().and_then(|n| n.to_str()).unwrap_or("/").to_string());
        group_paths.push(p.display().to_string());
        if p == root {
            break;
        }
        current = p.parent();
    }
    groups.reverse();
    group_paths.reverse();

    Some(Candidate { path: dir.display().to_string(), name, groups, group_paths, kinds, suggestions })
}

/// The package.json script a command runs (`pnpm dev`, `npm run dev`, `yarn start:dev`).
pub fn script_body(project_dir: &Path, command: &str) -> Option<String> {
    let words: Vec<&str> = command.split_whitespace().collect();
    let name = match words.as_slice() {
        ["npm", "run" | "run-script", name, ..] => *name,
        ["npm", "start", ..] => "start",
        ["pnpm" | "yarn" | "bun", "run", name, ..] => *name,
        ["pnpm" | "yarn" | "bun", name, ..] => *name,
        _ => return None,
    };
    let pkg = read_json(&project_dir.join("package.json"))?;
    pkg.get("scripts")?.get(name)?.as_str().map(str::to_string)
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// From the nearest lockfile, walking up to the scan root (monorepos lock at the top).
fn package_manager(root: &Path, dir: &Path) -> String {
    let mut current = Some(dir);
    while let Some(d) = current {
        for (file, pm) in [("pnpm-lock.yaml", "pnpm"), ("yarn.lock", "yarn"), ("bun.lockb", "bun"), ("bun.lock", "bun"), ("package-lock.json", "npm")] {
            if d.join(file).is_file() {
                return pm.to_string();
            }
        }
        if d == root {
            break;
        }
        current = d.parent();
    }
    "npm".to_string()
}

/// Script names, best first, that usually mean "run this while developing".
const PREFERRED: &[&str] = &["dev", "start:dev", "develop", "serve", "start", "watch", "storybook", "preview"];

fn node_scripts(pkg: &serde_json::Value, pm: &str) -> Vec<Suggestion> {
    let Some(scripts) = pkg.get("scripts").and_then(|s| s.as_object()) else { return Vec::new() };
    let long_running = |name: &str, body: &str| {
        PREFERRED.contains(&name)
            || name.starts_with("dev:")
            || name.starts_with("start:")
            || name == "tauri"
            || ["--watch", "nodemon", "vite", "next dev", "nuxt dev", "tsx watch", "webpack serve", "astro dev"]
                .iter()
                .any(|m| body.contains(m))
    };
    let mut found: Vec<(usize, Suggestion)> = scripts
        .iter()
        .filter_map(|(name, body)| {
            let body = body.as_str().unwrap_or("");
            // Long-running but not for development: tests in watch mode, deploys, analysers.
            const NOT_DEV: &[&str] = &["build", "prod", "debug", "test", "deploy", "lint", "analyze", "format", "typecheck", "e2e"];
            if !long_running(name, body) || NOT_DEV.iter().any(|w| name.contains(w)) {
                return None;
            }
            let rank = PREFERRED.iter().position(|p| p == name).unwrap_or(PREFERRED.len());
            // `tauri` is the CLI itself; the dev app is `tauri dev`.
            let script = if name == "tauri" { "tauri dev".to_string() } else { name.clone() };
            let command = match pm {
                "npm" => format!("npm run {script}"),
                pm => format!("{pm} {script}"),
            };
            let label = if name == "tauri" { "tauri dev".to_string() } else { name.clone() };
            Some((rank, Suggestion { name: label, command, recommended: false }))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.name.cmp(&b.1.name)));
    let mut out: Vec<Suggestion> = found.into_iter().map(|(_, s)| s).collect();
    // A Tauri app is developed through `tauri dev`, not its bare web `dev`.
    if let Some(i) = out.iter().position(|s| s.name == "tauri dev") {
        out[i].recommended = true;
    } else if let Some(first) = out.first_mut() {
        first.recommended = true;
    }
    out
}

fn cargo_has_bin(dir: &Path) -> bool {
    fs::read_to_string(dir.join("Cargo.toml")).is_ok_and(|t| t.contains("[[bin]]"))
}

/// Makefile targets that usually start something.
fn make_targets(make: &str) -> Vec<String> {
    const WANTED: &[&str] = &["dev", "run", "serve", "start", "watch", "up"];
    let mut out = Vec::new();
    for line in make.lines() {
        if let Some((target, _)) = line.split_once(':') {
            let target = target.trim();
            if WANTED.contains(&target) && !line.starts_with('\t') && !out.iter().any(|t| t == target) {
                out.push(target.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, file: &str, body: &str) {
        let path = dir.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn finds_projects_groups_and_commands() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("work");
        write(&root, "shop/backend/package.json", r#"{"scripts":{"build":"nest build","start":"nest start","start:dev":"nest start --watch","lint":"oxlint"}}"#);
        write(&root, "shop/backend/pnpm-lock.yaml", "");
        write(&root, "shop/backend/docker-compose.yml", "services:\n  db:\n    image: postgres\n");
        write(&root, "shop/dashboard/package.json", r#"{"scripts":{"dev":"vite","build":"vite build"}}"#);
        write(&root, "shop/dashboard/yarn.lock", "");
        write(&root, "tools/app/package.json", r#"{"scripts":{"dev":"vite","tauri":"tauri"}}"#);
        write(&root, "tools/app/src-tauri/Cargo.toml", "[package]\nname='x'");
        write(&root, "tools/app/src-tauri/src/main.rs", "fn main(){}");
        write(&root, "shop/backend/node_modules/x/package.json", r#"{"scripts":{"dev":"x"}}"#);
        write(&root, "libs/only-lint/package.json", r#"{"scripts":{"lint":"eslint"}}"#);

        let found = scan(&root);
        let names: Vec<_> = found.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["backend", "dashboard", "app"]);

        let backend = &found[0];
        assert_eq!(backend.groups, ["work", "shop"]);
        assert_eq!(backend.kinds, ["node", "compose"]);
        let cmds: Vec<_> = backend.suggestions.iter().map(|s| (s.command.as_str(), s.recommended)).collect();
        assert_eq!(cmds, [("pnpm start:dev", true), ("pnpm start", false), ("docker compose up", false)]);

        assert_eq!(found[1].suggestions[0].command, "yarn dev");
        let app: Vec<_> = found[2].suggestions.iter().map(|s| (s.command.as_str(), s.recommended)).collect();
        assert_eq!(app, [("npm run dev", false), ("npm run tauri dev", true)]);
    }

    #[test]
    fn make_targets_skip_recipes() {
        let make = "dev: deps\n\tgo run .\nbuild:\n\tgo build\nup:\n\tdocker compose up\n";
        assert_eq!(make_targets(make), ["dev", "up"]);
    }
}

#[cfg(test)]
mod real {
    /// `ROUSE_SCAN=~/code cargo test --lib scan_real -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn scan_real() {
        let root = std::env::var("ROUSE_SCAN").expect("set ROUSE_SCAN");
        let started = std::time::Instant::now();
        let found = super::scan(std::path::Path::new(&root));
        for c in &found {
            let cmds: Vec<String> = c.suggestions.iter().map(|s| format!("{}{}", if s.recommended { "*" } else { "" }, s.command)).collect();
            println!("{:<60} {}", c.path.trim_start_matches(&root), cmds.join(" | "));
        }
        println!("{} projects in {:?}", found.len(), started.elapsed());
    }
}
