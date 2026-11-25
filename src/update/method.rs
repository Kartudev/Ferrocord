//! How this copy of Ferrocord was installed, which decides what "Update"
//! does. Detection is a pure function of [`Facts`] (gathered from
//! `current_exe()`, the environment and a few file checks) so every case
//! is unit-tested.

use std::path::{Path, PathBuf};

use super::CRATE;
use super::release::Platform;

/// The paths `install.sh` uses (`${XDG_BIN_HOME:-$HOME/.local/bin}`,
/// `${XDG_DATA_HOME:-$HOME/.local/share}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptLayout {
    pub bin_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl ScriptLayout {
    /// The layout `install.sh` would use with this environment (empty
    /// variables count as unset, like the script's `${VAR:-default}`).
    pub fn from_env(
        home: Option<&Path>,
        xdg_bin_home: Option<&Path>,
        xdg_data_home: Option<&Path>,
    ) -> Option<Self> {
        let nonempty =
            |p: Option<&Path>| p.filter(|p| !p.as_os_str().is_empty()).map(PathBuf::from);
        let home = nonempty(home);
        let bin_dir =
            nonempty(xdg_bin_home).or_else(|| home.as_ref().map(|h| h.join(".local/bin")))?;
        let data_dir =
            nonempty(xdg_data_home).or_else(|| home.as_ref().map(|h| h.join(".local/share")))?;
        Some(Self { bin_dir, data_dir })
    }

    /// Reads `HOME`, `XDG_BIN_HOME` and `XDG_DATA_HOME`.
    pub fn from_process_env() -> Option<Self> {
        let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
        Self::from_env(
            var("HOME").as_deref(),
            var("XDG_BIN_HOME").as_deref(),
            var("XDG_DATA_HOME").as_deref(),
        )
    }

    pub fn bin_path(&self, name: &str) -> PathBuf {
        self.bin_dir.join(name)
    }

    pub fn apps_dir(&self) -> PathBuf {
        self.data_dir.join("applications")
    }

    pub fn icon_dir(&self) -> PathBuf {
        self.data_dir.join("icons/hicolor/scalable/apps")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.data_dir.join(CRATE)
    }

    pub fn version_file(&self) -> PathBuf {
        self.state_dir().join("installed-version")
    }
}

/// Why the app cannot update itself (it then only links to the release
/// page).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualReason {
    /// `/usr/bin`, a distribution package, or any other location that is
    /// not ours.
    SystemPath,
    /// Flatpak, Snap or AppImage: the sandbox or image updates the app.
    Sandbox(&'static str),
    /// Running from a cargo `target/` directory.
    BuildDir,
    /// Our layout, but the folder is not writable (e.g. Program Files).
    ReadOnly,
    /// No release build exists for this OS / CPU.
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallMethod {
    /// Installed by `install.sh` (Linux): self-update replaces the files
    /// the script installed.
    Script(ScriptLayout),
    /// The extracted Windows zip in a user-writable folder.
    WindowsZip {
        dir: PathBuf,
    },
    /// `cargo install`: never self-updated; the user re-runs cargo.
    Cargo,
    Manual(ManualReason),
}

impl InstallMethod {
    pub fn can_self_update(&self) -> bool {
        matches!(self, Self::Script(_) | Self::WindowsZip { .. })
    }

    /// Settings → Updates, "Installed with".
    pub fn label(&self) -> String {
        match self {
            Self::Script(l) => format!("Install script ({})", l.bin_dir.display()),
            Self::WindowsZip { dir } => format!("Zip folder ({})", dir.display()),
            Self::Cargo => "cargo install".to_owned(),
            Self::Manual(r) => match r {
                ManualReason::SystemPath => "System location or package manager".to_owned(),
                ManualReason::Sandbox(s) => (*s).to_owned(),
                ManualReason::BuildDir => "Development build (cargo target directory)".to_owned(),
                ManualReason::ReadOnly => "Read-only folder".to_owned(),
                ManualReason::Unsupported => "A platform without release builds".to_owned(),
            },
        }
    }

    /// One sentence on what "Update" does with this method.
    pub fn explanation(&self) -> &'static str {
        match self {
            Self::Script(_) | Self::WindowsZip { .. } => {
                "Ferrocord downloads the release, checks its SHA-256 checksum and replaces itself."
            }
            Self::Cargo => "Installed with cargo: update it with the cargo command below.",
            Self::Manual(ManualReason::Sandbox(_)) => {
                "This package updates through its own store or image; the release page has the details."
            }
            Self::Manual(ManualReason::ReadOnly) => {
                "Ferrocord cannot write to its folder: download the new version from the release page."
            }
            Self::Manual(_) => {
                "Ferrocord was not installed by its install script: download the new version from the release page."
            }
        }
    }
}

/// The command shown for `cargo install` copies.
pub fn cargo_command() -> String {
    format!(
        "cargo install --git https://github.com/{} --locked --force",
        super::repo()
    )
}

/// Everything detection looks at.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// `current_exe()`, canonicalized.
    pub exe: PathBuf,
    pub platform: Option<Platform>,
    /// `install.sh`'s layout and its binary path (canonicalized when it
    /// exists).
    pub script: Option<(ScriptLayout, PathBuf)>,
    /// `installed-version` exists.
    pub version_marker: bool,
    /// `${CARGO_HOME:-~/.cargo}/bin/<crate>`, canonicalized when it exists.
    pub cargo_bin: Option<PathBuf>,
    /// Flatpak / Snap / AppImage environment.
    pub sandbox: Option<&'static str>,
    /// A file can be created next to the executable.
    pub dir_writable: bool,
}

/// Decides the install method (see `docs/architecture.md`, "Updates").
pub fn detect(f: &Facts) -> InstallMethod {
    if let Some(s) = f.sandbox {
        return InstallMethod::Manual(ManualReason::Sandbox(s));
    }
    if in_build_dir(&f.exe) {
        return InstallMethod::Manual(ManualReason::BuildDir);
    }
    let in_cargo_bin = f.cargo_bin.as_deref() == Some(f.exe.as_path())
        || f.exe
            .parent()
            .is_some_and(|bin| bin.ends_with(".cargo/bin") || bin.ends_with(".cargo\\bin"));
    if in_cargo_bin {
        return InstallMethod::Cargo;
    }
    match f.platform.unwrap_or(Platform::Other) {
        Platform::LinuxX64 => match &f.script {
            Some((layout, bin)) if *bin == f.exe && f.version_marker => {
                if f.dir_writable {
                    InstallMethod::Script(layout.clone())
                } else {
                    InstallMethod::Manual(ManualReason::ReadOnly)
                }
            }
            _ => InstallMethod::Manual(ManualReason::SystemPath),
        },
        Platform::WindowsX64 => match f.exe.parent() {
            Some(dir) if f.dir_writable => InstallMethod::WindowsZip {
                dir: dir.to_path_buf(),
            },
            _ => InstallMethod::Manual(ManualReason::ReadOnly),
        },
        Platform::Other => InstallMethod::Manual(ManualReason::Unsupported),
    }
}

/// `…/target/{debug,release}/<exe>` or `…/target/<triple>/{debug,release}/<exe>`.
fn in_build_dir(exe: &Path) -> bool {
    let names: Vec<String> = exe
        .ancestors()
        .skip(1)
        .take(3)
        .map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default()
        })
        .collect();
    let profile = |n: &str| matches!(n, "debug" | "release" | "deps");
    match names.as_slice() {
        [p, t, ..] if profile(p) && t == "target" => true,
        [d, p, t] if d == "deps" && profile(p) && t == "target" => true,
        [p, _triple, t] if profile(p) && t == "target" => true,
        _ => false,
    }
}

fn sandbox_from_env(var: impl Fn(&str) -> bool) -> Option<&'static str> {
    if var("FLATPAK_ID") || Path::new("/.flatpak-info").exists() {
        Some("Flatpak")
    } else if var("SNAP") {
        Some("Snap")
    } else if var("APPIMAGE") {
        Some("AppImage")
    } else {
        None
    }
}

/// Whether a file can be created in `dir` (a real probe: permissions,
/// ACLs and read-only mounts all count).
pub fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".{CRATE}-write-test-{}", std::process::id()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(f) => {
            drop(f);
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// The running executable, canonicalized. On Linux a binary replaced by
/// an update reads as `… (deleted)`, so callers capture this before
/// installing.
pub fn current_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(std::fs::canonicalize(&exe).unwrap_or(exe))
}

/// Gathers the facts for this process (blocking: canonicalize and probe
/// files; run it off the UI thread).
pub fn gather(exe: &Path) -> Facts {
    let canon = |p: PathBuf| std::fs::canonicalize(&p).unwrap_or(p);
    let exe_name = if cfg!(windows) {
        format!("{CRATE}.exe")
    } else {
        CRATE.to_owned()
    };
    let script = ScriptLayout::from_process_env().map(|l| {
        let bin = canon(l.bin_path(CRATE));
        (l, bin)
    });
    let version_marker = script
        .as_ref()
        .is_some_and(|(l, _)| l.version_file().is_file());
    let cargo_home = std::env::var_os("CARGO_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|b| b.home_dir().join(".cargo")));
    let cargo_bin = cargo_home.map(|h| canon(h.join("bin").join(&exe_name)));
    let sandbox = sandbox_from_env(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()));
    let dir_writable = exe.parent().is_some_and(dir_writable);
    Facts {
        exe: exe.to_path_buf(),
        platform: Some(Platform::current()),
        script,
        version_marker,
        cargo_bin,
        sandbox,
        dir_writable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> ScriptLayout {
        ScriptLayout {
            bin_dir: "/home/u/.local/bin".into(),
            data_dir: "/home/u/.local/share".into(),
        }
    }

    fn linux(exe: &str) -> Facts {
        Facts {
            exe: exe.into(),
            platform: Some(Platform::LinuxX64),
            script: Some((layout(), "/home/u/.local/bin/ferrocord".into())),
            version_marker: true,
            cargo_bin: Some("/home/u/.cargo/bin/ferrocord".into()),
            sandbox: None,
            dir_writable: true,
        }
    }

    #[test]
    fn layout_follows_install_sh() {
        let l = ScriptLayout::from_env(Some(Path::new("/home/u")), None, None).unwrap();
        assert_eq!(l, layout());
        assert_eq!(
            l.version_file(),
            Path::new("/home/u/.local/share/ferrocord/installed-version")
        );
        assert_eq!(
            l.icon_dir(),
            Path::new("/home/u/.local/share/icons/hicolor/scalable/apps")
        );
        assert_eq!(l.apps_dir(), Path::new("/home/u/.local/share/applications"));
        // XDG overrides; empty values count as unset (`${VAR:-default}`).
        let l = ScriptLayout::from_env(
            Some(Path::new("/home/u")),
            Some(Path::new("/opt/b")),
            Some(Path::new("")),
        )
        .unwrap();
        assert_eq!(l.bin_dir, Path::new("/opt/b"));
        assert_eq!(l.data_dir, Path::new("/home/u/.local/share"));
        assert!(ScriptLayout::from_env(None, None, None).is_none());
        assert!(ScriptLayout::from_env(Some(Path::new("")), None, None).is_none());
    }

    #[test]
    fn install_sh_copies_update_themselves() {
        assert_eq!(
            detect(&linux("/home/u/.local/bin/ferrocord")),
            InstallMethod::Script(layout())
        );
    }

    #[test]
    fn the_script_layout_needs_the_version_marker_and_a_writable_dir() {
        let mut f = linux("/home/u/.local/bin/ferrocord");
        f.version_marker = false;
        assert_eq!(detect(&f), InstallMethod::Manual(ManualReason::SystemPath));
        let mut f = linux("/home/u/.local/bin/ferrocord");
        f.dir_writable = false;
        assert_eq!(detect(&f), InstallMethod::Manual(ManualReason::ReadOnly));
    }

    #[test]
    fn cargo_installs_are_never_self_updated() {
        let m = detect(&linux("/home/u/.cargo/bin/ferrocord"));
        assert_eq!(m, InstallMethod::Cargo);
        assert!(!m.can_self_update());
        // A custom CARGO_HOME.
        let mut f = linux("/data/cargo/bin/ferrocord");
        f.cargo_bin = Some("/data/cargo/bin/ferrocord".into());
        assert_eq!(detect(&f), InstallMethod::Cargo);
        assert!(
            cargo_command()
                .contains("--git https://github.com/FerroApps/Ferrocord --locked --force")
        );
    }

    #[test]
    fn system_paths_and_unknown_locations_only_notify() {
        for exe in [
            "/usr/bin/ferrocord",
            "/opt/ferrocord/ferrocord",
            "/home/u/bin/ferrocord",
        ] {
            assert_eq!(
                detect(&linux(exe)),
                InstallMethod::Manual(ManualReason::SystemPath),
                "{exe}"
            );
        }
    }

    #[test]
    fn sandboxes_and_build_dirs_only_notify() {
        let mut f = linux("/home/u/.local/bin/ferrocord");
        f.sandbox = Some("Flatpak");
        assert_eq!(
            detect(&f),
            InstallMethod::Manual(ManualReason::Sandbox("Flatpak"))
        );
        for exe in [
            "/src/Ferrocord/target/debug/ferrocord",
            "/src/Ferrocord/target/release/ferrocord",
            "/src/Ferrocord/target/x86_64-unknown-linux-gnu/release/ferrocord",
            "/src/Ferrocord/target/debug/deps/updater-1234",
        ] {
            assert_eq!(
                detect(&linux(exe)),
                InstallMethod::Manual(ManualReason::BuildDir),
                "{exe}"
            );
        }
        // A folder that merely contains "release" is not a build dir.
        assert_eq!(
            detect(&linux("/home/u/release/ferrocord")),
            InstallMethod::Manual(ManualReason::SystemPath)
        );
    }

    #[test]
    fn sandbox_env_detection() {
        assert_eq!(sandbox_from_env(|k| k == "SNAP"), Some("Snap"));
        assert_eq!(sandbox_from_env(|k| k == "APPIMAGE"), Some("AppImage"));
        assert_eq!(sandbox_from_env(|k| k == "FLATPAK_ID"), Some("Flatpak"));
    }

    #[test]
    fn windows_zip_folders_update_unless_read_only() {
        let f = Facts {
            exe: r"C:\Users\u\Apps\Ferrocord\ferrocord.exe".into(),
            platform: Some(Platform::WindowsX64),
            dir_writable: true,
            ..Facts::default()
        };
        assert!(matches!(detect(&f), InstallMethod::WindowsZip { .. }));
        let ro = Facts {
            dir_writable: false,
            ..f
        };
        assert_eq!(detect(&ro), InstallMethod::Manual(ManualReason::ReadOnly));
    }

    #[test]
    fn other_platforms_only_notify() {
        let f = Facts {
            exe: "/Applications/ferrocord".into(),
            platform: Some(Platform::Other),
            dir_writable: true,
            ..Facts::default()
        };
        assert_eq!(detect(&f), InstallMethod::Manual(ManualReason::Unsupported));
    }

    #[test]
    fn write_probe() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(dir_writable(tmp.path()));
        assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
        assert!(!dir_writable(&tmp.path().join("missing")));
    }
}
