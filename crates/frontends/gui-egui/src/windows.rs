//! The open document windows, one per file, in open order.

use std::path::{Path, PathBuf};

use eframe::egui::ViewportId;

/// Open windows keyed by canonical path.
// Until the shell uses it.
#[allow(dead_code)]
pub struct Windows<W> {
    entries: Vec<(PathBuf, W)>,
}

#[allow(dead_code)]
impl<W> Windows<W> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn id_for(canonical: &Path) -> ViewportId {
        ViewportId::from_hash_of(canonical)
    }

    pub fn find(&self, canonical: &Path) -> Option<ViewportId> {
        self.entries
            .iter()
            .any(|(p, _)| p == canonical)
            .then(|| Self::id_for(canonical))
    }

    pub fn insert(&mut self, canonical: PathBuf, window: W) -> ViewportId {
        let id = Self::id_for(&canonical);
        self.entries.push((canonical, window));
        id
    }

    pub fn remove(&mut self, id: ViewportId) -> Option<W> {
        let at = self
            .entries
            .iter()
            .position(|(p, _)| Self::id_for(p) == id)?;
        Some(self.entries.remove(at).1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (ViewportId, &W)> {
        self.entries.iter().map(|(p, w)| (Self::id_for(p), w))
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.entries.iter().map(|(p, _)| p.clone()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The path as the list keys it: symlinks resolved, else made absolute.
    pub fn canonical(path: &Path) -> PathBuf {
        std::fs::canonicalize(path)
            .or_else(|_| std::path::absolute(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn insert_then_find() {
        let mut w = Windows::new();
        let id = w.insert(p("/a.pdf"), 1);
        assert_eq!(w.find(&p("/a.pdf")), Some(id));
        assert_eq!(id, Windows::<i32>::id_for(&p("/a.pdf")));
        assert!(!w.is_empty());
    }

    #[test]
    fn a_second_path_is_another_window() {
        let mut w = Windows::new();
        let a = w.insert(p("/a.pdf"), 1);
        let b = w.insert(p("/b.pdf"), 2);
        assert_ne!(a, b);
        assert_eq!(w.find(&p("/b.pdf")), Some(b));
        assert_eq!(w.iter().count(), 2);
    }

    #[test]
    fn remove_forgets_the_path() {
        let mut w = Windows::new();
        let a = w.insert(p("/a.pdf"), 1);
        assert_eq!(w.remove(a), Some(1));
        assert_eq!(w.find(&p("/a.pdf")), None);
        assert!(w.is_empty());
        assert_eq!(w.remove(a), None);
    }

    #[test]
    fn paths_keep_the_open_order() {
        let mut w = Windows::new();
        w.insert(p("/b.pdf"), 1);
        w.insert(p("/a.pdf"), 2);
        w.insert(p("/c.pdf"), 3);
        assert_eq!(w.paths(), vec![p("/b.pdf"), p("/a.pdf"), p("/c.pdf")]);
        let a = Windows::<i32>::id_for(&p("/a.pdf"));
        w.remove(a);
        assert_eq!(w.paths(), vec![p("/b.pdf"), p("/c.pdf")]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_finds_the_same_window() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("doc.pdf");
        std::fs::write(&file, b"x").unwrap();
        let link = dir.path().join("link.pdf");
        std::os::unix::fs::symlink(&file, &link).unwrap();

        let mut w = Windows::new();
        let id = w.insert(Windows::<i32>::canonical(&file), 1);
        assert_eq!(w.find(&Windows::<i32>::canonical(&link)), Some(id));

        // A relative path to the same file.
        let rel = relative_to(&file, &std::env::current_dir().unwrap());
        assert!(rel.is_relative());
        assert_eq!(w.find(&Windows::<i32>::canonical(&rel)), Some(id));
    }

    /// `target` relative to `base`, with `..` as needed.
    #[cfg(unix)]
    fn relative_to(target: &Path, base: &Path) -> PathBuf {
        let t = target.canonicalize().unwrap();
        let b = base.canonicalize().unwrap();
        let t: Vec<_> = t.components().collect();
        let b: Vec<_> = b.components().collect();
        let common = t.iter().zip(&b).take_while(|(x, y)| x == y).count();
        let mut out = PathBuf::new();
        for _ in common..b.len() {
            out.push("..");
        }
        for c in &t[common..] {
            out.push(c);
        }
        out
    }
}
