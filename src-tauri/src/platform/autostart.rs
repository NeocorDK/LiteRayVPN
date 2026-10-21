//! Start with Windows. TUN mode needs administrator rights, so it uses a Task Scheduler
//! task with highest privileges (no UAC prompt at logon); proxy mode uses the HKCU Run key.

use std::os::windows::process::CommandExt;
use std::process::Command;

use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
use winreg::RegKey;

use crate::xray::runner::CREATE_NO_WINDOW;

const NAME: &str = "LiteRay";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

fn command_line() -> anyhow::Result<String> {
    let exe = std::env::current_exe()?;
    Ok(format!("\"{}\" --autostart", exe.display()))
}

fn schtasks(args: &[&str]) -> anyhow::Result<()> {
    let out = Command::new("schtasks").args(args).creation_flags(CREATE_NO_WINDOW).output()?;
    if !out.status.success() {
        anyhow::bail!("schtasks: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

fn set_run_key(enabled: bool) -> anyhow::Result<()> {
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)?;
    if enabled {
        key.set_value(NAME, &command_line()?)?;
    } else {
        let _ = key.delete_value(NAME);
    }
    Ok(())
}

fn task_exists() -> bool {
    Command::new("schtasks")
        .args(["/Query", "/TN", NAME])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `elevated` selects the scheduled task (requires running as administrator).
pub fn set(enabled: bool, elevated: bool) -> anyhow::Result<()> {
    if enabled && elevated {
        let cmd = command_line()?;
        schtasks(&["/Create", "/F", "/TN", NAME, "/TR", &cmd, "/SC", "ONLOGON", "/RL", "HIGHEST", "/DELAY", "0000:10"])?;
        set_run_key(false)?;
    } else {
        if task_exists() {
            schtasks(&["/Delete", "/F", "/TN", NAME])?;
        }
        set_run_key(enabled)?;
    }
    Ok(())
}
