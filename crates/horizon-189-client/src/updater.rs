//! Self-update for the installed app. The app keeps its own clone of the
//! repository under Application Support; at launch it fetches the branch,
//! and when the tip differs from the commit this binary was built from it
//! rebuilds with the user's Rust toolchain and swaps the new program into
//! the bundle in place (a rename over the old file, so no copies are left).
//! The running game keeps its old program until it restarts.
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

/// The commit this binary was built from, set by the updater's build.
pub const BUILD: Option<&str> = option_env!("HORIZON_189_BUILD");
/// The program's name inside `Contents/MacOS`.
pub const EXECUTABLE: &str = "horizon-189";
pub const APP_NAME: &str = "Bob Client.app";
/// Names the app was installed under before; an update renames these in
/// place rather than leaving a second copy.
const OLD_APP_NAMES: [&str; 1] = ["Horizon 1.8.9.app"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Not running from an installed app (development and validation runs).
    Off,
    Checking,
    UpToDate,
    /// Compiling: crates finished so far.
    Building(u32),
    /// Installed; takes effect on restart.
    Ready(String),
    Failed(String),
}

impl Status {
    /// One short line for the launcher.
    pub fn label(&self) -> String {
        match self {
            Status::Off => "Development build".to_owned(),
            Status::Checking => "Checking for updates…".to_owned(),
            Status::UpToDate => "Up to date".to_owned(),
            Status::Building(0) => "Downloading update…".to_owned(),
            Status::Building(done) => format!("Building update… {done}"),
            Status::Ready(commit) => format!("Update {commit} installed · used from the next Play"),
            Status::Failed(reason) => format!("Update failed: {reason}"),
        }
    }
}

/// Where the app keeps its settings, update source, clone and build.
pub fn support_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(Path::new(&home).join("Library/Application Support/Horizon 1.8.9"))
}

/// The `.app` holding `executable`, if it runs from one.
pub fn bundle_of(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app")
        .then(|| bundle.to_path_buf())
}

/// The update source, written by `--install`: a git URL and a branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub repository: String,
    pub branch: String,
}

impl Source {
    pub fn parse(text: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let repository = value.get("repository")?.as_str()?.trim().to_owned();
        let branch = value
            .get("branch")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("main")
            .trim()
            .to_owned();
        // Plain https remotes only: the URL reaches `git` as an argument.
        let safe = |s: &str| {
            !s.is_empty()
                && !s.starts_with('-')
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-._/:@~".contains(c))
        };
        (repository.starts_with("https://") && safe(&repository) && safe(&branch))
            .then_some(Self { repository, branch })
    }

    pub fn to_json(&self) -> String {
        serde_json::json!({"repository": self.repository, "branch": self.branch}).to_string()
    }
}

/// Whether the branch tip needs building: always for an untagged build.
pub fn outdated(built: Option<&str>, tip: &str) -> bool {
    built != Some(tip)
}

/// Counts finished crates from cargo's progress lines.
pub fn compiled(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("Compiling ") || line.starts_with("Fresh ")
}

pub struct Updater {
    status: Arc<Mutex<Status>>,
    bundle: Option<PathBuf>,
}

impl Updater {
    /// Start checking in the background when running from an installed app.
    pub fn start() -> Self {
        let bundle = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.canonicalize().ok())
            .and_then(|exe| bundle_of(&exe));
        let status = Arc::new(Mutex::new(if bundle.is_some() {
            Status::Checking
        } else {
            Status::Off
        }));
        if let (Some(bundle), Some(support)) = (bundle.clone(), support_dir()) {
            let shared = status.clone();
            let spawned = std::thread::Builder::new()
                .name("Updater".into())
                .spawn(move || {
                    let result = update(&support, &bundle, &shared);
                    let next = match result {
                        Ok(status) => status,
                        Err(reason) => {
                            log(&support, &format!("failed: {reason}"));
                            Status::Failed(reason)
                        }
                    };
                    if let Ok(mut status) = shared.lock() {
                        *status = next;
                    }
                });
            if spawned.is_err() {
                *status.lock().expect("fresh lock") = Status::Failed("no thread".into());
            }
        }
        Self { status, bundle }
    }

    pub fn status(&self) -> Status {
        self.status
            .lock()
            .map_or(Status::Failed("state lost".into()), |s| s.clone())
    }

    /// Reopen the app on its new program; the caller then exits.
    pub fn restart(&self) -> bool {
        let Some(bundle) = &self.bundle else {
            return false;
        };
        let bundle = if bundle.exists() {
            bundle.clone()
        } else {
            bundle.with_file_name(APP_NAME)
        };
        Command::new("/usr/bin/open")
            .arg("-n")
            .arg(bundle)
            .spawn()
            .is_ok()
    }
}

fn log(support: &Path, line: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(support.join("update.log"))
    {
        let _ = writeln!(file, "{line}");
    }
}

fn git(support: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("/usr/bin/git")
        .args(args)
        .current_dir(support)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|_| "git is missing (install the Xcode command line tools)".to_owned())?;
    log(
        support,
        &format!("git {}: {}", args.join(" "), output.status),
    );
    log(support, &String::from_utf8_lossy(&output.stderr));
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(format!(
            "git {} failed",
            args.first().copied().unwrap_or("")
        ))
    }
}

fn cargo() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let rustup = Path::new(&home).join(".cargo/bin/cargo");
    [rustup, PathBuf::from("/opt/homebrew/bin/cargo")]
        .into_iter()
        .find(|path| path.is_file())
}

/// rustup's tools, Homebrew, then the system's.
fn build_path() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin")
}

fn update(support: &Path, bundle: &Path, status: &Mutex<Status>) -> Result<Status, String> {
    let set = |next: Status| {
        if let Ok(mut status) = status.lock() {
            *status = next;
        }
    };
    let source = std::fs::read_to_string(support.join("update.json"))
        .ok()
        .and_then(|text| Source::parse(&text))
        .ok_or("no update source (reinstall the app)")?;
    log(support, &format!("checking {}", source.branch));
    let clone = support.join("source");
    if !clone.join(".git").is_dir() {
        set(Status::Building(0));
        let target = clone.to_string_lossy().into_owned();
        git(
            support,
            &[
                "clone",
                "--depth",
                "1",
                "--branch",
                &source.branch,
                &source.repository,
                &target,
            ],
        )?;
    }
    let clone_dir = clone.to_string_lossy().into_owned();
    git(
        support,
        &[
            "-C",
            &clone_dir,
            "fetch",
            "--depth",
            "1",
            "origin",
            &source.branch,
        ],
    )?;
    let tip = git(support, &["-C", &clone_dir, "rev-parse", "FETCH_HEAD"])?;
    let short: String = tip.chars().take(10).collect();
    if !outdated(BUILD, &tip) {
        log(support, &format!("up to date at {short}"));
        return Ok(Status::UpToDate);
    }
    // Built and installed by an earlier run: only a restart is missing.
    let installed = std::fs::read_to_string(support.join("installed")).unwrap_or_default();
    if installed.trim() == tip {
        return Ok(Status::Ready(short));
    }
    git(
        support,
        &[
            "-C",
            &clone_dir,
            "checkout",
            "--force",
            "--detach",
            "FETCH_HEAD",
        ],
    )?;
    let cargo = cargo().ok_or("Rust is not installed (rustup.rs)")?;
    set(Status::Building(0));
    let target_dir = support.join("build");
    let mut child = Command::new(cargo)
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            "horizon-189-client",
            "--target",
            "aarch64-apple-darwin",
            "--target-dir",
        ])
        .arg(&target_dir)
        .current_dir(&clone)
        .env("HORIZON_189_BUILD", &tip)
        // Apps opened from Finder get almost no PATH; cargo needs rustc and
        // the system linker.
        .env("PATH", build_path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "could not start cargo".to_owned())?;
    let mut done = 0;
    let mut tail = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if compiled(&line) {
                done += 1;
                set(Status::Building(done));
            }
            tail.push(line);
            if tail.len() > 40 {
                tail.remove(0);
            }
        }
    }
    let built = child.wait().map_err(|_| "cargo stopped".to_owned())?;
    log(support, &tail.join("\n"));
    if !built.success() {
        return Err("the build failed (see update.log)".into());
    }
    let program = target_dir.join("aarch64-apple-darwin/release/horizon-189-client");
    install_program(&program, bundle).map_err(|error| format!("install: {error}"))?;
    adopt_name(bundle).map_err(|error| format!("rename: {error}"))?;
    std::fs::write(support.join("installed"), &tip).map_err(|error| error.to_string())?;
    log(support, &format!("installed {short}"));
    Ok(Status::Ready(short))
}

/// Copy `program` into the bundle beside the old one, then rename it over
/// the old one: the swap is atomic and leaves no second copy behind.
pub fn install_program(program: &Path, bundle: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let macos = bundle.join("Contents/MacOS");
    std::fs::create_dir_all(&macos)?;
    let staged = macos.join(format!(".{EXECUTABLE}.new"));
    std::fs::copy(program, &staged)?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&staged, macos.join(EXECUTABLE))
}

/// Give an installed bundle the current name and `Info.plist`: one under
/// an older name is renamed in place (never copied), unless an app with the
/// new name is already there. Returns where the bundle is now.
pub fn adopt_name(bundle: &Path) -> std::io::Result<PathBuf> {
    let contents = bundle.join("Contents");
    std::fs::create_dir_all(&contents)?;
    std::fs::write(
        contents.join("Info.plist"),
        info_plist(env!("CARGO_PKG_VERSION")),
    )?;
    let Some(folder) = bundle.parent() else {
        return Ok(bundle.to_path_buf());
    };
    let named = folder.join(APP_NAME);
    if bundle == named || named.exists() {
        return Ok(bundle.to_path_buf());
    }
    std::fs::rename(bundle, &named)?;
    Ok(named)
}

/// `program`, or the same program in a bundle renamed by [`adopt_name`]
/// since it started (a running program keeps its launch path).
pub fn locate(program: &Path) -> PathBuf {
    if program.exists() {
        return program.to_path_buf();
    }
    bundle_of(program)
        .and_then(|bundle| Some(bundle.parent()?.join(APP_NAME)))
        .map(|bundle| bundle.join("Contents/MacOS").join(EXECUTABLE))
        .filter(|renamed| renamed.exists())
        .unwrap_or_else(|| program.to_path_buf())
}

/// This program on disk, following a rename.
pub fn program() -> std::io::Result<PathBuf> {
    Ok(locate(&std::env::current_exe()?))
}

/// `Info.plist` for the bundle.
pub fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleDisplayName</key>
	<string>Bob Client</string>
	<key>CFBundleExecutable</key>
	<string>{EXECUTABLE}</string>
	<key>CFBundleIconFile</key>
	<string>horizon</string>
	<key>CFBundleIdentifier</key>
	<string>dev.horizon.client189</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>Bob Client</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>{version}</string>
	<key>CFBundleVersion</key>
	<string>{version}</string>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.games</string>
	<key>LSArchitecturePriority</key>
	<array><string>arm64</string></array>
	<key>LSMinimumSystemVersion</key>
	<string>13.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSHumanReadableCopyright</key>
	<string>Not an official Minecraft product; not approved by or associated with Mojang or Microsoft.</string>
</dict>
</plist>
"#
    )
}

/// `--install`: create or refresh the app bundle from this program, in
/// /Applications (or ~/Applications when that is not writable), and record
/// the update source. Returns the bundle.
pub fn install(source: Option<&Source>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let program = std::env::current_exe()?;
    let home = std::env::var_os("HOME").ok_or("no home directory")?;
    let system = Path::new("/Applications");
    let folder = if std::fs::metadata(system).is_ok_and(|m| !m.permissions().readonly())
        && tempfile_probe(system)
    {
        system.to_path_buf()
    } else {
        let user = Path::new(&home).join("Applications");
        std::fs::create_dir_all(&user)?;
        user
    };
    let bundle = folder.join(APP_NAME);
    for old in OLD_APP_NAMES {
        let old = folder.join(old);
        if old.exists() && !bundle.exists() {
            std::fs::rename(&old, &bundle)?;
        }
    }
    let resources = bundle.join("Contents/Resources");
    std::fs::create_dir_all(&resources)?;
    let version = env!("CARGO_PKG_VERSION");
    std::fs::write(bundle.join("Contents/Info.plist"), info_plist(version))?;
    std::fs::write(
        resources.join("horizon.icns"),
        include_bytes!("../../../packaging/icons/horizon.icns"),
    )?;
    install_program(&program, &bundle)?;
    if let Some(source) = source {
        let support = support_dir().ok_or("no home directory")?;
        std::fs::create_dir_all(&support)?;
        std::fs::write(support.join("update.json"), source.to_json())?;
        if let Some(build) = BUILD {
            std::fs::write(support.join("installed"), build)?;
        }
    }
    Ok(bundle)
}

fn tempfile_probe(folder: &Path) -> bool {
    let probe = folder.join(format!(".horizon-189-probe-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_programs_inside_an_app_bundle_update() {
        assert_eq!(
            bundle_of(Path::new(
                "/Applications/Horizon 1.8.9.app/Contents/MacOS/horizon-189"
            )),
            Some(PathBuf::from("/Applications/Horizon 1.8.9.app"))
        );
        assert_eq!(
            bundle_of(Path::new("/tmp/target/release/horizon-189-client")),
            None
        );
        assert_eq!(bundle_of(Path::new("/x.app/MacOS/a")), None);
    }

    #[test]
    fn update_sources_must_be_plain_https_remotes() {
        let source =
            Source::parse(r#"{"repository":"https://example.com/a/b.git"}"#).expect("parsed");
        assert_eq!(source.branch, "main");
        assert_eq!(Source::parse(&source.to_json()), Some(source));
        for bad in [
            r#"{"repository":"ssh://host/x"}"#,
            r#"{"repository":"https://host/x; rm -rf ~"}"#,
            r#"{"repository":"https://host/x","branch":"--upload-pack=x"}"#,
            "not json",
        ] {
            assert_eq!(Source::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn builds_rebuild_when_the_tip_moves_and_cargo_progress_counts() {
        assert!(outdated(None, "abc"));
        assert!(outdated(Some("abc"), "def"));
        assert!(!outdated(Some("abc"), "abc"));
        assert!(compiled("   Compiling horizon-189-client v0.1.0 (/x)"));
        assert!(!compiled("    Finished `release` profile"));
        assert_eq!(
            Status::Ready("abc".into()).label(),
            "Update abc installed · used from the next Play"
        );
    }

    #[test]
    fn an_app_under_an_old_name_is_renamed_in_place() {
        let dir = std::env::temp_dir().join(format!("h189-rename-{}", std::process::id()));
        let old = dir.join("Horizon 1.8.9.app");
        std::fs::create_dir_all(old.join("Contents/MacOS")).unwrap();
        std::fs::write(old.join("Contents/MacOS").join(EXECUTABLE), b"x").unwrap();
        let program = old.join("Contents/MacOS").join(EXECUTABLE);
        let named = adopt_name(&old).unwrap();
        assert_eq!(named, dir.join(APP_NAME));
        assert!(!old.exists() && named.join("Contents/MacOS").join(EXECUTABLE).is_file());
        let plist = std::fs::read_to_string(named.join("Contents/Info.plist")).unwrap();
        assert!(plist.contains("<string>Bob Client</string>"));
        // The old program path still finds the program after the rename.
        assert_eq!(
            locate(&program),
            named.join("Contents/MacOS").join(EXECUTABLE)
        );
        // Already named: stays put. Never clobbers another app of that name.
        assert_eq!(adopt_name(&named).unwrap(), named);
        let other = dir.join("Horizon 1.8.9.app");
        std::fs::create_dir_all(other.join("Contents")).unwrap();
        assert_eq!(adopt_name(&other).unwrap(), other);
        assert!(other.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_program_swap_leaves_one_file() {
        let dir = std::env::temp_dir().join(format!("h189-bundle-{}", std::process::id()));
        let bundle = dir.join("Test.app");
        let program = dir.join("program");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&program, b"one").unwrap();
        install_program(&program, &bundle).unwrap();
        std::fs::write(&program, b"two").unwrap();
        install_program(&program, &bundle).unwrap();
        let macos = bundle.join("Contents/MacOS");
        let names: Vec<_> = std::fs::read_dir(&macos)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [std::ffi::OsString::from(EXECUTABLE)]);
        assert_eq!(std::fs::read(macos.join(EXECUTABLE)).unwrap(), b"two");
        assert!(info_plist("1.0").contains("<string>horizon-189</string>"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
