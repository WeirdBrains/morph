//! Open-source notices: the crates built into Morph (`notices.tsv`, written
//! by `scripts/notices.py`) as a page the About page opens in a tab.

use std::path::PathBuf;

const TABLE: &str = include_str!("../notices.tsv");
const MORPH_LICENSE: &str = include_str!("../../../LICENSE");

/// One crate: name, version, license, repository.
pub fn crates() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    TABLE
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut f = l.split('\t');
            Some((f.next()?, f.next()?, f.next().unwrap_or(""), f.next().unwrap_or("")))
        })
        .collect()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The notices as an HTML page.
pub fn html() -> String {
    let mut rows = String::new();
    for (name, version, license, repo) in crates() {
        let link = if repo.starts_with("https://") { format!("<a href=\"{0}\">{0}</a>", escape(repo)) } else { escape(repo) };
        rows.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n", escape(name), escape(version), escape(license), link));
    }
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Morph open-source notices</title><style>\
         :root{{color-scheme:dark}}body{{background:#0d0d12;color:#f5f6fa;font:14px system-ui,sans-serif;margin:2rem auto;max-width:960px;padding:0 1rem}}\
         a{{color:#00e5ff}}table{{border-collapse:collapse;width:100%}}td,th{{text-align:left;padding:.25rem .5rem;border-bottom:1px solid #1e1e28}}\
         pre{{white-space:pre-wrap;background:#16161d;padding:1rem;border-radius:6px}}</style></head><body>\
         <h1>Open-source notices</h1><p>Morph {} is built from these {} crates, under the licenses their authors chose. \
         Their license texts are in each crate's repository and source package.</p>\
         <table><tr><th>Crate</th><th>Version</th><th>License</th><th>Repository</th></tr>\n{}</table>\
         <h2>Morph</h2><pre>{}</pre></body></html>",
        env!("CARGO_PKG_VERSION"),
        crates().len(),
        rows,
        escape(MORPH_LICENSE)
    )
}

/// Write the page to Morph's cache directory; `None` if that fails.
pub fn write_page() -> Option<PathBuf> {
    let dir = std::env::temp_dir().join(format!("morph-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("notices.html");
    std::fs::write(&path, html()).ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_crate_is_listed_with_a_license() {
        let list = crates();
        assert!(list.len() > 100, "{}", list.len());
        for name in ["gpui", "wry", "serde"] {
            assert!(list.iter().any(|c| c.0 == name), "{name} missing");
        }
        assert!(list.iter().all(|c| !c.2.is_empty()));
        let page = html();
        assert!(page.contains("<td>wry</td>"));
        assert!(page.contains("BSD"), "Morph's own license is included");
    }
}
