use super::{
    model::{Account, UsageWindow},
    poller,
    settings::TrayMode,
};
use crate::runtime::{AppState, Runtime};
use chrono::Utc;
use std::{
    f64::consts::{FRAC_PI_2, TAU},
    sync::{Arc, Mutex},
    time::Instant,
};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tauri_plugin_positioner::{Position, WindowExt};

#[derive(Default)]
struct Presentation {
    revision: u64,
    pixels: Vec<u8>,
    tooltip: String,
}

#[derive(Default)]
pub struct Visibility {
    pub last_blur: Mutex<Option<Instant>>,
    presentation: Mutex<Presentation>,
}

fn limiting_window(account: &Account) -> Option<&UsageWindow> {
    account
        .snapshot
        .as_ref()?
        .windows
        .iter()
        .filter(|window| window.used_percent.is_finite())
        .max_by(|left, right| left.used_percent.total_cmp(&right.used_percent))
}

fn remaining(account: &Account) -> Option<f64> {
    limiting_window(account).map(|window| 100.0 - window.used_percent)
}

fn stale(account: &Account, state: &AppState) -> bool {
    if account.error.is_some() {
        return true;
    }
    account.snapshot.as_ref().is_some_and(|snapshot| {
        let now = Utc::now();
        let old =
            chrono::DateTime::parse_from_rfc3339(&snapshot.fetched_at).map_or(true, |fetched| {
                (now - fetched.with_timezone(&Utc)).num_seconds()
                    > (state.settings.idle_poll_interval_seconds * 2).max(180) as i64
            });
        old || snapshot.windows.iter().any(|window| {
            window.resets_at.as_ref().is_some_and(|reset| {
                chrono::DateTime::parse_from_rfc3339(reset).map_or(true, |reset| reset <= now)
            })
        })
    })
}

fn selected(state: &AppState) -> Option<&Account> {
    if state.settings.tray_mode == TrayMode::Pinned {
        return state
            .settings
            .pinned_account_id
            .as_ref()
            .and_then(|id| state.accounts.iter().find(|account| &account.id == id))
            .or_else(|| state.accounts.first());
    }
    state
        .accounts
        .iter()
        .filter(|account| !stale(account, state))
        .filter_map(|account| remaining(account).map(|value| (account, value)))
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(account, _)| account)
        .or_else(|| state.accounts.first())
}

pub fn ring_rgba(remaining: Option<f64>, stale: bool, monochrome: bool) -> Vec<u8> {
    let size = 44;
    let mut bytes = vec![0_u8; size * size * 4];
    let value = remaining.map(|value| value.clamp(0.0, 100.0) / 100.0);
    for y in 0..size {
        for x in 0..size {
            let mut alpha = 0.0_f64;
            for sy in 0..4 {
                for sx in 0..4 {
                    let dx = x as f64 + (sx as f64 + 0.5) / 4.0 - 22.0;
                    let dy = y as f64 + (sy as f64 + 0.5) / 4.0 - 22.0;
                    let radius = dx.hypot(dy);
                    let angle = (dy.atan2(dx) + FRAC_PI_2).rem_euclid(TAU) / TAU;
                    if (14.5..18.5).contains(&radius) {
                        alpha += match value {
                            Some(value) if angle <= value => 255.0,
                            Some(_) => 55.0,
                            None if (angle * 12.0).fract() < 0.55 => 170.0,
                            None => 0.0,
                        };
                    }
                    if stale && dx.abs() <= 4.5 && dy.abs() <= 1.2 {
                        alpha += 255.0;
                    }
                }
            }
            let offset = (y * size + x) * 4;
            let color = if monochrome {
                [0, 0, 0]
            } else if value.is_some_and(|value| value <= 0.05) {
                [224, 156, 98]
            } else {
                [134, 174, 151]
            };
            bytes[offset..offset + 3].copy_from_slice(&color);
            bytes[offset + 3] = (alpha / 16.0).round().min(255.0) as u8;
        }
    }
    bytes
}

pub fn update(app: &AppHandle, state: &AppState) {
    let account = selected(state);
    let amount = account.and_then(remaining);
    let is_stale = account.is_some_and(|account| stale(account, state));
    let pixels = ring_rgba(amount, is_stale, cfg!(target_os = "macos"));
    let tooltip = match (account, amount) {
        (Some(account), Some(amount)) => {
            let window = limiting_window(account)
                .map(|window| window.label.chars().take(64).collect::<String>())
                .unwrap_or_default();
            let amount = if amount > 0.0 && amount < 1.0 {
                "<1".into()
            } else {
                format!("{:.0}", amount.floor())
            };
            format!(
                "Margin · {} · {} · {}% remaining{}",
                account.label,
                window,
                amount,
                if is_stale { " · last known" } else { "" }
            )
        }
        (Some(account), None) => format!("Margin · {} · usage unavailable", account.label),
        _ => "Margin · Add an account".into(),
    };
    let revision = state.revision;
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(tray) = handle.tray_by_id("margin") else {
            return;
        };
        let visibility = handle.state::<Visibility>();
        let Ok(mut previous) = visibility.presentation.lock() else {
            return;
        };
        if revision < previous.revision {
            return;
        }
        previous.revision = revision;
        if previous.pixels != pixels {
            let updated = tray.set_icon_with_as_template(
                Some(Image::new_owned(pixels.clone(), 44, 44)),
                cfg!(target_os = "macos"),
            );
            if updated.is_ok() {
                previous.pixels = pixels;
            }
        }
        if previous.tooltip != tooltip && tray.set_tooltip(Some(&tooltip)).is_ok() {
            previous.tooltip = tooltip;
        }
    });
}

pub fn show(app: &AppHandle, settings: bool) {
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "macos")]
        let position = Position::TrayBottomCenter;
        #[cfg(not(target_os = "macos"))]
        let position = Position::TrayCenter;
        if window.move_window_constrained(position).is_err() {
            let _ = window.center();
        }
        let anchor = app
            .tray_by_id("margin")
            .and_then(|tray| tray.rect().ok().flatten())
            .map(|rect| {
                (
                    rect.position.to_physical::<f64>(1.0),
                    rect.size.to_physical::<f64>(1.0),
                )
            });
        let monitor = anchor
            .and_then(|(position, _)| {
                window
                    .monitor_from_point(position.x, position.y)
                    .ok()
                    .flatten()
            })
            .or_else(|| window.current_monitor().ok().flatten());
        if let Some(monitor) = monitor {
            let work = monitor.work_area();
            let scale = monitor.scale_factor();
            let width = (420.0 * scale).round().min(f64::from(work.size.width));
            let height = (660.0 * scale).round().min(f64::from(work.size.height));
            let _ = window.set_size(tauri::PhysicalSize::new(width as u32, height as u32));
            let left = f64::from(work.position.x);
            let top = f64::from(work.position.y);
            let right = left + f64::from(work.size.width);
            let bottom = top + f64::from(work.size.height);
            let (x, y) = match anchor {
                Some((tray_position, tray_size)) => {
                    let above = tray_position.y - height;
                    let y = if cfg!(target_os = "macos") || above < top {
                        tray_position.y + tray_size.height
                    } else {
                        above
                    };
                    (tray_position.x + tray_size.width / 2.0 - width / 2.0, y)
                }
                None => (
                    left + (right - left - width) / 2.0,
                    top + (bottom - top - height) / 2.0,
                ),
            };
            let x = x.clamp(left, (right - width).max(left));
            let y = y.clamp(top, (bottom - height).max(top));
            let _ = window.set_position(tauri::PhysicalPosition::new(
                x.round() as i32,
                y.round() as i32,
            ));
        }
        let _ = window.show();
        let _ = window.set_focus();
        if settings {
            let _ = app.emit("open_settings", ());
        }
        let runtime = app.state::<Arc<Runtime>>().inner().clone();
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            let view = runtime.inner.lock().await.view.clone();
            crate::runtime::publish(&handle, &view);
            poller::refresh(&runtime).await;
        });
    }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "Open Margin", true, None::<&str>)?;
    let refresh_item = MenuItem::with_id(app, "refresh", "Refresh usage", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Margin", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &show_item,
            &refresh_item,
            &settings_item,
            &separator,
            &quit_item,
        ],
    )?;
    TrayIconBuilder::with_id("margin")
        .icon(Image::new_owned(
            ring_rgba(None, false, cfg!(target_os = "macos")),
            44,
            44,
        ))
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Margin · Checking accounts")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show(app, false),
            "settings" => show(app, true),
            "refresh" => {
                let runtime = app.state::<Arc<Runtime>>().inner().clone();
                tauri::async_runtime::spawn(async move {
                    poller::refresh(&runtime).await;
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            tauri_plugin_positioner::on_tray_event(app, &event);
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let blurred = app
                    .state::<Visibility>()
                    .last_blur
                    .lock()
                    .ok()
                    .and_then(|value| *value)
                    .is_some_and(|time| time.elapsed().as_millis() < 250);
                if let Some(window) = app.get_webview_window("main") {
                    if window.is_visible().unwrap_or(false) || blurred {
                        let _ = window.hide();
                    } else {
                        show(app, false);
                    }
                }
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        model::{AccountSource, Identity, Provider, Snapshot, UsageError, UsageWindow},
        settings::Settings,
    };

    fn account(id: &str, usage: &[f64]) -> Account {
        Account {
            id: id.into(),
            provider: Provider::Claude,
            label: id.into(),
            color: "#ffffff".into(),
            source: AccountSource::Keychain,
            identity: Identity::default(),
            snapshot: Some(Snapshot {
                windows: usage
                    .iter()
                    .enumerate()
                    .map(|(index, used)| UsageWindow {
                        id: format!("window_{index}"),
                        label: "Session".into(),
                        used_percent: *used,
                        resets_at: None,
                        window_seconds: None,
                    })
                    .collect(),
                fetched_at: Utc::now().to_rfc3339(),
                extra_usage: None,
            }),
            error: None,
            refreshing: false,
        }
    }

    #[test]
    fn lowest_remaining_uses_all_windows_and_excludes_stale_accounts() {
        let mut state = AppState {
            revision: 0,
            accounts: vec![account("a", &[10.0, 98.0]), account("b", &[90.0])],
            settings: Settings {
                tray_mode: TrayMode::Lowest,
                ..Settings::default()
            },
            refreshing: false,
            notice: None,
        };
        assert_eq!(selected(&state).unwrap().id, "a");
        state.accounts[0].error = Some(UsageError::new("network", "Offline"));
        assert_eq!(selected(&state).unwrap().id, "b");
        state.settings.tray_mode = TrayMode::Pinned;
        state.settings.pinned_account_id = Some("a".into());
        let pinned = selected(&state).unwrap();
        assert_eq!(pinned.id, "a");
        assert!(stale(pinned, &state));
    }

    #[test]
    fn ring_is_rgba_with_transparent_corners_and_distinct_unknown_state() {
        let full = ring_rgba(Some(100.0), false, true);
        let empty = ring_rgba(Some(0.0), false, true);
        let unknown = ring_rgba(None, false, true);
        assert_eq!(full.len(), 44 * 44 * 4);
        assert_eq!(full[3], 0);
        assert_ne!(unknown, empty);
        assert_ne!(full, empty);
    }
}
