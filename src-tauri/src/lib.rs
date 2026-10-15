mod commands;
pub mod core;
mod runtime;

use crate::core::{poller, tray};
use runtime::Runtime;
use std::{sync::Arc, time::Instant};
use tauri::{Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            tray::show(app, false)
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_positioner::init())
        .manage(tray::Visibility::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::refresh_accounts,
            commands::discover_accounts,
            commands::add_account,
            commands::remove_account,
            commands::rename_account,
            commands::save_settings,
            commands::hide_window,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let path = app.path().app_data_dir()?.join("state.json");
            let runtime = Arc::new(Runtime::new(path).map_err(std::io::Error::other)?);
            app.manage(runtime.clone());
            tray::setup(app.handle())?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Ok(enabled) = handle.autolaunch().is_enabled() {
                    let mut inner = runtime.inner.lock().await;
                    inner.view.settings.autostart = enabled;
                    inner.view.changed();
                }
                if runtime.first_run {
                    if let Err(error) = commands::discover(&handle, &runtime).await {
                        let mut inner = runtime.inner.lock().await;
                        inner.view.notice = Some(error);
                        inner.view.changed();
                    }
                }
                let view = runtime.inner.lock().await.view.clone();
                runtime::publish(&handle, &view);
                poller::start(handle.clone(), runtime);
                if !std::env::args().any(|argument| argument == "--background") {
                    tray::show(&handle, false);
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(false) => {
                if let Ok(mut last_blur) = window
                    .app_handle()
                    .state::<tray::Visibility>()
                    .last_blur
                    .lock()
                {
                    *last_blur = Some(Instant::now());
                }
                let _ = window.hide();
            }
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("Margin could not start")
        .run(|_app, _event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                tray::show(_app, false);
            }
        });
}
