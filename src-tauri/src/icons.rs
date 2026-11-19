//! Project logos, found the way a browser would: the `<link rel="icon">` in the project's
//! index.html (Vite and Next resolve `/favicon.svg` from `public/`), then the usual favicon
//! spots. Read fresh on every refresh, so a changed logo shows up without rescanning.
//! Files only; nothing is executed. Large files are skipped.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use base64::Engine;
use regex::Regex;

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
    fn inline_data_icons_are_used_as_is() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "index.html", br#"<link href="data:image/svg+xml,%3Csvg/%3E" rel="icon">"#);
        assert_eq!(project_icon(tmp.path()).unwrap(), "data:image/svg+xml,%3Csvg/%3E");
    }
}
