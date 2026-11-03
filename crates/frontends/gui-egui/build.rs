//! Stamps the commit into `noido --version`: `0.1.0+deadbeef`, and renders
//! the window icon from `assets/noido.svg`.
//!
//! A release is built from a plain checkout on GitHub, where the workflow
//! passes the commit in `NOIDO_GIT_SHA`; a build elsewhere asks git instead.
//! The `+` is SemVer build metadata, which never changes how two versions
//! compare, so a snapshot is still "0.1.0" to anything that cares.
//!
//! The rerun line is what makes the stamp change: without it Cargo keeps the
//! build it has cached and the old commit stays in the binary.

use std::path::PathBuf;
use std::process::Command;

/// The icon's source, and the side of the window icon rendered from it
/// (`src/icon.rs` holds the same number).
const ICON_SVG: &str = "assets/noido.svg";
const ICON_SIDE: u32 = 256;

fn main() {
    println!("cargo:rerun-if-env-changed=NOIDO_GIT_SHA");
    let commit = commit().unwrap_or_else(|| "unknown".to_string());
    println!(
        "cargo:rustc-env=NOIDO_VERSION={}+{}",
        env!("CARGO_PKG_VERSION"),
        commit
    );
    render_icon();
}

/// Renders the SVG to `$OUT_DIR/icon.rgba`: straight RGBA, as the window
/// icon wants it.
fn render_icon() {
    println!("cargo:rerun-if-changed={ICON_SVG}");
    let svg = std::fs::read(ICON_SVG).expect("read the icon SVG");
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())
        .expect("parse the icon SVG");
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(ICON_SIDE, ICON_SIDE).expect("an icon-sized pixmap");
    let size = tree.size();
    // Scaled by the width alone: a non-square SVG would come out cropped.
    assert_eq!(size.width(), size.height(), "{ICON_SVG} must be square");
    let scale = ICON_SIDE as f32 / size.width();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia keeps colour premultiplied by alpha.
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("icon.rgba"), rgba).expect("write the icon");
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
