//! The macOS app bundle, `noido.app`, as files on disk: the `Info.plist`
//! text, reading its identity back, and writing the bundle. Pure file work,
//! so it is tested on every platform.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// The bundle identifier Launch Services knows noido by.
pub const IDENTIFIER: &str = "io.github.rikardg.noido";

/// The bundle's directory name.
pub const APP_NAME: &str = "noido.app";

/// The `Info.plist` of a bundle holding noido `version`: a viewer for PDFs
/// (ranked below the default) and the owner of the `noido` URL scheme.
pub fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
<key>CFBundleExecutable</key><string>noido</string>
<key>CFBundleIdentifier</key><string>{IDENTIFIER}</string>
<key>CFBundleName</key><string>noido</string>
<key>CFBundleDisplayName</key><string>noido</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>CFBundleDocumentTypes</key>
<array>
<dict>
<key>CFBundleTypeName</key><string>PDF document</string>
<key>CFBundleTypeRole</key><string>Viewer</string>
<key>LSHandlerRank</key><string>Alternate</string>
<key>LSItemContentTypes</key><array><string>com.adobe.pdf</string></array>
</dict>
</array>
<key>CFBundleURLTypes</key>
<array>
<dict>
<key>CFBundleURLName</key><string>noido</string>
<key>CFBundleURLSchemes</key><array><string>noido</string></array>
</dict>
</array>
</dict>
</plist>
"#
    )
}

/// What an installed bundle says about itself.
#[derive(Debug, PartialEq)]
pub struct Identity {
    pub identifier: String,
    pub version: String,
}

/// The identifier and version in a plist's top-level dict; `None` when it
/// is not XML or lacks either.
pub fn identity(plist_xml: &str) -> Option<Identity> {
    let doc = roxmltree::Document::parse(plist_xml).ok()?;
    let dict = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("dict"))?;
    let value = |key: &str| {
        let mut elements = dict.children().filter(|n| n.is_element());
        while let Some(k) = elements.next() {
            let v = elements.next()?;
            if k.has_tag_name("key") && k.text() == Some(key) && v.has_tag_name("string") {
                return v.text().map(str::to_owned);
            }
        }
        None
    };
    Some(Identity {
        identifier: value("CFBundleIdentifier")?,
        version: value("CFBundleShortVersionString")?,
    })
}

/// The identity of the bundle at `app`, `None` when its plist is unreadable.
pub fn read_identity(app: &Path) -> Option<Identity> {
    identity(&std::fs::read_to_string(app.join("Contents/Info.plist")).ok()?)
}

/// Writes the bundle at `app`, replacing whatever is there: the plist and
/// a copy of `exe` (never a link) as `Contents/MacOS/noido`.
pub fn write_bundle(exe: &Path, app: &Path, version: &str) -> Result<(), String> {
    let fail =
        |what: &str, path: &Path, e: std::io::Error| format!("{what} {}: {e}", path.display());
    if std::fs::symlink_metadata(app).is_ok() {
        std::fs::remove_dir_all(app).map_err(|e| fail("cannot remove", app, e))?;
    }
    let macos = app.join("Contents/MacOS");
    std::fs::create_dir_all(&macos).map_err(|e| fail("cannot create", &macos, e))?;
    let plist = app.join("Contents/Info.plist");
    std::fs::write(&plist, info_plist(version)).map_err(|e| fail("cannot write", &plist, e))?;
    let target = macos.join("noido");
    std::fs::copy(exe, &target)
        .map_err(|e| format!("cannot copy {} to {}: {e}", exe.display(), target.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| fail("cannot make executable", &target, e))?;
    }
    Ok(())
}

/// The `X.app` when `exe` is `X.app/Contents/MacOS/<name>`, judged by the
/// path's components alone.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    let is = |p: &Path, name: &str| p.file_name().is_some_and(|n| n == name);
    let is_app = app.extension().is_some_and(|e| e == "app") && app.file_stem().is_some();
    (is(macos, "MacOS") && is(contents, "Contents") && is_app).then(|| app.to_path_buf())
}

/// Whether `dir` is in a temporary location, where a bundle would vanish.
pub fn in_temp_dir(dir: &Path) -> bool {
    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let temp = std::env::temp_dir();
    let temp = temp.canonicalize().unwrap_or(temp);
    dir.starts_with(&temp)
        || [
            "/tmp",
            "/private/tmp",
            "/var/folders",
            "/private/var/folders",
        ]
        .iter()
        .any(|t| dir.starts_with(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_is_xml_and_declares_pdfs_and_the_scheme() {
        let xml = info_plist("1.2.3");
        roxmltree::Document::parse(&xml).unwrap();
        for wanted in [
            "<key>CFBundleExecutable</key><string>noido</string>",
            "<key>CFBundleIdentifier</key><string>io.github.rikardg.noido</string>",
            "<key>CFBundleShortVersionString</key><string>1.2.3</string>",
            "<key>CFBundlePackageType</key><string>APPL</string>",
            "<key>LSMinimumSystemVersion</key><string>11.0</string>",
            "<string>com.adobe.pdf</string>",
            "<key>LSHandlerRank</key><string>Alternate</string>",
            "<key>CFBundleTypeRole</key><string>Viewer</string>",
            "<key>CFBundleURLSchemes</key><array><string>noido</string></array>",
        ] {
            assert!(xml.contains(wanted), "missing {wanted}");
        }
        assert!(!xml.contains("CFBundleTypeExtensions"));
    }

    #[test]
    fn identity_reads_back_what_the_plist_says() {
        assert_eq!(
            identity(&info_plist("1.2.3")),
            Some(Identity {
                identifier: IDENTIFIER.into(),
                version: "1.2.3".into()
            })
        );
        assert_eq!(identity("not xml"), None);
    }

    #[test]
    fn bundle_layout() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("fake-noido");
        std::fs::write(&exe, b"binary bytes").unwrap();
        let app = dir.path().join(APP_NAME);
        write_bundle(&exe, &app, "1.2.3").unwrap();
        let copy = app.join("Contents/MacOS/noido");
        let meta = std::fs::symlink_metadata(&copy).unwrap();
        assert!(meta.file_type().is_file());
        assert_eq!(std::fs::read(&copy).unwrap(), b"binary bytes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(meta.permissions().mode() & 0o111, 0);
        }
        assert_eq!(read_identity(&app).unwrap().version, "1.2.3");
    }

    #[test]
    fn rewriting_a_bundle_replaces_it_whole() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("fake-noido");
        std::fs::write(&exe, b"x").unwrap();
        let app = dir.path().join(APP_NAME);
        write_bundle(&exe, &app, "1.0.0").unwrap();
        let old = app.join("Contents/Resources/old");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, b"old").unwrap();
        write_bundle(&exe, &app, "1.2.3").unwrap();
        assert!(!old.exists());
        assert_eq!(read_identity(&app).unwrap().version, "1.2.3");
    }

    #[test]
    fn bundle_of_finds_the_app_by_its_path() {
        assert_eq!(
            bundle_of(Path::new("/A/noido.app/Contents/MacOS/noido")),
            Some(PathBuf::from("/A/noido.app"))
        );
        assert_eq!(bundle_of(Path::new("/opt/homebrew/bin/noido")), None);
        assert_eq!(bundle_of(Path::new("/A/noido/Contents/MacOS/noido")), None);
        assert_eq!(bundle_of(Path::new("/A/noido.app/Contents/noido")), None);
    }

    #[test]
    fn temp_dirs_are_recognised() {
        assert!(in_temp_dir(Path::new("/tmp/x")));
        assert!(in_temp_dir(Path::new("/private/var/folders/ab/T/x")));
        assert!(in_temp_dir(&std::env::temp_dir().join("x")));
        assert!(!in_temp_dir(Path::new("/Users/someone/repo/target/app")));
    }
}
