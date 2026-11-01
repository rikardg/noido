//! Stamps the commit into `noido --version`: `0.1.0+deadbeef`.
//!
//! A release is built from a plain checkout on GitHub, where the workflow
//! passes the commit in `NOIDO_GIT_SHA`; a build elsewhere asks git instead.
//! The `+` is SemVer build metadata, which never changes how two versions
//! compare, so a snapshot is still "0.1.0" to anything that cares.
//!
//! The rerun line is what makes the stamp change: without it Cargo keeps the
//! build it has cached and the old commit stays in the binary.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=NOIDO_GIT_SHA");
    let commit = commit().unwrap_or_else(|| "unknown".to_string());
    println!(
        "cargo:rustc-env=NOIDO_VERSION={}+{}",
        env!("CARGO_PKG_VERSION"),
        commit
    );
}

/// The commit, shortened to eight characters: from the environment first, and
/// from git for a local build.
fn commit() -> Option<String> {
    if let Ok(sha) = std::env::var("NOIDO_GIT_SHA") {
        let short = shorten(&sha);
        if !short.is_empty() {
            return Some(short);
        }
    }
    let output = Command::new("git")
        .args(["rev-parse", "--short=8", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let short = shorten(&String::from_utf8_lossy(&output.stdout));
    (!short.is_empty()).then_some(short)
}

/// The first eight characters, when they look like a commit; empty otherwise,
/// so a version string never picks up something that is not one.
fn shorten(sha: &str) -> String {
    let short: String = sha.trim().chars().take(8).collect();
    if short.len() == 8 && short.chars().all(|c| c.is_ascii_hexdigit()) {
        short
    } else {
        String::new()
    }
}
