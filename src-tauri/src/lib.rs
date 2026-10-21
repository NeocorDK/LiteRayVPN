mod app;
mod commands;
mod net;
mod paths;
mod platform;
mod profile;
mod routing;
mod state;
mod tray;
mod xray;

use std::sync::Arc;

use tauri::{Manager, RunEvent, WindowEvent};

use app::AppState;

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    // An elevated relaunch waits for the old instance to exit before claiming the single instance.
    platform::elevation::wait_for_parent_exit(&args, 10_000);

    let paths = paths::Paths::detect().expect("cannot prepare data folders");
    let state = Arc::new(AppState::new(paths));

    // A crash while in proxy mode leaves Windows pointing at a dead local port: restore the
    // user's own settings. If they changed the proxy since, keep theirs and drop the backup.
    let mixed_port = state.store.lock().unwrap().settings.mixed_port;
    let backup = state.paths.sysproxy_backup();
    if net::sysproxy::points_to(mixed_port) {
        let _ = net::sysproxy::disable(&backup);
    } else {
        let _ = std::fs::remove_file(&backup);
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            tray::show_window(app);
            if has_flag(&argv, "--connect") {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = app::connect(&app).await;
                });
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(state)
        .invoke_handler(commands::handler())
        .setup(move |app| {
            let handle = app.handle().clone();
            let state = handle.state::<Arc<AppState>>().inner().clone();

            if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
                if let Err(e) = state.paths.seed_core(&paths::bundled_core_dir(&exe_dir)) {
                    log::warn!("cannot copy bundled core: {e}");
                }
            }
            tray::create(&handle)?;
            app::spawn_background(&handle);

            let settings = state.store.lock().unwrap().settings.clone();
            let autostarted = has_flag(&args, "--autostart");
            if !(autostarted && settings.start_minimized) {
                tray::show_window(&handle);
            }
            if has_flag(&args, "--connect") || (autostarted && settings.connect_on_start) {
                tauri::async_runtime::spawn(async move {
                    let _ = app::connect(&handle).await;
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps the client running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building LiteRay");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            app::shutdown(handle);
        }
    });
}
