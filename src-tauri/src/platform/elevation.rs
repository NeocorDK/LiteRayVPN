//! Administrator rights: detection and relaunch through UAC.

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, WaitForSingleObject, PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        let _ = CloseHandle(token);
        ok.is_ok() && elevation.TokenIsElevated != 0
    }
}

/// Starts a new elevated copy of the client. The new copy waits for this process to exit
/// (`--wait-pid`) so the single-instance guard does not reject it.
pub fn relaunch_elevated(extra_args: &[&str]) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let mut args = vec![format!("--wait-pid={}", std::process::id())];
    args.extend(extra_args.iter().map(|a| a.to_string()));
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("runas"),
            &HSTRING::from(exe.as_os_str()),
            &HSTRING::from(args.join(" ")),
            None,
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute returns a value > 32 on success; 5 (access denied) when UAC was declined.
    if result.0 as isize <= 32 {
        anyhow::bail!("elevation was cancelled");
    }
    Ok(())
}

/// Blocks until the process from `--wait-pid=<pid>` exits (at most `timeout_ms`).
pub fn wait_for_parent_exit(args: &[String], timeout_ms: u32) {
    let Some(pid) = args.iter().find_map(|a| a.strip_prefix("--wait-pid=")).and_then(|p| p.parse::<u32>().ok()) else {
        return;
    };
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            let _ = WaitForSingleObject(handle, timeout_ms) == WAIT_OBJECT_0;
            let _ = CloseHandle(handle);
        }
    }
}
