//! Persistent client data (state.json) and the in-memory application state.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::profile::subscription::Subscription;
use crate::profile::Profile;
use crate::routing::RoutingSettings;
use crate::xray::config_builder::ConnMode;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub mode: ConnMode,
    /// "auto" | "ru" | "en"
    pub language: String,
    pub autostart: bool,
    pub connect_on_start: bool,
    pub start_minimized: bool,
    pub mixed_port: u16,
    pub ipv6: bool,
    pub core_prerelease: bool,
    pub geo_repo: String,
    /// Fallback when the subscription does not suggest an interval.
    pub sub_update_hours: u32,
    pub last_geo_update: Option<i64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: ConnMode::Tun,
            language: "auto".into(),
            autostart: false,
            connect_on_start: false,
            start_minimized: false,
            mixed_port: 10808,
            ipv6: false,
            core_prerelease: true,
            geo_repo: "runetfreedom/russia-v2ray-rules-dat".into(),
            sub_update_hours: 12,
            last_geo_update: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Store {
    pub settings: Settings,
    pub profiles: Vec<Profile>,
    pub subscriptions: Vec<Subscription>,
    pub routing: RoutingSettings,
    pub selected: Option<String>,
    /// Last TUN adapter slot, persisted so a session after a crash avoids the stale name.
    pub tun_slot: u8,
}

impl Store {
    pub fn load(path: &Path) -> Self {
        match std::fs::read(path) {
            // Notepad and PowerShell 5 write a UTF-8 BOM, which serde_json rejects.
            Ok(bytes) => serde_json::from_slice(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes)).unwrap_or_else(|e| {
                // Keep the unreadable file for manual recovery instead of silently losing it.
                log::warn!("state.json is corrupt ({e}); starting with defaults");
                let _ = std::fs::copy(path, path.with_extension("json.corrupt"));
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Atomic write: temp file then rename, so a crash never leaves a half-written state.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn selected_profile(&self) -> Option<&Profile> {
        let id = self.selected.as_ref()?;
        self.profiles.iter().find(|p| &p.id == id)
    }

    /// Replaces the profiles of one subscription, keeping the selection if the selected
    /// server is still present (matched by address, port and credential).
    pub fn replace_subscription_profiles(&mut self, sub_id: &str, new_profiles: Vec<Profile>) {
        let selected_identity = self
            .selected_profile()
            .filter(|p| p.subscription_id.as_deref() == Some(sub_id))
            .map(Profile::identity);
        let insert_at = self
            .profiles
            .iter()
            .position(|p| p.subscription_id.as_deref() == Some(sub_id))
            .unwrap_or(self.profiles.len());
        self.profiles.retain(|p| p.subscription_id.as_deref() != Some(sub_id));
        if let Some(identity) = selected_identity {
            self.selected = new_profiles.iter().find(|p| p.identity() == identity).map(|p| p.id.clone());
        }
        let insert_at = insert_at.min(self.profiles.len());
        self.profiles.splice(insert_at..insert_at, new_profiles);
        if self.selected.is_none() {
            self.selected = self.profiles.first().map(|p| p.id.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Server, Protocol};

    fn profile(id: &str, sub: Option<&str>, addr: &str) -> Profile {
        Profile {
            id: id.into(),
            name: id.into(),
            subscription_id: sub.map(String::from),
            link: String::new(),
            server: Server { protocol: Some(Protocol::Vless), address: addr.into(), port: 443, credential: "u".into(), ..Default::default() },
        }
    }

    #[test]
    fn refresh_keeps_selection_and_position() {
        let mut store = Store {
            profiles: vec![profile("manual", None, "m"), profile("a", Some("s"), "h1"), profile("b", Some("s"), "h2"), profile("tail", None, "t")],
            selected: Some("b".into()),
            ..Default::default()
        };
        store.replace_subscription_profiles("s", vec![profile("b2", Some("s"), "h2"), profile("c2", Some("s"), "h3")]);
        let ids: Vec<&str> = store.profiles.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["manual", "b2", "c2", "tail"]);
        assert_eq!(store.selected.as_deref(), Some("b2"));
    }

    #[test]
    fn refresh_without_match_falls_back_to_first() {
        let mut store = Store { profiles: vec![profile("a", Some("s"), "h1")], selected: Some("a".into()), ..Default::default() };
        store.replace_subscription_profiles("s", vec![profile("z", Some("s"), "other")]);
        assert_eq!(store.selected.as_deref(), Some("z"));
    }

    #[test]
    fn loads_file_with_bom() {
        let path = std::env::temp_dir().join(format!("literay-bom-{}.json", std::process::id()));
        std::fs::write(&path, b"\xEF\xBB\xBF{\"selected\": \"x\"}").unwrap();
        let store = Store::load(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(store.selected.as_deref(), Some("x"));
    }

    #[test]
    fn old_state_files_get_defaults() {
        let store: Store = serde_json::from_str(r#"{"settings": {"language": "ru"}}"#).unwrap();
        assert_eq!(store.settings.language, "ru");
        assert_eq!(store.settings.mixed_port, 10808);
        assert!(store.routing.ru_direct);
    }
}
