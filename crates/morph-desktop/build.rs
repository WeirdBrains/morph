//! The git commit and build date the About page shows. `MORPH_GIT_COMMIT`
//! overrides the commit (builds from a tarball have no git), and
//! `SOURCE_DATE_EPOCH` the date (reproducible package builds set it).

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

/// `days` since 1970-01-01 as a civil date (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn main() {
    println!("cargo:rerun-if-env-changed=MORPH_GIT_COMMIT");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    if let Some(dir) = git(&["rev-parse", "--git-dir"]) {
        let dir = std::path::Path::new(&dir);
        let dir = if dir.is_absolute() { dir.to_path_buf() } else { std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join(dir) };
        println!("cargo:rerun-if-changed={}", dir.join("HEAD").display());
        if let Some(head) = git(&["symbolic-ref", "-q", "HEAD"]) {
            println!("cargo:rerun-if-changed={}", dir.join(head).display());
        }
    }
    let commit = std::env::var("MORPH_GIT_COMMIT").ok().filter(|s| !s.is_empty()).or_else(|| git(&["rev-parse", "--short=12", "HEAD"])).unwrap_or_else(|| "unknown".into());
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0));
    let (y, m, d) = civil(secs.div_euclid(86_400));
    println!("cargo:rustc-env=MORPH_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=MORPH_BUILD_DATE={y:04}-{m:02}-{d:02}");
}
