//! Application controller: connection lifecycle, background tasks and state shared with the UI.

use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager};

use crate::net::{self, sysproxy};
use crate::paths::Paths;
use crate::platform::elevation;
use crate::profile::subscription;
use crate::state::Store;
use crate::tray;
use crate::xray::config_builder::{self, BuildOptions, ConnMode};
use crate::xray::runner::{LogBuffer, XrayRunner};
use crate::xray::stats::{self, SpeedMeter};
use crate::xray::updater;

pub const EVENT_STATUS: &str = "status";
pub const EVENT_STATS: &str = "stats";
pub const EVENT_CHANGED: &str = "changed";

/// Error code the UI recognizes to offer a restart with administrator rights.
pub const ERR_NEED_ADMIN: &str = "NEED_ADMIN";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ConnStatus {
    Disconnected,
    Connecting,
    #[serde(rename_all = "camelCase")]
    Connected { profile_id: String, profile_name: String, mode: ConnMode, since: i64 },
    Error { message: String },
}

struct Active {
    mode: ConnMode,
    mixed_port: u16,
}

pub struct AppState {
    pub paths: Paths,
    pub store: Mutex<Store>,
    pub log: LogBuffer,
    runner: Mutex<XrayRunner>,
    status: Mutex<ConnStatus>,
    active: Mutex<Option<Active>>,
    stats_task: Mutex<Option<JoinHandle<()>>>,
    /// Serializes connect/disconnect so two clicks cannot start two cores.
    op_lock: tokio::sync::Mutex<()>,
    pub http: reqwest::Client,
}

pub fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn http_client(proxy: Option<u16>) -> reqwest::Client {
    let mut b = reqwest::Client::builder()
        .user_agent(concat!("LiteRay/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120));
    b = match proxy {
        Some(port) => b.proxy(reqwest::Proxy::all(format!("socks5h://127.0.0.1:{port}")).expect("valid proxy url")),
        None => b.no_proxy(),
    };
    b.build().expect("http client")
}

impl AppState {
    pub fn new(paths: Paths) -> Self {
        let store = Store::load(&paths.state_file());
        let runner = XrayRunner::new();
        let log = runner.log_buffer();
        Self {
            paths,
            store: Mutex::new(store),
            log,
            runner: Mutex::new(runner),
            status: Mutex::new(ConnStatus::Disconnected),
            active: Mutex::new(None),
            stats_task: Mutex::new(None),
            op_lock: tokio::sync::Mutex::new(()),
            http: http_client(None),
        }
    }

    pub fn status(&self) -> ConnStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn is_connected(&self) -> bool {
        matches!(self.status(), ConnStatus::Connected { .. })
    }

    pub fn save(&self) -> Result<(), String> {
        self.store.lock().unwrap().save(&self.paths.state_file()).map_err(|e| e.to_string())
    }

    /// Client that tunnels through the running core, for fallbacks when direct access fails.
    pub fn proxied_http(&self) -> Option<reqwest::Client> {
        self.active.lock().unwrap().as_ref().map(|a| http_client(Some(a.mixed_port)))
    }
}

fn set_status(app: &AppHandle, status: ConnStatus) {
    let state = app.state::<Arc<AppState>>();
    *state.status.lock().unwrap() = status.clone();
    let _ = app.emit(EVENT_STATUS, &status);
    tray::refresh(app);
}

pub fn notify_changed(app: &AppHandle) {
    let _ = app.emit(EVENT_CHANGED, ());
    tray::refresh(app);
}

fn port_open(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
}

pub async fn connect(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<Arc<AppState>>().inner().clone();
    let _guard = state.op_lock.lock().await;
    stop_core(app, &state);

    let (profile, settings, routing) = {
        let store = state.store.lock().unwrap();
        let profile = store.selected_profile().cloned().ok_or("NO_PROFILE")?;
        (profile, store.settings.clone(), store.routing.clone())
    };
    if settings.mode == ConnMode::Tun && !elevation::is_elevated() {
        return Err(ERR_NEED_ADMIN.into());
    }
    if !net::port_is_free(settings.mixed_port) {
        return Err(format!("PORT_BUSY:{}", settings.mixed_port));
    }
    set_status(app, ConnStatus::Connecting);

    // Each start uses the other adapter name; if Windows is still removing that one too,
    // xray fails with "...already exists" and one more attempt with the next name follows.
    let mut result = Err(String::new());
    for _attempt in 0..2 {
        let tun_slot = {
            let mut store = state.store.lock().unwrap();
            store.tun_slot = store.tun_slot.wrapping_add(1) % 2;
            let _ = store.save(&state.paths.state_file());
            store.tun_slot
        };
        result = start_core(&state, &profile.server, &settings, &routing, tun_slot).await;
        match &result {
            Err(e) if settings.mode == ConnMode::Tun && e.contains("already exists") => continue,
            _ => break,
        }
    }

    let result = match result {
        Ok(metrics_port) => async {
            if settings.mode == ConnMode::SystemProxy {
                sysproxy::enable(settings.mixed_port, &state.paths.sysproxy_backup()).map_err(|e| e.to_string())?;
            }
            *state.active.lock().unwrap() = Some(Active { mode: settings.mode, mixed_port: settings.mixed_port });
            Ok::<u16, String>(metrics_port)
        }
        .await,
        Err(e) => Err(e),
    };

    match result {
        Ok(metrics_port) => {
            set_status(
                app,
                ConnStatus::Connected { profile_id: profile.id, profile_name: profile.name, mode: settings.mode, since: now_unix() },
            );
            start_stats(app, metrics_port);
            Ok(())
        }
        Err(e) => {
            stop_core(app, &state);
            set_status(app, ConnStatus::Error { message: e.clone() });
            Err(e)
        }
    }
}

/// Writes the config and starts xray; returns the metrics port once the core is ready.
async fn start_core(
    state: &Arc<AppState>,
    server: &crate::profile::Server,
    settings: &crate::state::Settings,
    routing: &crate::routing::RoutingSettings,
    tun_slot: u8,
) -> Result<u16, String> {
    let metrics_port = net::free_port().map_err(|e| e.to_string())?;
    let config = config_builder::build(&BuildOptions {
        server,
        mode: settings.mode,
        routing,
        mixed_port: settings.mixed_port,
        metrics_port,
        ipv6: settings.ipv6,
        self_exe: Paths::self_exe_for_rules(),
        tun_name: config_builder::tun_adapter_name(tun_slot),
    })
    .map_err(|e| e.to_string())?;
    let config_path = state.paths.run_config();
    std::fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).map_err(|e| e.to_string())?;

    let core_dir = state.paths.core_dir.clone();
    let st = state.clone();
    tauri::async_runtime::spawn_blocking(move || {
        st.runner
            .lock()
            .unwrap()
            // Generous: the first TUN start installs the wintun driver and registers a new adapter.
            .start(&core_dir, &config_path, || port_open(metrics_port), Duration::from_secs(30))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    Ok(metrics_port)
}

/// Stops the stats loop, the core and the system proxy. Does not change the status.
fn stop_core(_app: &AppHandle, state: &Arc<AppState>) {
    if let Some(task) = state.stats_task.lock().unwrap().take() {
        task.abort();
    }
    if let Some(active) = state.active.lock().unwrap().take() {
        if active.mode == ConnMode::SystemProxy {
            let _ = sysproxy::disable(&state.paths.sysproxy_backup());
        }
    }
    state.runner.lock().unwrap().stop();
}

pub async fn disconnect(app: &AppHandle) {
    let state = app.state::<Arc<AppState>>().inner().clone();
    let _guard = state.op_lock.lock().await;
    stop_core(app, &state);
    set_status(app, ConnStatus::Disconnected);
}

/// Synchronous cleanup for application exit.
pub fn shutdown(app: &AppHandle) {
    let state = app.state::<Arc<AppState>>().inner().clone();
    stop_core(app, &state);
}

/// Re-applies the configuration if connected (after a change of server, rules or settings).
pub async fn reconnect_if_connected(app: &AppHandle) -> Result<(), String> {
    if app.state::<Arc<AppState>>().is_connected() {
        connect(app).await
    } else {
        Ok(())
    }
}

fn start_stats(handle: &AppHandle, metrics_port: u16) {
    let app = handle.clone();
    let state = app.state::<Arc<AppState>>().inner().clone();
    let task = tauri::async_runtime::spawn(async move {
        let client = http_client(None);
        let mut meter = SpeedMeter::default();
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        loop {
            ticker.tick().await;
            if !state.runner.lock().unwrap().is_running() {
                let state2 = state.clone();
                // The core died on its own: undo the system proxy so the user keeps internet access.
                if let Some(active) = state2.active.lock().unwrap().take() {
                    if active.mode == ConnMode::SystemProxy {
                        let _ = sysproxy::disable(&state2.paths.sysproxy_backup());
                    }
                }
                set_status(&app, ConnStatus::Error { message: "CORE_STOPPED".into() });
                break;
            }
            if let Ok(sample) = stats::fetch(&client, metrics_port).await {
                let ev = meter.update(&sample);
                tray::set_speed(&app, ev.down_speed, ev.up_speed);
                let _ = app.emit(EVENT_STATS, &ev);
            }
        }
    });
    *handle.state::<Arc<AppState>>().stats_task.lock().unwrap() = Some(task);
}

/// Downloads one subscription and merges it into the store. Falls back to the tunnel
/// when the direct request fails and a connection is up.
pub async fn refresh_subscription(app: &AppHandle, sub_id: &str) -> Result<(), String> {
    let state = app.state::<Arc<AppState>>().inner().clone();
    let url = {
        let store = state.store.lock().unwrap();
        store.subscriptions.iter().find(|s| s.id == sub_id).map(|s| s.url.clone()).ok_or("subscription not found")?
    };

    let mut result = subscription::fetch(&state.http, &url, sub_id).await;
    if result.is_err() {
        if let Some(proxied) = state.proxied_http() {
            result = subscription::fetch(&proxied, &url, sub_id).await;
        }
    }

    // Server the tunnel is using right now, to detect whether the refresh changed it.
    let live = match state.status() {
        ConnStatus::Connected { profile_id, .. } => {
            let store = state.store.lock().unwrap();
            store.profiles.iter().find(|p| p.id == profile_id).map(|p| (p.identity(), p.server.clone()))
        }
        _ => None,
    };
    let outcome = {
        let mut store = state.store.lock().unwrap();
        let outcome = match result {
            Ok(fetched) => {
                if let Some(sub) = store.subscriptions.iter_mut().find(|s| s.id == sub_id) {
                    sub.last_update = Some(now_unix());
                    sub.last_error = None;
                    sub.userinfo = fetched.userinfo;
                    sub.update_interval_hours = fetched.update_interval_hours;
                    sub.skipped = fetched.skipped;
                    if let Some(title) = fetched.title {
                        if sub.name.is_empty() || sub.name == sub.url {
                            sub.name = title;
                        }
                    }
                }
                store.replace_subscription_profiles(sub_id, fetched.profiles);
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                if let Some(sub) = store.subscriptions.iter_mut().find(|s| s.id == sub_id) {
                    sub.last_error = Some(msg.clone());
                }
                Err(msg)
            }
        };
        let _ = store.save(&state.paths.state_file());
        outcome
    };
    notify_changed(app);

    // Profiles get new ids on every refresh. Re-point the live status at the new id, and
    // restart the tunnel only if the server's parameters actually changed.
    if let (Ok(()), Some((identity, server))) = (&outcome, live) {
        let replacement = {
            let store = state.store.lock().unwrap();
            store.selected_profile().filter(|p| p.identity() == identity).map(|p| (p.id.clone(), p.name.clone(), p.server == server))
        };
        match replacement {
            Some((id, name, true)) => {
                if let ConnStatus::Connected { mode, since, .. } = state.status() {
                    set_status(app, ConnStatus::Connected { profile_id: id, profile_name: name, mode, since });
                }
            }
            _ => {
                let _ = reconnect_if_connected(app).await;
            }
        }
    }
    outcome
}

/// Background loop: subscription auto-refresh and weekly geo file updates.
pub fn spawn_background(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let mut ticker = tokio::time::interval(Duration::from_secs(15 * 60));
        loop {
            ticker.tick().await;
            let state = app.state::<Arc<AppState>>().inner().clone();
            let due: Vec<String> = {
                let store = state.store.lock().unwrap();
                let fallback = store.settings.sub_update_hours.max(1);
                store
                    .subscriptions
                    .iter()
                    .filter(|s| {
                        let hours = s.update_interval_hours.filter(|h| *h > 0).unwrap_or(fallback) as i64;
                        s.last_update.is_none_or(|t| now_unix() - t >= hours * 3600)
                    })
                    .map(|s| s.id.clone())
                    .collect()
            };
            for id in due {
                let _ = refresh_subscription(&app, &id).await;
            }

            let (geo_due, repo) = {
                let mut store = state.store.lock().unwrap();
                let s = &mut store.settings;
                if s.last_geo_update.is_none() {
                    // Bundled files are fresh enough on first run.
                    s.last_geo_update = Some(now_unix());
                }
                (now_unix() - s.last_geo_update.unwrap_or(0) >= 7 * 24 * 3600, s.geo_repo.clone())
            };
            if geo_due && update_geo(&app, &repo).await.is_ok() {
                log::info!("geo files updated");
            }
            let _ = state.save();
        }
    });
}

pub async fn update_geo(app: &AppHandle, repo: &str) -> Result<(), String> {
    let state = app.state::<Arc<AppState>>().inner().clone();
    let core_dir = state.paths.core_dir.clone();
    let mut result = updater::update_geo(&state.http, repo, &core_dir).await;
    if result.is_err() {
        if let Some(proxied) = state.proxied_http() {
            result = updater::update_geo(&proxied, repo, &core_dir).await;
        }
    }
    result.map_err(|e| e.to_string())?;
    state.store.lock().unwrap().settings.last_geo_update = Some(now_unix());
    state.save()?;
    // New geo data takes effect on the next core start.
    reconnect_if_connected(app).await
}

pub async fn install_core(app: &AppHandle, include_prerelease: bool) -> Result<String, String> {
    let state = app.state::<Arc<AppState>>().inner().clone();
    let client = state.http.clone();
    let release = updater::latest_core_release(&client, include_prerelease).await.map_err(|e| e.to_string())?;
    // Download while the tunnel keeps working; stop the core only for the file swap.
    let exe = updater::download_core(&client, &release).await.map_err(|e| e.to_string())?;
    let was_connected = state.is_connected();
    let applied = {
        let _guard = state.op_lock.lock().await;
        stop_core(app, &state);
        updater::apply_core(&state.paths.core_dir, &exe, &release.tag)
    };
    if was_connected {
        connect(app).await?;
    } else {
        set_status(app, ConnStatus::Disconnected);
    }
    applied.map_err(|e| e.to_string())?;
    Ok(release.tag)
}
