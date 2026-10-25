use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;

use noido_engine::PdfBytes;

/// Reads the file at `path`, with a user-facing message on failure.
pub fn load_document(path: &Path) -> Result<PdfBytes, String> {
    let shown = path.display();
    if path.is_dir() {
        return Err(format!("{shown}: is a directory"));
    }
    let bytes = std::fs::read(path).map_err(|e| match e.kind() {
        ErrorKind::NotFound => format!("{shown}: no such file"),
        _ => format!("{shown}: {e}"),
    })?;
    if bytes.is_empty() {
        return Err(format!("{shown}: file is empty"));
    }
    Ok(Arc::new(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("noido-load-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file() {
        let path = temp_dir("missing").join("nope.pdf");
        let err = load_document(&path).unwrap_err();
        assert!(err.ends_with(": no such file"), "{err}");
    }

    #[test]
    fn directory() {
        let err = load_document(&temp_dir("dir")).unwrap_err();
        assert!(err.ends_with(": is a directory"), "{err}");
    }

    #[test]
    fn empty_file() {
        let path = temp_dir("empty").join("empty.pdf");
        std::fs::write(&path, b"").unwrap();
        let err = load_document(&path).unwrap_err();
        assert!(err.ends_with(": file is empty"), "{err}");
    }

    #[test]
    fn reads_fixture() {
        let path = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../fixtures/two-pages.pdf"
        ));
        let bytes = load_document(path).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
    }
}
