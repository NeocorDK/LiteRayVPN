//! System tray icon: connect/disconnect, show window, quit; tooltip with status and speed.

use std::sync::Arc;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::app::{self, AppState, ConnStatus};

const ICON_ON: &[u8] = include_bytes!("../icons/tray-on.png");
const ICON_OFF: &[u8] = include_bytes!("../icons/tray-off.png");

pub struct TrayState {
    tray: TrayIcon,
    show: MenuItem<Wry>,
    toggle: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

fn lang(app: &AppHandle) -> &'static str {
    let state = app.state::<Arc<AppState>>();
    let configured = state.store.lock().unwrap().settings.language.clone();
    match configured.as_str() {
        "ru" => "ru",
        "en" => "en",
        _ => crate::platform::system_language(),
    }
}

fn tr(lang: &str, key: &str) -> &'static str {
    let ru = lang == "ru";
    match key {
        "show" => if ru { "Открыть" } else { "Open" },
        "connect" => if ru { "Подключить" } else { "Connect" },
        "disconnect" => if ru { "Отключить" } else { "Disconnect" },
        "quit" => if ru { "Выход" } else { "Quit" },
        "connected" => if ru { "Подключено" } else { "Connected" },
        "connecting" => if ru { "Подключение…" } else { "Connecting…" },
        "disconnected" => if ru { "Отключено" } else { "Disconnected" },
        "error" => if ru { "Ошибка" } else { "Error" },
        _ => "",
    }
}

pub fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let l = lang(app);
    let show = MenuItem::with_id(app, "show", tr(l, "show"), true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", tr(l, "connect"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", tr(l, "quit"), true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &PredefinedMenuItem::separator(app)?, &quit])?;

    let tray = TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(ICON_OFF)?)
        .tooltip("LiteRay")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_window(app),
            "toggle" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if app.state::<Arc<AppState>>().is_connected() {
                        app::disconnect(&app).await;
                    } else if let Err(e) = app::connect(&app).await {
                        if e == app::ERR_NEED_ADMIN {
                            // The window explains the admin requirement and offers a restart.
                            show_window(&app);
                        }
                    }
                });
            }
            "quit" => {
                app::shutdown(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_window(tray.app_handle());
            }
        })
        .build(app)?;

    app.manage(TrayState { tray, show, toggle, quit });
    refresh(app);
    Ok(())
}

fn status_line(l: &str, status: &ConnStatus) -> String {
    match status {
        ConnStatus::Connected { profile_name, .. } => format!("{}: {profile_name}", tr(l, "connected")),
        ConnStatus::Connecting => tr(l, "connecting").to_string(),
        ConnStatus::Disconnected => tr(l, "disconnected").to_string(),
        ConnStatus::Error { .. } => tr(l, "error").to_string(),
    }
}

/// Re-reads status and language and updates menu texts, icon and tooltip.
pub fn refresh(app: &AppHandle) {
    let Some(t) = app.try_state::<TrayState>() else { return };
    let l = lang(app);
    let status = app.state::<Arc<AppState>>().status();
    let connected = matches!(status, ConnStatus::Connected { .. });
    let _ = t.show.set_text(tr(l, "show"));
    let _ = t.quit.set_text(tr(l, "quit"));
    let _ = t.toggle.set_text(tr(l, if connected { "disconnect" } else { "connect" }));
    let _ = t.toggle.set_enabled(!matches!(status, ConnStatus::Connecting));
    if let Ok(icon) = Image::from_bytes(if connected { ICON_ON } else { ICON_OFF }) {
        let _ = t.tray.set_icon(Some(icon));
    }
    let _ = t.tray.set_tooltip(Some(format!("LiteRay — {}", status_line(l, &status))));
}

pub fn format_speed(bytes_per_sec: u64) -> String {
    let b = bytes_per_sec as f64;
    if b >= 1024.0 * 1024.0 {
        format!("{:.1} MB/s", b / 1024.0 / 1024.0)
    } else {
        format!("{:.0} KB/s", b / 1024.0)
    }
}

pub fn set_speed(app: &AppHandle, down: u64, up: u64) {
    let Some(t) = app.try_state::<TrayState>() else { return };
    let l = lang(app);
    let status = app.state::<Arc<AppState>>().status();
    let tip = format!("LiteRay — {}\n↓ {}  ↑ {}", status_line(l, &status), format_speed(down), format_speed(up));
    let _ = t.tray.set_tooltip(Some(tip));
}
