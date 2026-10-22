use super::{
    model::{Account, UsageWindow},
    poller,
    settings::{TrayMetric, TrayMode},
};
use crate::runtime::{AppState, Runtime};
use chrono::Utc;
use serde::Serialize;
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
#[cfg(not(target_os = "macos"))]
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
    pub placement: Mutex<PopoverPlacement>,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PopoverPlacement {
    pub revision: u64,
    pub arrow_x: Option<f64>,
}

#[derive(Clone, Copy)]
struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

struct Layout {
    frame: Bounds,
    arrow_x: Option<f64>,
}

fn layout(work: Bounds, anchor: Option<Bounds>, scale: f64, macos: bool) -> Layout {
    let anchor = anchor.filter(|rect| rect.width > 0.0 && rect.height > 0.0);
    let width = (420.0 * scale).round().min(work.width);
    let height = (660.0 * scale).round().min(work.height);
    let (x, y) = match anchor {
        Some(tray) => {
            let above = tray.y - height;
            let y = if macos || above < work.y {
                tray.y + tray.height
            } else {
                above
            };
            (tray.x + tray.width / 2.0 - width / 2.0, y)
        }
        None => (
            work.x + (work.width - width) / 2.0,
            work.y + (work.height - height) / 2.0,
        ),
    };
    let x = x.clamp(work.x, work.x + work.width - width).round();
    let y = y.clamp(work.y, work.y + work.height - height).round();
    let arrow_x = anchor.filter(|_| macos).and_then(|tray| {
        let offset = (tray.x + tray.width / 2.0 - x) / scale;
        (offset >= 26.0 && offset <= width / scale - 26.0).then_some(offset)
    });
    Layout {
        frame: Bounds {
            x,
            y,
            width,
            height,
        },
        arrow_x,
    }
}

#[cfg(any(target_os = "macos", test))]
fn cocoa_layout(work: Bounds, anchor: Option<Bounds>) -> Layout {
    let flip = |rect: Bounds| Bounds {
        y: -rect.y - rect.height,
        ..rect
    };
    let placed = layout(flip(work), anchor.map(flip), 1.0, true);
    Layout {
        frame: flip(placed.frame),
        arrow_x: placed.arrow_x,
    }
}

#[cfg(target_os = "macos")]
fn place_macos_window(app: &AppHandle, window: &tauri::WebviewWindow) -> Option<f64> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSScreen, NSWindow};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let mtm = MainThreadMarker::new()?;
    let bounds = |rect: NSRect| Bounds {
        x: rect.origin.x,
        y: rect.origin.y,
        width: rect.size.width,
        height: rect.size.height,
    };
    let pointer = window.ns_window().ok()?;
    // Keep AppKit geometry in points on the main thread, including mixed-DPI moves.
    let native = unsafe { &*pointer.cast::<NSWindow>() };
    let tray_geometry = app.tray_by_id("margin").and_then(|tray| {
        tray.with_inner_tray_icon(move |tray| {
            let mtm = MainThreadMarker::new()?;
            let tray_window = tray.ns_status_item()?.button(mtm)?.window()?;
            let screen = tray_window.screen()?;
            Some((bounds(screen.visibleFrame()), bounds(tray_window.frame())))
        })
        .ok()
        .flatten()
    });
    let (work, anchor) = match tray_geometry {
        Some((work, anchor)) => (work, Some(anchor)),
        None => {
            let screen = native.screen().or_else(|| NSScreen::mainScreen(mtm))?;
            (bounds(screen.visibleFrame()), None)
        }
    };
    let placed = cocoa_layout(work, anchor);
    let content_rect = NSRect::new(
        NSPoint::new(placed.frame.x, placed.frame.y),
        NSSize::new(placed.frame.width, placed.frame.height),
    );
    native.setFrame_display(native.frameRectForContentRect(content_rect), true);
    placed.arrow_x
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowKind {
    Session,
    Weekly,
    Other,
}

fn window_kind(window: &UsageWindow) -> WindowKind {
    match window.id.as_str() {
        "five_hour" | "primary_window" => WindowKind::Session,
        "seven_day" | "secondary_window" => WindowKind::Weekly,
        _ => WindowKind::Other,
    }
}

fn expired(window: &UsageWindow, now: chrono::DateTime<Utc>) -> bool {
    window.resets_at.as_ref().is_some_and(|reset| {
        chrono::DateTime::parse_from_rfc3339(reset).is_ok_and(|reset| reset <= now)
    })
}

fn counts(window: &UsageWindow, metric: TrayMetric, now: chrono::DateTime<Utc>) -> bool {
    let kind = window_kind(window);
    match metric {
        TrayMetric::All => true,
        TrayMetric::Core => kind != WindowKind::Other && !expired(window, now),
        TrayMetric::Session => kind == WindowKind::Session && !expired(window, now),
        TrayMetric::Weekly => kind == WindowKind::Weekly && !expired(window, now),
    }
}

fn limiting_window(
    account: &Account,
    metric: TrayMetric,
    now: chrono::DateTime<Utc>,
) -> Option<&UsageWindow> {
    let windows = &account.snapshot.as_ref()?.windows;
    let most_used = |metric| {
        windows
            .iter()
            .filter(|window| window.used_percent.is_finite() && counts(window, metric, now))
            .max_by(|left, right| left.used_percent.total_cmp(&right.used_percent))
    };
    most_used(metric).or_else(|| most_used(TrayMetric::All))
}

fn remaining(account: &Account, metric: TrayMetric) -> Option<f64> {
    limiting_window(account, metric, Utc::now()).map(|window| 100.0 - window.used_percent)
}

fn stale(account: &Account, state: &AppState) -> bool {
    if account.error.as_ref().is_some_and(|error| {
        account.snapshot.is_none()
            || !matches!(error.kind.as_str(), "rateLimited" | "credentialsChanged")
    }) {
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
        .filter_map(|account| {
            remaining(account, state.settings.tray_metric).map(|value| (account, value))
        })
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
    let window = account
        .and_then(|account| limiting_window(account, state.settings.tray_metric, Utc::now()));
    let amount = window.map(|window| 100.0 - window.used_percent);
    let is_stale = account.is_some_and(|account| stale(account, state));
    let pixels = ring_rgba(amount, is_stale, cfg!(target_os = "macos"));
    let tooltip = match (account, amount) {
        (Some(account), Some(amount)) => {
            let window = window
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

#[cfg(target_os = "macos")]
fn fit_backdrop(window: &tauri::WebviewWindow) {
    use objc2::ClassType;
    use objc2_app_kit::{NSAutoresizingMaskOptions, NSVisualEffectView, NSWindow};
    use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize};

    let window = window.clone();
    let handle = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(pointer) = handle.ns_window() else {
            return;
        };
        // Tauri owns this NSWindow; access stays on its main thread while the handle lives.
        let native = unsafe { &*pointer.cast::<NSWindow>() };
        let Some(content) = native.contentView() else {
            return;
        };
        let bounds = content.bounds();
        let frame = NSRect::new(
            NSPoint::new(6.0, if content.isFlipped() { 14.0 } else { 6.0 }),
            NSSize::new(
                (bounds.size.width - 12.0).max(0.0),
                (bounds.size.height - 20.0).max(0.0),
            ),
        );
        for view in content.subviews() {
            if view.isKindOfClass(NSVisualEffectView::class()) {
                view.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewWidthSizable
                        | NSAutoresizingMaskOptions::ViewHeightSizable,
                );
                view.setFrame(frame);
            }
        }
        native.invalidateShadow();
    });
}

pub fn show(app: &AppHandle, settings: bool) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || show_on_main_thread(&handle, settings));
}

fn show_on_main_thread(app: &AppHandle, settings: bool) {
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "macos")]
        let arrow_x = place_macos_window(app, &window);
        #[cfg(not(target_os = "macos"))]
        let arrow_x = {
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
                })
                .filter(|(_, size)| size.width > 0.0 && size.height > 0.0);
            let monitor = anchor
                .and_then(|(position, _)| {
                    window
                        .monitor_from_point(position.x, position.y)
                        .ok()
                        .flatten()
                })
                .or_else(|| window.current_monitor().ok().flatten());
            let mut arrow_x = None;
            if let Some(monitor) = monitor {
                let work = monitor.work_area();
                let placed = layout(
                    Bounds {
                        x: f64::from(work.position.x),
                        y: f64::from(work.position.y),
                        width: f64::from(work.size.width),
                        height: f64::from(work.size.height),
                    },
                    anchor.map(|(position, size)| Bounds {
                        x: position.x,
                        y: position.y,
                        width: size.width,
                        height: size.height,
                    }),
                    monitor.scale_factor(),
                    cfg!(target_os = "macos"),
                );
                let resized = window.set_size(tauri::PhysicalSize::new(
                    placed.frame.width as u32,
                    placed.frame.height as u32,
                ));
                let positioned = window.set_position(tauri::PhysicalPosition::new(
                    placed.frame.x as i32,
                    placed.frame.y as i32,
                ));
                if resized.is_ok() && positioned.is_ok() {
                    arrow_x = placed.arrow_x;
                }
            }
            arrow_x
        };
        if let Ok(mut placement) = app.state::<Visibility>().placement.lock() {
            placement.revision = placement.revision.saturating_add(1);
            placement.arrow_x = arrow_x;
            let _ = window.emit("popover_placement_changed", placement.clone());
        }
        #[cfg(target_os = "macos")]
        fit_backdrop(&window);
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
            recovery: false,
            recovered: Vec::new(),
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
    fn transient_errors_keep_fresh_readings_live() {
        let mut state = AppState {
            revision: 0,
            accounts: vec![account("a", &[10.0])],
            settings: Settings::default(),
            refreshing: false,
            notice: None,
            recovery: false,
            recovered: Vec::new(),
        };
        for kind in ["rateLimited", "credentialsChanged"] {
            state.accounts[0].error = Some(UsageError::new(kind, "Retrying"));
            assert!(!stale(&state.accounts[0], &state));
            if let Some(snapshot) = state.accounts[0].snapshot.as_mut() {
                snapshot.fetched_at = (Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
            }
            assert!(stale(&state.accounts[0], &state));
            state.accounts[0].snapshot = None;
            assert!(stale(&state.accounts[0], &state));
            state.accounts[0] = account("a", &[10.0]);
        }
    }

    fn windowed(id: &str, windows: &[(&str, f64, Option<i64>)]) -> Account {
        let mut account = account(id, &[]);
        if let Some(snapshot) = account.snapshot.as_mut() {
            snapshot.windows = windows
                .iter()
                .map(|(id, used, reset_hours)| UsageWindow {
                    id: (*id).into(),
                    label: (*id).into(),
                    used_percent: *used,
                    resets_at: reset_hours
                        .map(|hours| (Utc::now() + chrono::Duration::hours(hours)).to_rfc3339()),
                    window_seconds: None,
                })
                .collect();
        }
        account
    }

    fn limiting_id(account: &Account, metric: TrayMetric) -> Option<&str> {
        limiting_window(account, metric, Utc::now()).map(|window| window.id.as_str())
    }

    #[test]
    fn window_kinds_match_provider_ids() {
        for (id, kind) in [
            ("five_hour", WindowKind::Session),
            ("primary_window", WindowKind::Session),
            ("seven_day", WindowKind::Weekly),
            ("secondary_window", WindowKind::Weekly),
            ("seven_day_opus", WindowKind::Other),
            ("seven_day_sonnet", WindowKind::Other),
            ("seven_day_routines", WindowKind::Other),
            ("seven_day_cowork", WindowKind::Other),
            ("weekly_scoped_0123456789abcdef", WindowKind::Other),
            ("extra_0123456789abcdef_primary_window", WindowKind::Other),
            ("extra_0123456789abcdef_secondary_window", WindowKind::Other),
        ] {
            let account = windowed("a", &[(id, 1.0, None)]);
            let window = &account.snapshot.as_ref().unwrap().windows[0];
            assert_eq!(window_kind(window), kind, "{id}");
        }
    }

    #[test]
    fn core_metric_ignores_model_scoped_and_expired_windows() {
        let claude = windowed(
            "a",
            &[
                ("five_hour", 40.0, Some(-1)),
                ("seven_day", 30.0, Some(48)),
                ("seven_day_opus", 99.0, Some(48)),
                ("weekly_scoped_0123456789abcdef", 97.0, Some(48)),
                ("seven_day_cowork", 95.0, Some(48)),
            ],
        );
        assert_eq!(limiting_id(&claude, TrayMetric::Core), Some("seven_day"));
        assert_eq!(
            limiting_id(&claude, TrayMetric::All),
            Some("seven_day_opus")
        );
        assert_eq!(limiting_id(&claude, TrayMetric::Weekly), Some("seven_day"));
        assert_eq!(
            limiting_id(&claude, TrayMetric::Session),
            Some("seven_day_opus")
        );

        let codex = windowed(
            "b",
            &[
                ("primary_window", 70.0, Some(2)),
                ("secondary_window", 20.0, Some(100)),
                ("extra_0123456789abcdef_primary_window", 90.0, Some(2)),
            ],
        );
        assert_eq!(
            limiting_id(&codex, TrayMetric::Core),
            Some("primary_window")
        );
        assert_eq!(
            limiting_id(&codex, TrayMetric::Session),
            Some("primary_window")
        );
        assert_eq!(
            limiting_id(&codex, TrayMetric::Weekly),
            Some("secondary_window")
        );
        assert_eq!(
            limiting_id(&codex, TrayMetric::All),
            Some("extra_0123456789abcdef_primary_window")
        );
    }

    #[test]
    fn core_metric_falls_back_to_any_window_without_core_windows() {
        let account = windowed(
            "a",
            &[
                ("seven_day_sonnet", 60.0, None),
                ("seven_day_opus", 80.0, None),
            ],
        );
        assert_eq!(
            limiting_id(&account, TrayMetric::Core),
            Some("seven_day_opus")
        );
        assert_eq!(limiting_id(&windowed("b", &[]), TrayMetric::Core), None);
    }

    #[test]
    fn lowest_account_uses_the_selected_metric() {
        let mut state = AppState {
            revision: 0,
            accounts: vec![
                windowed(
                    "a",
                    &[
                        ("five_hour", 20.0, Some(2)),
                        ("seven_day_opus", 99.0, Some(48)),
                    ],
                ),
                windowed(
                    "b",
                    &[("five_hour", 60.0, Some(2)), ("seven_day", 10.0, Some(48))],
                ),
            ],
            settings: Settings {
                tray_mode: TrayMode::Lowest,
                ..Settings::default()
            },
            refreshing: false,
            notice: None,
            recovery: false,
            recovered: Vec::new(),
        };
        assert_eq!(selected(&state).unwrap().id, "b");
        state.settings.tray_metric = TrayMetric::All;
        assert_eq!(selected(&state).unwrap().id, "a");
        state.settings.tray_metric = TrayMetric::Weekly;
        assert_eq!(selected(&state).unwrap().id, "a");
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

    #[test]
    fn cocoa_coordinates_keep_content_size_across_retina_and_external_screens() {
        for (work, tray, expected_x, expected_y) in [
            (
                Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 1440.0,
                    height: 875.0,
                },
                Bounds {
                    x: 1368.0,
                    y: 875.0,
                    width: 24.0,
                    height: 25.0,
                },
                1020.0,
                215.0,
            ),
            (
                Bounds {
                    x: 1440.0,
                    y: -180.0,
                    width: 1920.0,
                    height: 1055.0,
                },
                Bounds {
                    x: 3288.0,
                    y: 875.0,
                    width: 24.0,
                    height: 25.0,
                },
                2940.0,
                215.0,
            ),
            (
                Bounds {
                    x: -1920.0,
                    y: -180.0,
                    width: 1920.0,
                    height: 1055.0,
                },
                Bounds {
                    x: -1872.0,
                    y: 875.0,
                    width: 24.0,
                    height: 25.0,
                },
                -1920.0,
                215.0,
            ),
            (
                Bounds {
                    x: 0.0,
                    y: 900.0,
                    width: 1920.0,
                    height: 1055.0,
                },
                Bounds {
                    x: 1848.0,
                    y: 1955.0,
                    width: 24.0,
                    height: 25.0,
                },
                1500.0,
                1295.0,
            ),
        ] {
            let placed = cocoa_layout(work, Some(tray));
            assert_eq!(placed.frame.width, 420.0);
            assert_eq!(placed.frame.height, 660.0);
            assert_eq!(placed.frame.x, expected_x);
            assert_eq!(placed.frame.y, expected_y);
            assert_eq!(placed.frame.y + placed.frame.height, tray.y);
            assert_eq!(
                placed.frame.x + placed.arrow_x.unwrap(),
                tray.x + tray.width / 2.0
            );
        }
    }

    #[test]
    fn cocoa_small_work_area_and_missing_tray_stay_within_screen() {
        let work = Bounds {
            x: -360.0,
            y: 900.0,
            width: 360.0,
            height: 440.0,
        };
        let placed = cocoa_layout(work, None);
        assert_eq!(placed.frame.x, work.x);
        assert_eq!(placed.frame.y, work.y);
        assert_eq!(placed.frame.width, work.width);
        assert_eq!(placed.frame.height, work.height);
        assert_eq!(placed.arrow_x, None);
    }

    #[test]
    fn retina_arrow_tracks_tray_after_right_edge_clamping() {
        let work = Bounds {
            x: 0.0,
            y: 50.0,
            width: 2880.0,
            height: 1750.0,
        };
        let tray = Bounds {
            x: 2736.0,
            y: 0.0,
            width: 48.0,
            height: 50.0,
        };
        let placed = layout(work, Some(tray), 2.0, true);
        assert_eq!(placed.frame.x, 2040.0);
        assert_eq!(placed.frame.y, 50.0);
        assert_eq!(placed.arrow_x, Some(360.0));
        assert_eq!(placed.frame.x + placed.arrow_x.unwrap() * 2.0, 2760.0);
    }

    #[test]
    fn left_monitor_coordinates_keep_arrow_aligned() {
        let work = Bounds {
            x: -1920.0,
            y: 25.0,
            width: 1920.0,
            height: 1055.0,
        };
        let tray = Bounds {
            x: -1872.0,
            y: 0.0,
            width: 24.0,
            height: 25.0,
        };
        let placed = layout(work, Some(tray), 1.0, true);
        assert_eq!(placed.frame.x, -1920.0);
        assert_eq!(placed.arrow_x, Some(60.0));
    }

    #[test]
    fn unavailable_or_outside_anchor_has_no_misleading_arrow() {
        let work = Bounds {
            x: 0.0,
            y: 25.0,
            width: 1280.0,
            height: 775.0,
        };
        let centered = layout(work, None, 1.0, true);
        assert_eq!(centered.frame.x, 430.0);
        assert_eq!(centered.arrow_x, None);
        let hidden = Bounds {
            x: 0.0,
            y: 800.0,
            width: 34.0,
            height: 0.0,
        };
        let fallback = layout(work, Some(hidden), 1.0, true);
        assert_eq!(fallback.frame.x, centered.frame.x);
        assert_eq!(fallback.frame.y, centered.frame.y);
        assert_eq!(fallback.arrow_x, None);
        let edge = Bounds {
            x: 1260.0,
            y: 0.0,
            width: 20.0,
            height: 25.0,
        };
        assert_eq!(layout(work, Some(edge), 1.0, true).arrow_x, None);
    }

    #[test]
    fn windows_taskbar_placement_respects_each_scale() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let work = Bounds {
                x: -1920.0 * scale,
                y: 0.0,
                width: 1920.0 * scale,
                height: 1040.0 * scale,
            };
            let tray = Bounds {
                x: -80.0 * scale,
                y: 1040.0 * scale,
                width: 24.0 * scale,
                height: 40.0 * scale,
            };
            let placed = layout(work, Some(tray), scale, false);
            assert_eq!(placed.frame.width, 420.0 * scale);
            assert_eq!(placed.frame.height, 660.0 * scale);
            assert_eq!(placed.frame.x + placed.frame.width, 0.0);
            assert_eq!(placed.frame.y + placed.frame.height, tray.y);
            assert_eq!(placed.arrow_x, None);
        }
    }

    #[test]
    fn top_taskbar_places_window_below_and_small_displays_fit() {
        let work = Bounds {
            x: 0.0,
            y: 40.0,
            width: 360.0,
            height: 440.0,
        };
        let tray = Bounds {
            x: 300.0,
            y: 0.0,
            width: 24.0,
            height: 40.0,
        };
        let placed = layout(work, Some(tray), 1.5, false);
        assert_eq!(placed.frame.x, 0.0);
        assert_eq!(placed.frame.y, 40.0);
        assert_eq!(placed.frame.width, 360.0);
        assert_eq!(placed.frame.height, 440.0);
    }
}
