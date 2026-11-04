//! The macOS app bundle, `noido.app`, as files on disk: the `Info.plist`
//! text, reading its identity back, and writing the bundle. Pure file work,
//! so it is tested on every platform.
// Off macOS only the tests reach most of this.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::{Path, PathBuf};

/// The bundle identifier Launch Services knows noido by.
pub const IDENTIFIER: &str = "io.github.rikardg.noido";

/// The bundle's directory name.
pub const APP_NAME: &str = "noido.app";

/// The bundle's icon, `Contents/Resources/noido.icns`, rendered by build.rs
/// from `assets/noido.svg`.
const ICON_FILE: &str = "noido.icns";
const ICON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/noido.icns"));

/// The `Info.plist` of a bundle holding noido `version`: a viewer for PDFs
/// (ranked below the default) and the owner of the `noido` URL scheme.
///
/// `version` is the full one, commit included (`0.1.0+69fa5bc6`). macOS
/// gets only the numbers before the `+`; the full version goes in a key of
/// noido's own, which is what tells two builds apart.
pub fn info_plist(version: &str) -> String {
    let numbers = version.split('+').next().unwrap_or(version);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
<key>CFBundleExecutable</key><string>noido</string>
<key>CFBundleIdentifier</key><string>{IDENTIFIER}</string>
<key>CFBundleName</key><string>noido</string>
<key>CFBundleDisplayName</key><string>noido</string>
<key>CFBundleIconFile</key><string>noido</string>
<key>CFBundleShortVersionString</key><string>{numbers}</string>
<key>NoidoVersion</key><string>{version}</string>
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
/// is not XML or lacks either. The version is the full one when the bundle
/// has it, as an older bundle doesn't.
pub fn identity(plist_xml: &str) -> Option<Identity> {
    // Tools that rewrite a plist add a DOCTYPE.
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(plist_xml, options).ok()?;
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
        version: value("NoidoVersion").or_else(|| value("CFBundleShortVersionString"))?,
    })
}

/// The identity of the bundle at `app`, `None` when its plist is unreadable.
pub fn read_identity(app: &Path) -> Option<Identity> {
    identity(&std::fs::read_to_string(app.join("Contents/Info.plist")).ok()?)
}

/// The hint for a terminal `noido` when the installed noido.app is noido's
/// at another version than `this_version`.
pub fn stale_hint(installed: Option<&Identity>, this_version: &str) -> Option<String> {
    let installed =
        installed.filter(|i| i.identifier == IDENTIFIER && i.version != this_version)?;
    Some(format!(
        "noido.app in ~/Applications is version {}, this is {this_version}; noido --install updates it",
        installed.version
    ))
}

/// Writes the bundle at `app`, replacing whatever is there: the plist, the
/// icon, and a copy of `exe` (never a link) as `Contents/MacOS/noido`.
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
    let resources = app.join("Contents/Resources");
    std::fs::create_dir_all(&resources).map_err(|e| fail("cannot create", &resources, e))?;
    let icon = resources.join(ICON_FILE);
    std::fs::write(&icon, ICON).map_err(|e| fail("cannot write", &icon, e))?;
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

/// The program that signs a bundle.
pub const CODESIGN: &str = "codesign";

/// Signs `app` ad hoc: macOS won't run an unsigned arm64 binary.
pub fn sign(app: &Path) -> Result<(), String> {
    let status = std::process::Command::new(CODESIGN)
        .args(["-s", "-", "--force"])
        .arg(app)
        .status()
        .map_err(|e| format!("cannot run {CODESIGN}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{CODESIGN} failed: {status}"))
    }
}

/// Whether `exe` lies inside `app`: writing the bundle would delete it.
fn runs_from(exe: &Path, app: &Path) -> bool {
    let app = app.canonicalize().unwrap_or_else(|_| app.to_path_buf());
    exe.starts_with(app)
}

/// Builds `dir/noido.app` from the running binary and signs it.
#[cfg(target_os = "macos")]
pub fn app_bundle(dir: &Path) -> Result<(), String> {
    if in_temp_dir(dir) {
        return Err(format!(
            "{} is in a temporary directory, where macOS won't run apps",
            dir.display()
        ));
    }
    let exe = std::env::current_exe()
        .and_then(|e| e.canonicalize())
        .map_err(|e| format!("cannot find the running binary: {e}"))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let app = dir.join(APP_NAME);
    if runs_from(&exe, &app) {
        return Err(format!(
            "cannot replace {}, the app it is running from",
            app.display()
        ));
    }
    write_bundle(&exe, &app, env!("NOIDO_VERSION"))?;
    sign(&app)?;
    println!("{}", app.display());
    Ok(())
}

/// Launch Services' command line tool.
pub const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

/// Where things are: the two Applications folders, the binary to copy
/// and its version.
pub struct Places {
    pub user_apps: PathBuf,
    pub system_apps: PathBuf,
    pub exe: PathBuf,
    pub version: String,
}

/// One change to the system.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Remove the bundle; `version` is the one found there.
    Remove {
        app: PathBuf,
        version: Option<String>,
        /// Same version, replaced only because its icon is not this build's.
        stale_icon: bool,
    },
    Write {
        app: PathBuf,
        exe: PathBuf,
        version: String,
    },
    Sign(PathBuf),
    Register(PathBuf),
    Unregister(PathBuf),
}

/// What a command will do.
#[derive(Debug, PartialEq)]
pub enum Plan {
    /// Nothing to do; says why.
    Nothing(String),
    Steps(Vec<Step>),
}

enum Found {
    Missing,
    Ours(String),
    Foreign,
}

fn found(app: &Path) -> Found {
    if std::fs::symlink_metadata(app).is_err() {
        return Found::Missing;
    }
    match read_identity(app) {
        Some(id) if id.identifier == IDENTIFIER => Found::Ours(id.version),
        _ => Found::Foreign,
    }
}

pub fn plan_install(places: &Places) -> Plan {
    if places.system_apps.join(APP_NAME).exists() {
        return Plan::Nothing(format!(
            "{APP_NAME} is already in {}; remove it first, or use that one",
            places.system_apps.display()
        ));
    }
    let app = places.user_apps.join(APP_NAME);
    if runs_from(&places.exe, &app) {
        return Plan::Nothing(format!(
            "noido is running from {}; run --install from another noido binary",
            app.display()
        ));
    }
    let mut steps = Vec::new();
    match found(&app) {
        Found::Missing => {}
        Found::Ours(v) => {
            let stale_icon = v == places.version;
            if stale_icon && has_our_icon(&app) {
                return Plan::Nothing(format!("Already installed: {}, version {v}", app.display()));
            }
            steps.push(Step::Remove {
                app: app.clone(),
                version: Some(v),
                stale_icon,
            });
        }
        Found::Foreign => return foreign(&app),
    }
    steps.push(Step::Write {
        app: app.clone(),
        exe: places.exe.clone(),
        version: places.version.clone(),
    });
    steps.push(Step::Sign(app.clone()));
    steps.push(Step::Register(app));
    Plan::Steps(steps)
}

pub fn plan_uninstall(places: &Places) -> Plan {
    let app = places.user_apps.join(APP_NAME);
    match found(&app) {
        Found::Missing => Plan::Nothing(format!("Not installed: no {}", app.display())),
        Found::Foreign => foreign(&app),
        Found::Ours(v) => Plan::Steps(vec![
            Step::Unregister(app.clone()),
            Step::Remove {
                app,
                version: Some(v),
                stale_icon: false,
            },
        ]),
    }
}

/// Whether the bundle at `app` holds this build's icon: a bundle from
/// before the icon, or from a build with another one, does not.
fn has_our_icon(app: &Path) -> bool {
    std::fs::read(app.join("Contents/Resources").join(ICON_FILE)).is_ok_and(|icon| icon == ICON)
}

fn foreign(app: &Path) -> Plan {
    Plan::Nothing(format!("{} is not noido's; leaving it", app.display()))
}

/// One line for the list shown before asking.
pub fn describe(step: &Step) -> String {
    match step {
        Step::Write { app, version, .. } => format!("Write {} (noido {version})", app.display()),
        Step::Remove {
            app,
            version: Some(v),
            stale_icon,
        } => {
            let why = if *stale_icon {
                "; its icon is out of date"
            } else {
                ""
            };
            format!("Remove {} (noido {v}{why})", app.display())
        }
        Step::Remove {
            app, version: None, ..
        } => format!("Remove {}", app.display()),
        Step::Sign(app) => format!("Sign {} ad hoc ({CODESIGN})", app.display()),
        Step::Register(_) => "Register it with Launch Services (lsregister)".into(),
        Step::Unregister(app) => format!("Unregister {} from Launch Services", app.display()),
    }
}

/// Carries out the steps in order, stopping at the first failure. Launch
/// Services steps that fail are notes: Finder finds the app anyway.
pub fn carry_out(steps: &[Step], lsregister: &str) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    let mut launch_services = |flag: &str, app: &Path| match std::process::Command::new(lsregister)
        .arg(flag)
        .arg(app)
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => notes.push(format!("lsregister failed ({status})")),
        Err(e) => notes.push(format!("Skipped lsregister: {e}")),
    };
    for step in steps {
        match step {
            Step::Remove { app, .. } => std::fs::remove_dir_all(app)
                .map_err(|e| format!("cannot remove {}: {e}", app.display()))?,
            Step::Write { app, exe, version } => write_bundle(exe, app, version)?,
            Step::Sign(app) => sign(app)?,
            Step::Register(app) => launch_services("-f", app),
            Step::Unregister(app) => launch_services("-u", app),
        }
    }
    Ok(notes)
}

#[cfg(target_os = "macos")]
impl Places {
    fn here() -> Result<Self, String> {
        let home = std::env::home_dir().ok_or("cannot find your home directory")?;
        let exe = std::env::current_exe()
            .and_then(|exe| exe.canonicalize())
            .map_err(|e| format!("cannot find the noido binary: {e}"))?;
        Ok(Self {
            user_apps: home.join("Applications"),
            system_apps: PathBuf::from("/Applications"),
            exe,
            version: env!("NOIDO_VERSION").into(),
        })
    }
}

/// Shows the steps, asks, and carries them out on a yes; `true` when done.
#[cfg(target_os = "macos")]
fn proceed(plan: Plan) -> Result<bool, String> {
    let steps = match plan {
        Plan::Nothing(why) => {
            println!("{why}. Nothing to do.");
            return Ok(false);
        }
        Plan::Steps(steps) => steps,
    };
    let lines: Vec<String> = steps.iter().map(describe).collect();
    if !crate::desktop::ask_to_proceed(&lines)? {
        println!("Nothing changed.");
        return Ok(false);
    }
    for note in carry_out(&steps, LSREGISTER)? {
        println!("{note}");
    }
    println!("Done.");
    Ok(true)
}

/// `noido --install` on macOS.
#[cfg(target_os = "macos")]
pub fn install() -> Result<(), String> {
    let places = Places::here()?;
    println!(
        "This builds noido.app in ~/Applications from {}, so Finder offers noido for PDFs. Your default PDF app stays as it is.",
        places.exe.display()
    );
    if proceed(plan_install(&places))? {
        println!();
        println!(
            "To open PDFs with noido by default: select a PDF in Finder, Get Info, Open With: noido, Change All\u{2026}"
        );
    }
    Ok(())
}

/// `noido --uninstall` on macOS.
#[cfg(target_os = "macos")]
pub fn uninstall() -> Result<(), String> {
    let places = Places::here()?;
    println!("This removes noido.app from ~/Applications.");
    proceed(plan_uninstall(&places)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_reads_a_plist_with_a_doctype() {
        // What `plutil -replace` and Xcode write.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>a.b</string>
<key>CFBundleShortVersionString</key><string>1.2</string>
</dict></plist>"#;
        assert_eq!(
            identity(xml),
            Some(Identity {
                identifier: "a.b".into(),
                version: "1.2".into()
            })
        );
    }

    #[test]
    fn stale_hint_only_for_our_other_version() {
        let id = |identifier: &str, version: &str| Identity {
            identifier: identifier.into(),
            version: version.into(),
        };
        assert_eq!(stale_hint(None, "0.2.0"), None);
        let current = id(IDENTIFIER, env!("NOIDO_VERSION"));
        assert_eq!(stale_hint(Some(&current), env!("NOIDO_VERSION")), None);
        assert!(stale_hint(Some(&id(IDENTIFIER, "0.2.0+aaaaaaaa")), "0.2.0+bbbbbbbb").is_some());
        assert_eq!(
            stale_hint(Some(&id("com.example.other", "0.0.1")), "0.2.0"),
            None
        );
        assert_eq!(
            stale_hint(Some(&id(IDENTIFIER, "0.0.1")), "0.2.0").as_deref(),
            Some(
                "noido.app in ~/Applications is version 0.0.1, this is 0.2.0; noido --install updates it"
            )
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn app_bundle_refuses_a_temp_dir() {
        let dir = std::env::temp_dir().join("noido-app-bundle-refused");
        let e = app_bundle(&dir).unwrap_err();
        assert!(e.contains("temporary directory"), "{e}");
        assert!(!dir.exists());
    }

    #[test]
    fn the_running_app_is_not_replaced() {
        let dir = std::env::current_dir().unwrap();
        let app = dir.join("Cargo.toml");
        assert!(runs_from(&app.join("Contents/MacOS/noido"), &app));
        assert!(!runs_from(&dir.join("target/noido"), &app));
    }

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

    #[cfg(target_os = "macos")]
    #[test]
    fn install_compares_the_full_version_this_build_stamps() {
        let version = Places::here().unwrap().version;
        assert_eq!(version, env!("NOIDO_VERSION"));
        assert!(version.contains('+'), "{version} lacks the commit");
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
    fn a_bundle_from_before_the_full_version_counts_as_another_version() {
        // An installed noido.app written before NoidoVersion existed.
        let old = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{IDENTIFIER}</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
</dict></plist>"#
        );
        let installed = identity(&old).unwrap();
        assert_eq!(installed.version, "0.1.0");
        assert!(stale_hint(Some(&installed), "0.1.0+abcdef12").is_some());
    }

    #[test]
    fn the_plist_carries_the_full_version_and_macos_the_numbers() {
        let xml = info_plist("1.2.3+abcdef12");
        assert!(xml.contains("<key>CFBundleShortVersionString</key><string>1.2.3</string>"));
        assert!(xml.contains("<key>NoidoVersion</key><string>1.2.3+abcdef12</string>"));
        assert!(!xml.contains("CFBundleVersion<"));
        assert_eq!(identity(&xml).unwrap().version, "1.2.3+abcdef12");
    }

    #[test]
    fn the_plist_names_the_icon() {
        assert!(info_plist("1.2.3").contains("<key>CFBundleIconFile</key><string>noido</string>"));
    }

    /// The entries of an `.icns`: (type, data), checking the lengths.
    fn icns_entries(icns: &[u8]) -> Vec<(String, &[u8])> {
        assert_eq!(&icns[..4], b"icns");
        let total = u32::from_be_bytes(icns[4..8].try_into().unwrap()) as usize;
        assert_eq!(total, icns.len());
        let mut entries = Vec::new();
        let mut at = 8;
        while at < total {
            let kind = String::from_utf8(icns[at..at + 4].to_vec()).unwrap();
            let len = u32::from_be_bytes(icns[at + 4..at + 8].try_into().unwrap()) as usize;
            assert!(len > 8 && at + len <= total, "{kind}: length {len}");
            entries.push((kind, &icns[at + 8..at + len]));
            at += len;
        }
        entries
    }

    #[test]
    fn the_icon_holds_a_png_of_each_size() {
        let entries = icns_entries(ICON);
        let wanted = [
            ("icp4", 16),
            ("icp5", 32),
            ("ic11", 32),
            ("ic12", 64),
            ("ic07", 128),
            ("ic13", 256),
            ("ic08", 256),
            ("ic14", 512),
            ("ic09", 512),
            ("ic10", 1024),
        ];
        let kinds: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(kinds, wanted.map(|(k, _)| k));
        for ((kind, png), (_, side)) in entries.iter().zip(wanted) {
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "{kind} is not a PNG");
            let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
            let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
            assert_eq!((width, height), (side, side), "{kind}");
        }
    }

    #[test]
    fn install_same_version_without_the_icon_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let places = places(dir.path());
        install_at(&places, "2.0.0");
        std::fs::remove_file(app_of(&places).join("Contents/Resources/noido.icns")).unwrap();
        let mut steps = vec![Step::Remove {
            app: app_of(&places),
            version: Some("2.0.0".into()),
            stale_icon: true,
        }];
        steps.extend(fresh_steps(&places));
        assert_eq!(plan_install(&places), Plan::Steps(steps));
    }

    #[test]
    fn install_same_version_with_another_icon_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let places = places(dir.path());
        install_at(&places, "2.0.0");
        std::fs::write(
            app_of(&places).join("Contents/Resources/noido.icns"),
            b"old",
        )
        .unwrap();
        let mut steps = vec![Step::Remove {
            app: app_of(&places),
            version: Some("2.0.0".into()),
            stale_icon: true,
        }];
        steps.extend(fresh_steps(&places));
        assert_eq!(plan_install(&places), Plan::Steps(steps));
    }

    #[test]
    fn install_from_another_commit_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let mut places = places(dir.path());
        places.version = "2.0.0+bbbbbbbb".into();
        install_at(&places, "2.0.0+aaaaaaaa");
        let mut steps = vec![Step::Remove {
            app: app_of(&places),
            version: Some("2.0.0+aaaaaaaa".into()),
            stale_icon: false,
        }];
        steps.extend(fresh_steps(&places));
        assert_eq!(plan_install(&places), Plan::Steps(steps));
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
        assert_eq!(
            std::fs::read(app.join("Contents/Resources/noido.icns")).unwrap(),
            ICON
        );
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

    fn places(dir: &Path) -> Places {
        Places {
            user_apps: dir.join("user"),
            system_apps: dir.join("system"),
            exe: dir.join("noido-binary"),
            version: "2.0.0".into(),
        }
    }

    fn install_at(places: &Places, version: &str) {
        std::fs::write(&places.exe, b"bin").unwrap();
        write_bundle(&places.exe, &places.user_apps.join(APP_NAME), version).unwrap();
    }

    fn app_of(places: &Places) -> PathBuf {
        places.user_apps.join(APP_NAME)
    }

    fn fresh_steps(places: &Places) -> Vec<Step> {
        let app = app_of(places);
        vec![
            Step::Write {
                app: app.clone(),
                exe: places.exe.clone(),
                version: places.version.clone(),
            },
            Step::Sign(app.clone()),
            Step::Register(app),
        ]
    }

    #[test]
    fn install_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let places = places(dir.path());
        assert_eq!(plan_install(&places), Plan::Steps(fresh_steps(&places)));
    }

    #[test]
    fn install_same_version_is_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let places = places(dir.path());
        install_at(&places, "2.0.0");
        assert_eq!(
            plan_install(&places),
            Plan::Nothing(format!(
                "Already installed: {}, version 2.0.0",
                app_of(&places).display()
            ))
        );
    }

    #[test]
    fn install_other_version_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let places = places(dir.path());
        install_at(&places, "1.0.0");
        let mut steps = vec![Step::Remove {
            app: app_of(&places),
            version: Some("1.0.0".into()),
            stale_icon: false,
        }];
        steps.extend(fresh_steps(&places));
        assert_eq!(plan_install(&places), Plan::Steps(steps));
    }

    #[test]
    fn install_stops_for_system_app() {
        let dir = tempfile::tempdir().unwrap();
        let places = places(dir.path());
        std::fs::create_dir_all(places.system_apps.join(APP_NAME)).unwrap();
        assert_eq!(
            plan_install(&places),
            Plan::Nothing(format!(
                "noido.app is already in {}; remove it first, or use that one",
                places.system_apps.display()
            ))
        );
    }

    #[test]
    fn install_stops_when_running_from_the_installed_app() {
        let dir = tempfile::tempdir().unwrap();
        let mut places = places(dir.path());
        install_at(&places, "1.0.0");
        let app = app_of(&places).canonicalize().unwrap();
        places.exe = app.join("Contents/MacOS/noido");
        assert_eq!(
            plan_install(&places),
            Plan::Nothing(format!(
                "noido is running from {}; run --install from another noido binary",
                app_of(&places).display()
            ))
        );
    }

    fn someone_elses_app(places: &Places) {
        let contents = app_of(places).join("Contents");
        std::fs::create_dir_all(&contents).unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            info_plist("1.0.0").replace(IDENTIFIER, "com.example.other"),
        )
        .unwrap();
    }

    #[test]
    fn install_leaves_someone_elses_app() {
        let dir = tempfile::tempdir().unwrap();
        let places = places(dir.path());
        someone_elses_app(&places);
        assert_eq!(
            plan_install(&places),
            Plan::Nothing(format!(
                "{} is not noido's; leaving it",
                app_of(&places).display()
            ))
        );
    }

    #[test]
    fn uninstall_ours() {
        let dir = tempfile::tempdir().unwrap();
        let places = places(dir.path());
        install_at(&places, "1.0.0");
        let app = app_of(&places);
        assert_eq!(
            plan_uninstall(&places),
            Plan::Steps(vec![
                Step::Unregister(app.clone()),
                Step::Remove {
                    app,
                    version: Some("1.0.0".into()),
                    stale_icon: false,
                }
            ])
        );
    }

    #[test]
    fn uninstall_missing_is_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let places = places(dir.path());
        assert_eq!(
            plan_uninstall(&places),
            Plan::Nothing(format!("Not installed: no {}", app_of(&places).display()))
        );
    }

    #[test]
    fn uninstall_leaves_someone_elses_app() {
        let dir = tempfile::tempdir().unwrap();
        let places = places(dir.path());
        someone_elses_app(&places);
        assert_eq!(
            plan_uninstall(&places),
            Plan::Nothing(format!(
                "{} is not noido's; leaving it",
                app_of(&places).display()
            ))
        );
    }

    #[test]
    fn describe_says_which_version_replaces_which() {
        let app = PathBuf::from("/U/noido.app");
        assert_eq!(
            describe(&Step::Remove {
                app: app.clone(),
                version: Some("1.0.0".into()),
                stale_icon: false,
            }),
            "Remove /U/noido.app (noido 1.0.0)"
        );
        assert_eq!(
            describe(&Step::Remove {
                app: app.clone(),
                version: Some("1.0.0".into()),
                stale_icon: true,
            }),
            "Remove /U/noido.app (noido 1.0.0; its icon is out of date)"
        );
        assert_eq!(
            describe(&Step::Write {
                app: app.clone(),
                exe: "/x".into(),
                version: "2.0.0".into()
            }),
            "Write /U/noido.app (noido 2.0.0)"
        );
        assert_eq!(
            describe(&Step::Sign(app.clone())),
            "Sign /U/noido.app ad hoc (codesign)"
        );
        assert_eq!(
            describe(&Step::Register(app.clone())),
            "Register it with Launch Services (lsregister)"
        );
        assert_eq!(
            describe(&Step::Unregister(app)),
            "Unregister /U/noido.app from Launch Services"
        );
    }

    #[test]
    fn a_failing_registration_is_a_note_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join(APP_NAME);
        let notes = carry_out(
            &[Step::Register(app.clone()), Step::Unregister(app)],
            "noido-no-such-tool",
        )
        .unwrap();
        assert_eq!(notes.len(), 2, "{notes:?}");
    }

    #[test]
    fn carrying_out_writes_then_removes_the_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let places = places(dir.path());
        std::fs::write(&places.exe, b"bin").unwrap();
        let app = app_of(&places);
        // Skip signing and registering: only the file steps.
        let steps = [Step::Write {
            app: app.clone(),
            exe: places.exe.clone(),
            version: "2.0.0".into(),
        }];
        carry_out(&steps, "noido-no-such-tool").unwrap();
        assert_eq!(read_identity(&app).unwrap().version, "2.0.0");
        let remove = [Step::Remove {
            app: app.clone(),
            version: None,
            stale_icon: false,
        }];
        carry_out(&remove, "noido-no-such-tool").unwrap();
        assert!(!app.exists());
    }
}
