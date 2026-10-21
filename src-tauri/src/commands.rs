//! Tauri commands invoked from the UI. All are async so none of them blocks the UI thread.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::app::{self, AppState, ConnStatus};
use crate::platform::{autostart, elevation};
use crate::profile::subscription::{self, Subscription};
use crate::profile::new_id;
use crate::routing::{self, RoutingSettings, RuleKind};
use crate::state::{Settings, Store};
use crate::xray::runner;
use crate::xray::updater::{self, CoreRelease};

type Res<T> = Result<T, String>;
type St<'a> = State<'a, Arc<AppState>>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    store: Store,
    status: ConnStatus,
    elevated: bool,
    portable: bool,
    core_version: Option<String>,
    system_language: &'static str,
    app_version: &'static str,
    data_dir: String,
}

#[tauri::command]
pub async fn get_snapshot(state: St<'_>) -> Res<Snapshot> {
    Ok(Snapshot {
        store: state.store.lock().unwrap().clone(),
        status: state.status(),
        elevated: elevation::is_elevated(),
        portable: state.paths.portable,
        core_version: runner::core_version(&state.paths.core_dir),
        system_language: crate::platform::system_language(),
        app_version: env!("CARGO_PKG_VERSION"),
        data_dir: state.paths.data_dir.display().to_string(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    added: usize,
    skipped: usize,
    subscription: bool,
}

fn looks_like_subscription(text: &str) -> bool {
    let t = text.trim();
    (t.starts_with("http://") || t.starts_with("https://")) && !t.contains(char::is_whitespace)
}

/// Accepts either share links (one or many, plain or base64) or a subscription URL.
#[tauri::command]
pub async fn import_text(app: AppHandle, state: St<'_>, text: String) -> Res<ImportResult> {
    if looks_like_subscription(&text) {
        add_subscription(app, state, text.trim().to_string()).await?;
        return Ok(ImportResult { added: 0, skipped: 0, subscription: true });
    }
    let links = subscription::extract_links(&text);
    if links.is_empty() {
        return Err("NO_LINKS".into());
    }
    let (profiles, skipped) = subscription::profiles_from_links(&links, None);
    let added = profiles.len();
    {
        let mut store = state.store.lock().unwrap();
        if store.selected.is_none() {
            store.selected = profiles.first().map(|p| p.id.clone());
        }
        store.profiles.extend(profiles);
    }
    state.save()?;
    app::notify_changed(&app);
    Ok(ImportResult { added, skipped, subscription: false })
}

#[tauri::command]
pub async fn add_subscription(app: AppHandle, state: St<'_>, url: String) -> Res<()> {
    let url = url.trim().to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("BAD_URL".into());
    }
    let id = new_id();
    {
        let mut store = state.store.lock().unwrap();
        if store.subscriptions.iter().any(|s| s.url == url) {
            return Err("DUPLICATE".into());
        }
        store.subscriptions.push(Subscription {
            id: id.clone(),
            name: String::new(),
            url,
            last_update: None,
            update_interval_hours: None,
            userinfo: None,
            last_error: None,
            skipped: 0,
        });
    }
    state.save()?;
    app::notify_changed(&app);
    app::refresh_subscription(&app, &id).await
}

#[tauri::command]
pub async fn refresh_subscription(app: AppHandle, id: String) -> Res<()> {
    app::refresh_subscription(&app, &id).await
}

#[tauri::command]
pub async fn rename_subscription(app: AppHandle, state: St<'_>, id: String, name: String) -> Res<()> {
    if let Some(s) = state.store.lock().unwrap().subscriptions.iter_mut().find(|s| s.id == id) {
        s.name = name.trim().to_string();
    }
    state.save()?;
    app::notify_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn delete_subscription(app: AppHandle, state: St<'_>, id: String) -> Res<()> {
    {
        let mut store = state.store.lock().unwrap();
        store.subscriptions.retain(|s| s.id != id);
        store.profiles.retain(|p| p.subscription_id.as_deref() != Some(&id));
        if store.selected_profile().is_none() {
            store.selected = store.profiles.first().map(|p| p.id.clone());
        }
    }
    state.save()?;
    app::notify_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn delete_profile(app: AppHandle, state: St<'_>, id: String) -> Res<()> {
    {
        let mut store = state.store.lock().unwrap();
        store.profiles.retain(|p| p.id != id);
        if store.selected_profile().is_none() {
            store.selected = store.profiles.first().map(|p| p.id.clone());
        }
    }
    state.save()?;
    app::notify_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn select_profile(app: AppHandle, state: St<'_>, id: String) -> Res<()> {
    {
        let mut store = state.store.lock().unwrap();
        if !store.profiles.iter().any(|p| p.id == id) {
            return Err("profile not found".into());
        }
        store.selected = Some(id);
    }
    state.save()?;
    app::notify_changed(&app);
    app::reconnect_if_connected(&app).await
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PingEvent {
    id: String,
    ms: Option<u32>,
}

/// TCP pings servers in parallel; each result is pushed as a "ping" event as it arrives.
#[tauri::command]
pub async fn ping_profiles(app: AppHandle, state: St<'_>, ids: Vec<String>) -> Res<()> {
    let targets: Vec<(String, String, u16)> = {
        let store = state.store.lock().unwrap();
        store
            .profiles
            .iter()
            .filter(|p| ids.is_empty() || ids.contains(&p.id))
            .map(|p| (p.id.clone(), p.server.address.clone(), p.server.port))
            .collect()
    };
    let sem = Arc::new(tokio::sync::Semaphore::new(16));
    let mut tasks = Vec::new();
    for (id, host, port) in targets {
        let app = app.clone();
        let sem = sem.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            let _permit = sem.acquire().await;
            let ms = crate::net::ping::tcp_ping(&host, port, Duration::from_secs(3)).await;
            let _ = app.emit("ping", PingEvent { id, ms });
        }));
    }
    for t in tasks {
        let _ = t.await;
    }
    Ok(())
}

#[tauri::command]
pub async fn normalize_rule(kind: RuleKind, value: String) -> Res<String> {
    routing::normalize(kind, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_routing(app: AppHandle, state: St<'_>, routing: RoutingSettings) -> Res<RoutingSettings> {
    let norm = |kind, list: &[String]| -> Res<Vec<String>> {
        let mut out: Vec<String> = Vec::new();
        for v in list {
            let n = routing::normalize(kind, v).map_err(|e| format!("{v}: {e}"))?;
            if !out.contains(&n) {
                out.push(n);
            }
        }
        Ok(out)
    };
    let routing = RoutingSettings {
        apps: norm(RuleKind::App, &routing.apps)?,
        domains: norm(RuleKind::Domain, &routing.domains)?,
        ips: norm(RuleKind::Ip, &routing.ips)?,
        ..routing
    };
    state.store.lock().unwrap().routing = routing.clone();
    state.save()?;
    app::notify_changed(&app);
    app::reconnect_if_connected(&app).await?;
    Ok(routing)
}

#[tauri::command]
pub async fn set_settings(app: AppHandle, state: St<'_>, settings: Settings) -> Res<()> {
    let old = state.store.lock().unwrap().settings.clone();
    if settings.mixed_port < 1024 {
        return Err("BAD_PORT".into());
    }
    let elevated = settings.mode == crate::xray::config_builder::ConnMode::Tun;
    if settings.autostart != old.autostart || (settings.autostart && settings.mode != old.mode) {
        if settings.autostart && elevated && !elevation::is_elevated() {
            return Err(app::ERR_NEED_ADMIN.into());
        }
        autostart::set(settings.autostart, elevated).map_err(|e| e.to_string())?;
    }
    let needs_restart = settings.mode != old.mode || settings.ipv6 != old.ipv6 || settings.mixed_port != old.mixed_port;
    state.store.lock().unwrap().settings = settings;
    state.save()?;
    app::notify_changed(&app);
    if needs_restart {
        app::reconnect_if_connected(&app).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn connect(app: AppHandle) -> Res<()> {
    app::connect(&app).await
}

#[tauri::command]
pub async fn disconnect(app: AppHandle) -> Res<()> {
    app::disconnect(&app).await;
    Ok(())
}

/// Relaunches the client with administrator rights and connects right away.
#[tauri::command]
pub async fn restart_as_admin(app: AppHandle) -> Res<()> {
    elevation::relaunch_elevated(&["--connect"]).map_err(|e| e.to_string())?;
    app::shutdown(&app);
    app.exit(0);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    name: String,
    path: String,
}

/// Running processes with a known executable path, one entry per path.
#[tauri::command]
pub async fn list_processes() -> Res<Vec<ProcessInfo>> {
    tauri::async_runtime::spawn_blocking(|| {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_exe(UpdateKind::Always));
        let windows_dir = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()).to_lowercase();
        let mut by_path: BTreeMap<String, ProcessInfo> = BTreeMap::new();
        for p in sys.processes().values() {
            let Some(exe) = p.exe() else { continue };
            let path = exe.to_string_lossy().to_string();
            if path.is_empty() || path.to_lowercase().starts_with(&windows_dir) {
                continue;
            }
            let name = exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            by_path.entry(path.to_lowercase()).or_insert(ProcessInfo { name, path });
        }
        let mut list: Vec<ProcessInfo> = by_path.into_values().collect();
        list.sort_by_key(|p| p.name.to_lowercase());
        list
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_logs(state: St<'_>) -> Res<Vec<String>> {
    Ok(state.log.lock().unwrap().iter().cloned().collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreUpdateInfo {
    current: Option<String>,
    latest: CoreRelease,
    newer: bool,
}

#[tauri::command]
pub async fn check_core_update(state: St<'_>) -> Res<CoreUpdateInfo> {
    let prerelease = state.store.lock().unwrap().settings.core_prerelease;
    let mut result = updater::latest_core_release(&state.http, prerelease).await;
    if result.is_err() {
        if let Some(proxied) = state.proxied_http() {
            result = updater::latest_core_release(&proxied, prerelease).await;
        }
    }
    let latest = result.map_err(|e| e.to_string())?;
    let current = runner::core_version(&state.paths.core_dir);
    let newer = updater::is_newer(current.as_deref(), &latest.tag);
    Ok(CoreUpdateInfo { current, latest, newer })
}

#[tauri::command]
pub async fn install_core_update(app: AppHandle, state: St<'_>) -> Res<String> {
    let prerelease = state.store.lock().unwrap().settings.core_prerelease;
    app::install_core(&app, prerelease).await
}

#[tauri::command]
pub async fn update_geo(app: AppHandle, state: St<'_>) -> Res<()> {
    let repo = state.store.lock().unwrap().settings.geo_repo.clone();
    app::update_geo(&app, &repo).await
}

#[tauri::command]
pub async fn open_data_dir(state: St<'_>) -> Res<()> {
    std::process::Command::new("explorer").arg(&state.paths.data_dir).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        get_snapshot,
        import_text,
        add_subscription,
        refresh_subscription,
        rename_subscription,
        delete_subscription,
        delete_profile,
        select_profile,
        ping_profiles,
        normalize_rule,
        set_routing,
        set_settings,
        connect,
        disconnect,
        restart_as_admin,
        list_processes,
        get_logs,
        check_core_update,
        install_core_update,
        update_geo,
        open_data_dir,
    ]
}
