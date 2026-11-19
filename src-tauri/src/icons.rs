//! Project logos, found the way a browser would: the `<link rel="icon">` in the project's
//! index.html (Vite and Next resolve `/favicon.svg` from `public/`), then the usual favicon
//! spots. Read fresh on every refresh, so a changed logo shows up without rescanning.
//! Files only; nothing is executed. Large files are skipped.
//!
//! A project without a logo of its own gets its stack's (NestJS, Express, Vite, Rust…),
//! guessed from its dependencies; the app draws those from its bundled icon set.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use base64::Engine;
use regex::Regex;
use serde::Serialize;

/// What to show for a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Logo {
    /// Its own icon, as a `data:` URL.
    Image { src: String },
    /// Its stack's logo, by icon slug (simple-icons naming).
    Tech { slug: &'static str },
}

/// The project's own logo, else its stack's, else None.
pub fn project_logo(dir: &Path) -> Option<Logo> {
    project_icon(dir).map(|src| Logo::Image { src }).or_else(|| tech(dir).map(|slug| Logo::Tech { slug }))
}

/// package.json dependencies that name a stack, most specific first: a desktop shell or
/// meta-framework wins over the UI library or server it is built on.
const NODE_STACKS: &[(&[&str], &str)] = &[
    (&["@nestjs/core"], "nestjs"),
    (&["@adonisjs/core"], "adonisjs"),
    (&["@strapi/strapi"], "strapi"),
    (&["@tauri-apps/api", "@tauri-apps/cli"], "tauri"),
    (&["electron"], "electron"),
    (&["expo"], "expo"),
    (&["next"], "nextdotjs"),
    (&["nuxt"], "nuxt"),
    (&["@remix-run/react", "@remix-run/node"], "remix"),
    (&["@react-router/dev"], "reactrouter"),
    (&["astro"], "astro"),
    (&["@sveltejs/kit", "svelte"], "svelte"),
    (&["@angular/core"], "angular"),
    (&["@builder.io/qwik"], "qwik"),
    (&["solid-js"], "solid"),
    (&["gatsby"], "gatsby"),
    (&["vue"], "vuedotjs"),
    (&["react"], "react"),
    (&["hono"], "hono"),
    (&["fastify"], "fastify"),
    (&["express"], "express"),
    (&["koa"], "koa"),
    (&["vite"], "vite"),
];

/// The project's stack, from the files that declare it.
pub fn tech(dir: &Path) -> Option<&'static str> {
    let read = |f: &str| std::fs::read_to_string(dir.join(f)).ok();
    let has = |f: &str| dir.join(f).exists();

    if has("manage.py") {
        return Some("django");
    }
    if let Some(pkg) = read("package.json").and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
        let deps = |name: &str| ["dependencies", "devDependencies", "peerDependencies"].iter().any(|k| pkg[k].get(name).is_some());
        if let Some((_, slug)) = NODE_STACKS.iter().find(|(names, _)| names.iter().any(|n| deps(n))) {
            return Some(slug);
        }
        return Some(if has("bun.lockb") || has("bun.lock") {
            "bun"
        } else if deps("typescript") {
            "typescript"
        } else {
            "nodedotjs"
        });
    }
    if has("deno.json") || has("deno.jsonc") {
        return Some("deno");
    }
    if let Some(cargo) = read("Cargo.toml") {
        return Some(if cargo.contains("tauri") { "tauri" } else { "rust" });
    }
    if has("go.mod") {
        return Some("go");
    }
    let python: String = ["pyproject.toml", "requirements.txt", "Pipfile", "setup.py"].iter().filter_map(|f| read(f)).collect::<Vec<_>>().join("\n").to_ascii_lowercase();
    if !python.is_empty() {
        return Some(if python.contains("fastapi") {
            "fastapi"
        } else if python.contains("flask") {
            "flask"
        } else if python.contains("django") {
            "django"
        } else {
            "python"
        });
    }
    if let Some(gems) = read("Gemfile") {
        return Some(if gems.contains("rails") { "rubyonrails" } else { "ruby" });
    }
    if let Some(composer) = read("composer.json") {
        return Some(if composer.contains("laravel/framework") { "laravel" } else { "php" });
    }
    let compose = ["compose.yaml", "compose.yml", "docker-compose.yml", "docker-compose.yaml", "Dockerfile"];
    compose.iter().any(|f| has(f)).then_some("docker")
}

const MAX_BYTES: u64 = 256 * 1024;

const INDEX_FILES: &[&str] = &["index.html", "public/index.html", "src/index.html", "app/index.html"];

const FALLBACKS: &[&str] = &[
    "public/favicon.svg",
    "public/favicon.png",
    "public/favicon.ico",
    "public/logo.svg",
    "public/logo.png",
    "src/assets/logo.svg",
    "src/app/favicon.ico",
    "app/favicon.ico",
    "static/favicon.svg",
    "static/favicon.png",
    "static/favicon.ico",
    "favicon.svg",
    "favicon.ico",
    "src-tauri/icons/128x128.png",
];

/// The project's logo as a `data:` URL, or None when it has none.
pub fn project_icon(dir: &Path) -> Option<String> {
    for index in INDEX_FILES {
        let path = dir.join(index);
        let Ok(html) = std::fs::read_to_string(&path) else { continue };
        for href in icon_hrefs(&html) {
            if href.starts_with("data:image/") {
                return Some(href);
            }
            if href.contains("://") || href.starts_with("//") {
                continue; // remote icons are not fetched
            }
            let clean = href.split(['?', '#']).next().unwrap_or("");
            for candidate in resolve(dir, path.parent().unwrap_or(dir), clean) {
                if let Some(url) = data_url(&candidate) {
                    return Some(url);
                }
            }
        }
    }
    FALLBACKS.iter().find_map(|f| data_url(&dir.join(f)))
}

/// `href`s of icon links, SVG first (crisp at any size), then in document order.
fn icon_hrefs(html: &str) -> Vec<String> {
    static LINK: OnceLock<Regex> = OnceLock::new();
    static ATTR: OnceLock<Regex> = OnceLock::new();
    let link = LINK.get_or_init(|| Regex::new(r"(?is)<link\b[^>]*>").unwrap());
    let attr = ATTR.get_or_init(|| Regex::new(r#"(?is)\b(rel|href|type)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#).unwrap());
    let mut found: Vec<(bool, String)> = Vec::new();
    for tag in link.find_iter(html) {
        let (mut rel, mut href, mut kind) = (String::new(), None, String::new());
        for c in attr.captures_iter(tag.as_str()) {
            let value = c.get(2).or(c.get(3)).or(c.get(4)).map(|m| m.as_str().to_string()).unwrap_or_default();
            match c[1].to_ascii_lowercase().as_str() {
                "rel" => rel = value.to_ascii_lowercase(),
                "href" => href = Some(value),
                _ => kind = value.to_ascii_lowercase(),
            }
        }
        if let Some(href) = href.filter(|h| !h.is_empty())
            && rel.split_whitespace().any(|r| r == "icon" || r == "apple-touch-icon" || r == "mask-icon")
        {
            let path = href.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
            let svg = kind.contains("svg") || path.ends_with(".svg");
            found.push((svg, href));
        }
    }
    found.sort_by_key(|(svg, _)| !svg);
    found.into_iter().map(|(_, h)| h).collect()
}

/// Where an href can live: `/x` from the project root or its public folder, `x` next to the
/// index file or in public.
fn resolve(root: &Path, index_dir: &Path, href: &str) -> Vec<PathBuf> {
    if let Some(abs) = href.strip_prefix('/') {
        vec![root.join("public").join(abs), root.join(abs), index_dir.join(abs)]
    } else {
        vec![index_dir.join(href), root.join("public").join(href), root.join(href)]
    }
}

fn data_url(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES || meta.len() == 0 {
        return None;
    }
    let mime = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return None,
    };
    let bytes = std::fs::read(path).ok()?;
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, file: &str, body: &[u8]) {
        let p = dir.join(file);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn icon_from_index_html_in_public() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "index.html", br#"<head><link rel="icon" type="image/png" href="/favicon.png"><link rel='icon' href='/logo.svg?v=2'></head>"#);
        write(tmp.path(), "public/logo.svg", b"<svg/>");
        write(tmp.path(), "public/favicon.png", b"png");
        // The SVG wins, the query string is ignored.
        assert_eq!(project_icon(tmp.path()).unwrap(), "data:image/svg+xml;base64,PHN2Zy8+");
    }

    #[test]
    fn falls_back_to_common_spots_and_skips_remote_and_huge_files() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "index.html", br#"<link rel="shortcut icon" href="https://cdn.example.com/x.ico">"#);
        write(tmp.path(), "app/favicon.ico", b"ico");
        assert!(project_icon(tmp.path()).unwrap().starts_with("data:image/x-icon;base64,"));
        let big = tempfile::tempdir().unwrap();
        write(big.path(), "public/favicon.png", &vec![0u8; (MAX_BYTES + 1) as usize]);
        assert_eq!(project_icon(big.path()), None);
    }

    #[test]
    fn stack_logo_when_there_is_no_icon() {
        let case = |files: &[(&str, &str)]| {
            let tmp = tempfile::tempdir().unwrap();
            for (f, body) in files {
                write(tmp.path(), f, body.as_bytes());
            }
            project_logo(tmp.path())
        };
        let tech = |slug| Some(Logo::Tech { slug });
        // NestJS uses express underneath; the framework wins.
        assert_eq!(case(&[("package.json", r#"{"dependencies":{"@nestjs/core":"11","express":"5"}}"#)]), tech("nestjs"));
        assert_eq!(case(&[("package.json", r#"{"dependencies":{"express":"5"}}"#)]), tech("express"));
        // A Tauri app built with React and Vite is a Tauri app.
        assert_eq!(case(&[("package.json", r#"{"dependencies":{"react":"19"},"devDependencies":{"@tauri-apps/cli":"2","vite":"8"}}"#)]), tech("tauri"));
        assert_eq!(case(&[("package.json", r#"{"devDependencies":{"typescript":"6"}}"#)]), tech("typescript"));
        assert_eq!(case(&[("package.json", "{}"), ("bun.lock", "")]), tech("bun"));
        assert_eq!(case(&[("Cargo.toml", "[package]\nname='x'")]), tech("rust"));
        assert_eq!(case(&[("go.mod", "module x")]), tech("go"));
        assert_eq!(case(&[("manage.py", ""), ("package.json", r#"{"devDependencies":{"vite":"8"}}"#)]), tech("django"));
        assert_eq!(case(&[("requirements.txt", "FastAPI==0.120\nuvicorn")]), tech("fastapi"));
        assert_eq!(case(&[("compose.yaml", "services: {}")]), tech("docker"));
        assert_eq!(case(&[("Makefile", "run:")]), None);
        // Its own icon still comes first.
        let own = case(&[("package.json", r#"{"dependencies":{"express":"5"}}"#), ("public/favicon.svg", "<svg/>")]);
        assert!(matches!(own, Some(Logo::Image { .. })));
    }

    #[test]
    fn inline_data_icons_are_used_as_is() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "index.html", br#"<link href="data:image/svg+xml,%3Csvg/%3E" rel="icon">"#);
        assert_eq!(project_icon(tmp.path()).unwrap(), "data:image/svg+xml,%3Csvg/%3E");
    }
}
