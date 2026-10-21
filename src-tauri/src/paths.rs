//! Where data and the core live. A file named `portable` next to the exe switches to
//! portable mode: everything stays in the exe folder.
//!
//! Installed: data in %APPDATA%\LiteRay. The core is used straight from the bundled
//! `resources\xray` when that folder is writable (the per-user installer puts it under
//! %LOCALAPPDATA%), so it is not duplicated; otherwise (e.g. Program Files) it is copied to
//! %LOCALAPPDATA%\LiteRay-core, because the updater must be able to replace it.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub portable: bool,
    pub data_dir: PathBuf,
    pub core_dir: PathBuf,
}

const APP_DIR: &str = "LiteRay";
const CORE_FALLBACK_DIR: &str = "LiteRay-core";
const CORE_FILES: &[&str] = &["xray.exe", "wintun.dll", "geoip.dat", "geosite.dat", "VERSION"];

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".write-test");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Where the bundle places resources: Tauri on Windows resolves them next to the exe.
pub fn bundled_core_dir(exe_dir: &Path) -> PathBuf {
    exe_dir.join("resources").join("xray")
}

impl Paths {
    pub fn detect() -> anyhow::Result<Self> {
        let exe = std::env::current_exe()?;
        let exe_dir = exe.parent().ok_or_else(|| anyhow::anyhow!("exe has no parent dir"))?.to_path_buf();
        let portable = exe_dir.join("portable").exists();
        let (data_dir, core_dir) = if portable {
            (exe_dir.join("data"), exe_dir.join("xray"))
        } else {
            let roaming = std::env::var_os("APPDATA").map(PathBuf::from).ok_or_else(|| anyhow::anyhow!("APPDATA not set"))?;
            let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| roaming.clone());
            let bundled = bundled_core_dir(&exe_dir);
            let core = if bundled.join("xray.exe").exists() && is_writable(&bundled) {
                bundled
            } else {
                local.join(CORE_FALLBACK_DIR)
            };
            (roaming.join(APP_DIR), core)
        };
        std::fs::create_dir_all(&data_dir)?;
        std::fs::create_dir_all(&core_dir)?;
        Ok(Self { portable, data_dir, core_dir })
    }

    pub fn state_file(&self) -> PathBuf {
        self.data_dir.join("state.json")
    }

    /// The user's own proxy settings, saved while ours are active.
    pub fn sysproxy_backup(&self) -> PathBuf {
        self.data_dir.join("sysproxy-backup.json")
    }

    pub fn run_config(&self) -> PathBuf {
        self.data_dir.join("xray-config.json")
    }

    /// The core is updated in place, so it lives in a writable folder. On first run (or
    /// when a file is missing) it is seeded from the bundled resources.
    pub fn seed_core(&self, bundled: &Path) -> anyhow::Result<()> {
        if !bundled.exists() {
            return Ok(());
        }
        // Copying a folder onto itself would truncate the files.
        if std::fs::canonicalize(bundled).ok() == std::fs::canonicalize(&self.core_dir).ok() {
            return Ok(());
        }
        // A client update may ship a newer core than the one already installed.
        let read_version = |dir: &Path| std::fs::read_to_string(dir.join("VERSION")).ok().map(|v| v.trim().to_string());
        let bundled_newer = match read_version(bundled) {
            Some(v) => crate::xray::updater::is_newer(read_version(&self.core_dir).as_deref(), &v),
            None => false,
        };
        for name in CORE_FILES {
            let target = self.core_dir.join(name);
            let source = bundled.join(name);
            let core_file = matches!(*name, "xray.exe" | "VERSION");
            if source.exists() && (!target.exists() || (bundled_newer && core_file)) {
                std::fs::copy(&source, &target)?;
            }
        }
        Ok(())
    }

    pub fn self_exe_for_rules() -> Option<String> {
        std::env::current_exe().ok().map(|p| crate::routing::normalize(crate::routing::RuleKind::App, &p.to_string_lossy()).unwrap_or_default())
    }
}
