//! Windows system proxy (WinINet per-connection options, as used by browsers and most apps).
//! The user's previous settings are saved before enabling and restored afterwards, also
//! after a crash (the backup file survives until it is restored).

use std::path::Path;

use serde::{Deserialize, Serialize};
use windows::core::PWSTR;
use windows::Win32::Foundation::{GlobalFree, HGLOBAL};
use windows::Win32::Networking::WinInet::{
    InternetQueryOptionW, InternetSetOptionW, INTERNET_OPTION_PER_CONNECTION_OPTION, INTERNET_OPTION_REFRESH,
    INTERNET_OPTION_SETTINGS_CHANGED, INTERNET_PER_CONN, INTERNET_PER_CONN_AUTOCONFIG_URL, INTERNET_PER_CONN_FLAGS,
    INTERNET_PER_CONN_OPTIONW, INTERNET_PER_CONN_OPTION_LISTW, INTERNET_PER_CONN_PROXY_BYPASS,
    INTERNET_PER_CONN_PROXY_SERVER, PROXY_TYPE_DIRECT, PROXY_TYPE_PROXY,
};

const BYPASS: &str = "localhost;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;\
172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;192.168.*;<local>";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProxySettings {
    pub flags: u32,
    pub server: String,
    pub bypass: String,
    pub autoconfig_url: String,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set(settings: &ProxySettings) -> anyhow::Result<()> {
    let mut server = wide(&settings.server);
    let mut bypass = wide(&settings.bypass);
    let mut autoconfig = wide(&settings.autoconfig_url);
    let mut options = [INTERNET_PER_CONN_OPTIONW::default(); 4];
    options[0].dwOption = INTERNET_PER_CONN_FLAGS;
    options[0].Value.dwValue = settings.flags;
    options[1].dwOption = INTERNET_PER_CONN_PROXY_SERVER;
    options[1].Value.pszValue = PWSTR(server.as_mut_ptr());
    options[2].dwOption = INTERNET_PER_CONN_PROXY_BYPASS;
    options[2].Value.pszValue = PWSTR(bypass.as_mut_ptr());
    options[3].dwOption = INTERNET_PER_CONN_AUTOCONFIG_URL;
    options[3].Value.pszValue = PWSTR(autoconfig.as_mut_ptr());

    let list = INTERNET_PER_CONN_OPTION_LISTW {
        dwSize: std::mem::size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        pszConnection: PWSTR::null(),
        dwOptionCount: options.len() as u32,
        dwOptionError: 0,
        pOptions: options.as_mut_ptr(),
    };
    unsafe {
        InternetSetOptionW(
            None,
            INTERNET_OPTION_PER_CONNECTION_OPTION,
            Some(&list as *const _ as *const _),
            std::mem::size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        )?;
        let _ = InternetSetOptionW(None, INTERNET_OPTION_SETTINGS_CHANGED, None, 0);
        let _ = InternetSetOptionW(None, INTERNET_OPTION_REFRESH, None, 0);
    }
    Ok(())
}

/// Reads the current per-connection settings of the default (LAN) connection.
pub fn current() -> anyhow::Result<ProxySettings> {
    let ids: [INTERNET_PER_CONN; 4] = [
        INTERNET_PER_CONN_FLAGS,
        INTERNET_PER_CONN_PROXY_SERVER,
        INTERNET_PER_CONN_PROXY_BYPASS,
        INTERNET_PER_CONN_AUTOCONFIG_URL,
    ];
    let mut options = [INTERNET_PER_CONN_OPTIONW::default(); 4];
    for (o, id) in options.iter_mut().zip(ids) {
        o.dwOption = id;
    }
    let mut list = INTERNET_PER_CONN_OPTION_LISTW {
        dwSize: std::mem::size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        pszConnection: PWSTR::null(),
        dwOptionCount: options.len() as u32,
        dwOptionError: 0,
        pOptions: options.as_mut_ptr(),
    };
    let mut size = std::mem::size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32;
    unsafe {
        InternetQueryOptionW(None, INTERNET_OPTION_PER_CONNECTION_OPTION, Some(&mut list as *mut _ as *mut _), &mut size)?;
        // String values are allocated by WinINet with GlobalAlloc and must be freed by the caller.
        let take = |o: &INTERNET_PER_CONN_OPTIONW| -> String {
            let p = o.Value.pszValue;
            if p.is_null() {
                return String::new();
            }
            let s = p.to_string().unwrap_or_default();
            let _ = GlobalFree(Some(HGLOBAL(p.0 as *mut _)));
            s
        };
        Ok(ProxySettings {
            flags: options[0].Value.dwValue,
            server: take(&options[1]),
            bypass: take(&options[2]),
            autoconfig_url: take(&options[3]),
        })
    }
}

fn ours(port: u16) -> ProxySettings {
    ProxySettings {
        flags: PROXY_TYPE_PROXY | PROXY_TYPE_DIRECT,
        server: format!("127.0.0.1:{port}"),
        bypass: BYPASS.into(),
        autoconfig_url: String::new(),
    }
}

/// True when the system proxy currently points at our local port.
pub fn points_to(port: u16) -> bool {
    current().map(|c| c.flags & PROXY_TYPE_PROXY != 0 && c.server == ours(port).server).unwrap_or(false)
}

/// Saves the user's settings to `backup` (unless a backup from an unfinished session
/// already exists) and points the system proxy at the local port.
pub fn enable(port: u16, backup: &Path) -> anyhow::Result<()> {
    if !backup.exists() {
        let previous = current()?;
        std::fs::write(backup, serde_json::to_vec(&previous)?)?;
    }
    set(&ours(port))
}

/// Restores the settings saved by `enable`; without a backup falls back to "no proxy".
pub fn disable(backup: &Path) -> anyhow::Result<()> {
    let previous = std::fs::read(backup)
        .ok()
        .and_then(|b| serde_json::from_slice::<ProxySettings>(&b).ok())
        .unwrap_or(ProxySettings { flags: PROXY_TYPE_DIRECT, server: String::new(), bypass: String::new(), autoconfig_url: String::new() });
    set(&previous)?;
    let _ = std::fs::remove_file(backup);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touches the real system proxy (restored at the end): `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn enable_then_disable_restores_user_settings() {
        let original = current().unwrap();
        let backup = std::env::temp_dir().join(format!("literay-sysproxy-{}.json", std::process::id()));
        let user = ProxySettings {
            flags: PROXY_TYPE_DIRECT | 0x4, // PROXY_TYPE_AUTO_PROXY_URL
            server: "10.9.9.9:3128".into(),
            bypass: "*.corp.local;<local>".into(),
            autoconfig_url: "http://wpad.corp.local/proxy.pac".into(),
        };
        set(&user).unwrap();

        enable(10808, &backup).unwrap();
        assert!(points_to(10808));
        disable(&backup).unwrap();
        let after = current().unwrap();
        set(&original).unwrap();

        assert_eq!(after, user);
        assert!(!backup.exists());
        assert_eq!(current().unwrap(), original);
    }
}
