use crate::{
    core::{accounts, model::Provider, poller, settings::Settings},
    runtime::{publish, AppState, Runtime},
};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

#[tauri::command]
pub async fn get_state(runtime: State<'_, Arc<Runtime>>) -> Result<AppState, String> {
    Ok(runtime.inner.lock().await.view.clone())
}

#[tauri::command]
pub fn get_popover_placement(
    visibility: State<'_, crate::core::tray::Visibility>,
) -> Result<crate::core::tray::PopoverPlacement, String> {
    visibility
        .placement
        .lock()
        .map(|placement| placement.clone())
        .map_err(|_| "Popover position is unavailable.".into())
}

#[tauri::command]
pub async fn refresh_accounts(runtime: State<'_, Arc<Runtime>>) -> Result<(), String> {
    poller::refresh(&runtime).await;
    Ok(())
}

#[tauri::command]
pub async fn discover_accounts(
    app: AppHandle,
    runtime: State<'_, Arc<Runtime>>,
) -> Result<AppState, String> {
    discover(&app, &runtime).await
}

pub async fn discover(app: &AppHandle, runtime: &Runtime) -> Result<AppState, String> {
    let existing = runtime.inner.lock().await.view.accounts.clone();
    let starting_ids: Vec<String> = existing.iter().map(|account| account.id.clone()).collect();
    let discovered = tauri::async_runtime::spawn_blocking(move || accounts::discover(&existing))
        .await
        .map_err(|_| "Account discovery could not finish.")?;
    runtime
        .change(app, move |state| {
            for account in discovered {
                if state.accounts.len() >= 100 {
                    break;
                }
                if !starting_ids.contains(&account.id)
                    && !state
                        .accounts
                        .iter()
                        .any(|existing| existing.id == account.id)
                {
                    state.accounts.push(account);
                }
            }
            if state.settings.pinned_account_id.is_none() {
                state.settings.pinned_account_id =
                    state.accounts.first().map(|account| account.id.clone());
            }
            Ok(())
        })
        .await
}

#[tauri::command]
pub async fn add_account(
    app: AppHandle,
    runtime: State<'_, Arc<Runtime>>,
    provider: Provider,
    path: String,
) -> Result<AppState, String> {
    if path.len() > 4096 {
        return Err("That folder path is too long.".into());
    }
    let account =
        tauri::async_runtime::spawn_blocking(move || accounts::add_config_dir(provider, &path))
            .await
            .map_err(|_| "Could not inspect that folder.")??;
    runtime
        .change(&app, move |state| {
            if state.accounts.iter().any(|item| item.id == account.id) {
                return Err("That account folder is already connected.".into());
            }
            if state.accounts.len() >= 100 {
                return Err("Margin supports up to 100 account folders.".into());
            }
            if state.settings.pinned_account_id.is_none() {
                state.settings.pinned_account_id = Some(account.id.clone());
            }
            state.accounts.push(account);
            Ok(())
        })
        .await
}

#[tauri::command]
pub async fn remove_account(
    app: AppHandle,
    runtime: State<'_, Arc<Runtime>>,
    id: String,
) -> Result<AppState, String> {
    runtime
        .change(&app, move |state| {
            if !state.accounts.iter().any(|account| account.id == id) {
                return Err("Account no longer exists.".into());
            }
            state.accounts.retain(|account| account.id != id);
            if state.settings.pinned_account_id.as_deref() == Some(id.as_str()) {
                state.settings.pinned_account_id =
                    state.accounts.first().map(|account| account.id.clone());
            }
            Ok(())
        })
        .await
}

#[tauri::command]
pub async fn rename_account(
    app: AppHandle,
    runtime: State<'_, Arc<Runtime>>,
    id: String,
    label: String,
) -> Result<AppState, String> {
    let label = label.trim().to_string();
    if label.is_empty() || label.chars().count() > 48 || label.chars().any(char::is_control) {
        return Err("Use an account name between 1 and 48 characters.".into());
    }
    runtime
        .change(&app, move |state| {
            let account = state
                .accounts
                .iter_mut()
                .find(|account| account.id == id)
                .ok_or("Account no longer exists.")?;
            account.label = label;
            Ok(())
        })
        .await
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    runtime: State<'_, Arc<Runtime>>,
    settings: Settings,
) -> Result<AppState, String> {
    settings.validate()?;
    runtime.ensure_store_available()?;
    let mut inner = runtime.inner.lock().await;
    if settings
        .pinned_account_id
        .as_ref()
        .is_some_and(|id| !inner.view.accounts.iter().any(|account| &account.id == id))
    {
        return Err("Choose an account that is still connected.".into());
    }
    let previous_autostart = if inner.view.settings.autostart != settings.autostart {
        app.autolaunch()
            .is_enabled()
            .map_err(|_| "Could not read launch-at-login status.")?
    } else {
        settings.autostart
    };
    if previous_autostart != settings.autostart {
        let result = if settings.autostart {
            app.autolaunch().enable()
        } else {
            app.autolaunch().disable()
        };
        result.map_err(|_| {
            "Could not update launch at login. Try again from an installed copy of Margin."
        })?;
    }
    let mut next = inner.view.clone();
    next.settings = settings;
    next.notice = None;
    next.changed();
    if let Err(error) = runtime.persist(&next, &inner.alerts) {
        if previous_autostart != next.settings.autostart {
            let _ = if previous_autostart {
                app.autolaunch().enable()
            } else {
                app.autolaunch().disable()
            };
        }
        return Err(error);
    }
    inner.view = next.clone();
    drop(inner);
    publish(&app, &next);
    poller::refresh(&runtime).await;
    Ok(next)
}

#[tauri::command]
pub fn hide_window(app: AppHandle) -> Result<(), String> {
    app.get_webview_window("main")
        .ok_or("Window is unavailable.")?
        .hide()
        .map_err(|_| "Could not close the popover.".into())
}
