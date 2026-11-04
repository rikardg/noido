//! Stamps the commit into `noido --version`: `0.1.0+deadbeef`, and renders
//! the window icon and the macOS app's `.icns` from `assets/noido.svg`.
//!
//! The commit comes in `NOIDO_GIT_SHA`: from the workflow for a release,
//! and from the justfile, which reads the working copy, for every `just`
//! recipe. The rerun line is what makes the stamp change: a new commit is a
//! new value, so the script runs again. Without the variable the script
//! asks git, but only when it runs for another reason (build.rs or the icon
//! changed, a clean build); so a bare `cargo build` keeps the commit of the
//! last time it ran. The `+` is SemVer build metadata, which never changes
//! how two versions compare, so a snapshot is still "0.1.0" to anything
//! that cares.

use std::path::{Path, PathBuf};
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
    println!("cargo:rerun-if-changed={ICON_SVG}");
    let svg = std::fs::read(ICON_SVG).expect("read the icon SVG");
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())
        .expect("parse the icon SVG");
    let size = tree.size();
    // Scaled by the width alone: a non-square SVG would come out cropped.
    assert_eq!(size.width(), size.height(), "{ICON_SVG} must be square");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    write_window_icon(&tree, &out);
    write_icns(&tree, &out);
}

/// The SVG rendered as a `side` × `side` pixmap.
fn render(tree: &resvg::usvg::Tree, side: u32) -> tiny_skia::Pixmap {
    let mut pixmap = tiny_skia::Pixmap::new(side, side).expect("an icon-sized pixmap");
    let scale = side as f32 / tree.size().width();
    resvg::render(
        tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
}

/// The window icon, `$OUT_DIR/icon.rgba`: straight RGBA, as winit wants it.
fn write_window_icon(tree: &resvg::usvg::Tree, out: &Path) {
    // tiny-skia keeps colour premultiplied by alpha.
    let rgba: Vec<u8> = render(tree, ICON_SIDE)
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    std::fs::write(out.join("icon.rgba"), rgba).expect("write the icon");
}

/// The macOS app's icon, `$OUT_DIR/noido.icns`, packed here rather than by
/// `iconutil` so it builds anywhere: a header, then one entry per type,
/// each a PNG of the type's size. All lengths are big-endian and count
/// their own 8-byte header.
const ICNS_TYPES: [(&[u8; 4], u32); 10] = [
    (b"icp4", 16),
    (b"icp5", 32),
    (b"ic11", 32), // 16 @2x
    (b"ic12", 64), // 32 @2x
    (b"ic07", 128),
    (b"ic13", 256), // 128 @2x
    (b"ic08", 256),
    (b"ic14", 512), // 256 @2x
    (b"ic09", 512),
    (b"ic10", 1024), // 512 @2x
];

fn write_icns(tree: &resvg::usvg::Tree, out: &Path) {
    let mut body = Vec::new();
    for (kind, side) in ICNS_TYPES {
        let png = render(tree, side).encode_png().expect("encode an icon PNG");
        body.extend_from_slice(kind);
        body.extend_from_slice(&(png.len() as u32 + 8).to_be_bytes());
        body.extend_from_slice(&png);
    }
    let mut icns = b"icns".to_vec();
    icns.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
    icns.extend_from_slice(&body);
    std::fs::write(out.join("noido.icns"), icns).expect("write the icns");
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
