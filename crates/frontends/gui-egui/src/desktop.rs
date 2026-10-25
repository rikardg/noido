//! `noido --install` / `--uninstall`: register noido with the Linux desktop
//! through a `.desktop` file, so file managers offer it for PDFs. The
//! default PDF app is the user's to choose; this never touches it.

use std::io;
use std::path::{Path, PathBuf};

/// The file's name, which is also what the desktop calls the app.
pub const FILE_NAME: &str = "noido.desktop";

/// Rebuilds the MIME type cache of an applications directory.
pub const REFRESH_TOOL: &str = "update-desktop-database";

/// Where things go: the applications directory and the binary to launch.
pub struct Places {
    pub applications: PathBuf,
    pub exe: PathBuf,
}

/// One change to the system.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Write the `.desktop` file; `replaces` when a different one is there.
    Write {
        path: PathBuf,
        contents: String,
        replaces: bool,
    },
    Remove(PathBuf),
    /// Run [`REFRESH_TOOL`] on the directory.
    Refresh(PathBuf),
}

/// What a command will do.
#[derive(Debug, PartialEq)]
pub enum Plan {
    /// Already done; says why.
    Nothing(String),
    Steps(Vec<Step>),
}

/// The `.desktop` file launching `exe`.
pub fn desktop_entry(exe: &Path) -> Result<String, String> {
    let exe = exe
        .to_str()
        .ok_or_else(|| format!("{} is not valid UTF-8", exe.display()))?;
    Ok(format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=noido\n\
         GenericName=PDF Reader\n\
         Comment=Read PDFs and take notes\n\
         Exec={} %f\n\
         Icon=x-office-document\n\
         Terminal=false\n\
         StartupNotify=true\n\
         StartupWMClass=noido\n\
         MimeType=application/pdf;\n\
         Categories=Office;Viewer;\n",
        exec_argument(exe)
    ))
}

/// `arg` as written in an `Exec` value: quoted when it holds a reserved
/// character, `%` doubled, and backslashes doubled again for the key file's
/// own escaping.
fn exec_argument(arg: &str) -> String {
    const RESERVED: &str = " \t\n\"'\\><~|&;$*?#()`";
    let quoted = if arg.chars().any(|c| RESERVED.contains(c)) {
        let mut q = String::from("\"");
        for c in arg.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                q.push('\\');
            }
            q.push(c);
        }
        q.push('"');
        q
    } else {
        arg.to_owned()
    };
    quoted.replace('%', "%%").replace('\\', "\\\\")
}

pub fn plan_install(places: &Places) -> Result<Plan, String> {
    let path = places.applications.join(FILE_NAME);
    let contents = desktop_entry(&places.exe)?;
    let existing = std::fs::read_to_string(&path).ok();
    if existing.as_deref() == Some(contents.as_str()) {
        return Ok(Plan::Nothing(format!(
            "Already installed: {}",
            path.display()
        )));
    }
    Ok(Plan::Steps(vec![
        Step::Write {
            path,
            contents,
            replaces: existing.is_some(),
        },
        Step::Refresh(places.applications.clone()),
    ]))
}

pub fn plan_uninstall(places: &Places) -> Plan {
    let path = places.applications.join(FILE_NAME);
    if !path.exists() {
        return Plan::Nothing(format!("Not installed: no {}", path.display()));
    }
    Plan::Steps(vec![
        Step::Remove(path),
        Step::Refresh(places.applications.clone()),
    ])
}

/// One line for the list shown before asking.
pub fn describe(step: &Step) -> String {
    match step {
        Step::Write {
            path,
            replaces: false,
            ..
        } => format!("Write {}", path.display()),
        Step::Write {
            path,
            replaces: true,
            ..
        } => {
            format!("Replace {} (it differs from this noido's)", path.display())
        }
        Step::Remove(path) => format!("Remove {}", path.display()),
        Step::Refresh(dir) => format!("Run {REFRESH_TOOL} {}", dir.display()),
    }
}

/// Whether the answer to "Proceed? [y/N]" goes ahead; `None` is end of input.
pub fn confirmed(answer: Option<&str>) -> bool {
    answer.is_some_and(|a| a.trim_start().starts_with(['y', 'Y']))
}

/// Carries out the steps in order. Returns notes to show, e.g. a skipped
/// refresh; stops at the first failure.
pub fn carry_out(steps: &[Step], refresh_tool: &str) -> Result<Vec<String>, String> {
    let mut notes = Vec::new();
    for step in steps {
        match step {
            Step::Write { path, contents, .. } => {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)
                        .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
                }
                std::fs::write(path, contents)
                    .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            }
            Step::Remove(path) => std::fs::remove_file(path)
                .map_err(|e| format!("cannot remove {}: {e}", path.display()))?,
            // The desktop finds the file without the cache too, so a refresh
            // that doesn't happen is a note, not a failure.
            Step::Refresh(dir) => {
                match std::process::Command::new(refresh_tool).arg(dir).status() {
                    Ok(status) if status.success() => {}
                    Ok(status) => notes.push(format!("{refresh_tool} failed ({status})")),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        notes.push(format!("Skipped {refresh_tool}: not installed"));
                    }
                    Err(e) => notes.push(format!("Skipped {refresh_tool}: {e}")),
                }
            }
        }
    }
    Ok(notes)
}

impl Places {
    /// This system's applications directory and the running binary.
    fn here() -> Result<Self, String> {
        if !cfg!(target_os = "linux") {
            return Err("--install and --uninstall are for Linux desktops only".into());
        }
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::home_dir().map(|home| home.join(".local/share")))
            .ok_or("cannot find your home directory")?;
        let exe = std::env::current_exe()
            .and_then(|exe| exe.canonicalize())
            .map_err(|e| format!("cannot find the noido binary: {e}"))?;
        Ok(Self {
            applications: data_home.join("applications"),
            exe,
        })
    }
}

/// `noido --install`.
pub fn install() -> Result<(), String> {
    let places = Places::here()?;
    println!("This adds noido to your desktop's apps for PDFs, launching");
    println!(
        "{}. Your default PDF app stays as it is.",
        places.exe.display()
    );
    proceed(plan_install(&places)?)
}

/// `noido --uninstall`.
pub fn uninstall() -> Result<(), String> {
    let places = Places::here()?;
    println!("This removes noido from your desktop's apps.");
    proceed(plan_uninstall(&places))
}

/// Shows the steps, asks, and carries them out on a yes.
fn proceed(plan: Plan) -> Result<(), String> {
    let steps = match plan {
        Plan::Nothing(why) => {
            println!("{why}. Nothing to do.");
            return Ok(());
        }
        Plan::Steps(steps) => steps,
    };
    println!();
    for step in &steps {
        println!("  {}", describe(step));
    }
    println!();
    print!("Proceed? [y/N] ");
    io::Write::flush(&mut io::stdout()).map_err(|e| e.to_string())?;
    let mut answer = String::new();
    let read = io::stdin()
        .read_line(&mut answer)
        .map_err(|e| e.to_string())?;
    if read == 0 {
        // No answer was typed, so the prompt's line is still open.
        println!();
    }
    if !confirmed((read > 0).then_some(answer.as_str())) {
        println!("Nothing changed.");
        return Ok(());
    }
    for note in carry_out(&steps, REFRESH_TOOL)? {
        println!("{note}");
    }
    println!("Done.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("noido-desktop-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn places(dir: &Path) -> Places {
        Places {
            applications: dir.join("applications"),
            exe: PathBuf::from("/opt/noido/noido"),
        }
    }

    #[test]
    fn entry_launches_the_binary_for_pdfs() {
        let entry = desktop_entry(Path::new("/opt/noido/noido")).unwrap();
        let lines: Vec<&str> = entry.lines().collect();
        assert_eq!(lines[0], "[Desktop Entry]");
        for line in [
            "Type=Application",
            "Name=noido",
            "GenericName=PDF Reader",
            "Comment=Read PDFs and take notes",
            "Exec=/opt/noido/noido %f",
            "Icon=x-office-document",
            "Terminal=false",
            "StartupNotify=true",
            "StartupWMClass=noido",
            "MimeType=application/pdf;",
            "Categories=Office;Viewer;",
        ] {
            assert!(lines.contains(&line), "missing {line:?} in\n{entry}");
        }
    }

    #[test]
    fn entry_quotes_a_path_with_reserved_characters() {
        let entry = desktop_entry(Path::new("/home/a b/$x/no\"ido")).unwrap();
        // Quoted per the Exec rules, then backslashes doubled for the file.
        assert!(
            entry.contains("Exec=\"/home/a b/\\\\$x/no\\\\\"ido\" %f\n"),
            "{entry}"
        );
    }

    #[test]
    fn entry_doubles_percent_signs() {
        let entry = desktop_entry(Path::new("/opt/100%/noido")).unwrap();
        assert!(entry.contains("Exec=/opt/100%%/noido %f\n"), "{entry}");
    }

    #[test]
    fn fresh_install_writes_the_file_and_refreshes() {
        let dir = temp_dir("fresh");
        let places = places(&dir);
        let path = places.applications.join(FILE_NAME);
        assert_eq!(
            plan_install(&places).unwrap(),
            Plan::Steps(vec![
                Step::Write {
                    path,
                    contents: desktop_entry(&places.exe).unwrap(),
                    replaces: false,
                },
                Step::Refresh(places.applications.clone()),
            ])
        );
    }

    #[test]
    fn install_over_the_same_file_does_nothing() {
        let dir = temp_dir("same");
        let places = places(&dir);
        std::fs::create_dir_all(&places.applications).unwrap();
        let contents = desktop_entry(&places.exe).unwrap();
        std::fs::write(places.applications.join(FILE_NAME), contents).unwrap();
        assert!(matches!(plan_install(&places).unwrap(), Plan::Nothing(_)));
    }

    #[test]
    fn install_over_a_different_file_replaces_it() {
        let dir = temp_dir("different");
        let places = places(&dir);
        std::fs::create_dir_all(&places.applications).unwrap();
        std::fs::write(
            places.applications.join(FILE_NAME),
            "[Desktop Entry]\nExec=/old %f\n",
        )
        .unwrap();
        let Plan::Steps(steps) = plan_install(&places).unwrap() else {
            panic!("expected steps");
        };
        assert!(matches!(steps[0], Step::Write { replaces: true, .. }));
        assert!(
            describe(&steps[0]).starts_with("Replace "),
            "{}",
            describe(&steps[0])
        );
    }

    #[test]
    fn uninstall_removes_the_file_and_refreshes() {
        let dir = temp_dir("uninstall");
        let places = places(&dir);
        let path = places.applications.join(FILE_NAME);
        std::fs::create_dir_all(&places.applications).unwrap();
        std::fs::write(&path, "x").unwrap();
        assert_eq!(
            plan_uninstall(&places),
            Plan::Steps(vec![
                Step::Remove(path),
                Step::Refresh(places.applications.clone())
            ])
        );
    }

    #[test]
    fn uninstall_without_the_file_does_nothing() {
        let dir = temp_dir("not-installed");
        assert!(matches!(plan_uninstall(&places(&dir)), Plan::Nothing(_)));
    }

    #[test]
    fn steps_describe_themselves() {
        let path = PathBuf::from("/a/noido.desktop");
        let write = Step::Write {
            path: path.clone(),
            contents: String::new(),
            replaces: false,
        };
        assert_eq!(describe(&write), "Write /a/noido.desktop");
        assert_eq!(describe(&Step::Remove(path)), "Remove /a/noido.desktop");
        assert_eq!(
            describe(&Step::Refresh(PathBuf::from("/a"))),
            "Run update-desktop-database /a"
        );
    }

    #[test]
    fn only_yes_goes_ahead() {
        for yes in ["y", "Y", "yes", "y\n", " Yes\n"] {
            assert!(confirmed(Some(yes)), "{yes:?}");
        }
        for no in ["n", "", "\n", "no", "sure"] {
            assert!(!confirmed(Some(no)), "{no:?}");
        }
        assert!(!confirmed(None));
    }

    #[test]
    fn carrying_out_install_and_uninstall() {
        let dir = temp_dir("carry-out");
        let places = places(&dir);
        let Plan::Steps(install) = plan_install(&places).unwrap() else {
            panic!("expected steps");
        };
        // A tool that doesn't exist: skipped with a note.
        let notes = carry_out(&install, "noido-no-such-tool").unwrap();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("noido-no-such-tool"), "{notes:?}");
        let path = places.applications.join(FILE_NAME);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            desktop_entry(&places.exe).unwrap()
        );

        let Plan::Steps(uninstall) = plan_uninstall(&places) else {
            panic!("expected steps");
        };
        carry_out(&uninstall, "noido-no-such-tool").unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn a_failing_refresh_is_reported() {
        let dir = temp_dir("refresh-fails");
        std::fs::create_dir_all(&dir).unwrap();
        let notes = carry_out(&[Step::Refresh(dir)], "false").unwrap();
        assert_eq!(notes.len(), 1, "{notes:?}");
    }
}
