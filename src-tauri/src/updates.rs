use serde::Serialize;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Clone, Debug, Default, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpdatePhase {
    #[default]
    Idle,
    Checking,
    Current,
    Available {
        version: String,
    },
    Downloading {
        version: String,
        downloaded: u64,
        total: Option<u64>,
    },
    Installing {
        version: String,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct UpdateStatus {
    revision: u64,
    state: UpdatePhase,
}

#[derive(Default)]
pub struct Updates {
    status: Mutex<UpdateStatus>,
    pending: tokio::sync::Mutex<Option<Update>>,
}

impl Updates {
    fn status(&self) -> Result<UpdateStatus, String> {
        self.status
            .lock()
            .map(|status| status.clone())
            .map_err(|_| "Update status unavailable. Reopen Margin and try again.".into())
    }

    fn publish(&self, app: &AppHandle, state: UpdatePhase) -> Result<UpdateStatus, String> {
        let mut status = self
            .status
            .lock()
            .map_err(|_| "Update status unavailable. Reopen Margin and try again.")?;
        status.revision = status.revision.saturating_add(1);
        status.state = state;
        let _ = app.emit("update_status_changed", status.clone());
        Ok(status.clone())
    }

    fn fail(&self, app: &AppHandle, message: &str) -> Result<UpdateStatus, String> {
        self.publish(
            app,
            UpdatePhase::Error {
                message: message.into(),
            },
        )
    }
}

fn trusted_download(url: &reqwest::Url, version: &str) -> bool {
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    let prefix = format!("/Abulkosim/usagebar/releases/download/v{version}/");
    let Some(file) = url.path().strip_prefix(&prefix) else {
        return false;
    };
    file == "Margin.app.tar.gz"
        || file == format!("Margin_{version}_x64-setup.exe")
        || file == format!("Margin_{version}_x64_en-US.msi")
}

#[tauri::command]
pub fn get_update_status(updates: State<'_, Updates>) -> Result<UpdateStatus, String> {
    updates.status()
}

#[tauri::command]
pub async fn check_for_update(
    app: AppHandle,
    updates: State<'_, Updates>,
) -> Result<UpdateStatus, String> {
    let Ok(mut pending) = updates.pending.try_lock() else {
        return updates.status();
    };
    *pending = None;
    updates.publish(&app, UpdatePhase::Checking)?;
    let result = match app
        .updater_builder()
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(updater) => updater.check().await,
        Err(error) => Err(error),
    };
    match result {
        Ok(Some(mut update)) if trusted_download(&update.download_url, &update.version) => {
            update.timeout = Some(Duration::from_secs(300));
            let version = update.version.clone();
            *pending = Some(update);
            updates.publish(&app, UpdatePhase::Available { version })
        }
        Ok(None) => updates.publish(&app, UpdatePhase::Current),
        Ok(Some(_)) => updates.fail(&app, "This update has an unexpected download address. Try again after the release is corrected."),
        Err(_) => updates.fail(&app, "Couldn’t check for updates. Check your connection and try again."),
    }
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    updates: State<'_, Updates>,
    version: String,
) -> Result<UpdateStatus, String> {
    let Ok(mut pending) = updates.pending.try_lock() else {
        return updates.status();
    };
    let Some(update) = pending.take().filter(|update| update.version == version) else {
        return updates.fail(&app, "Check for updates again before installing.");
    };
    updates.publish(
        &app,
        UpdatePhase::Downloading {
            version: version.clone(),
            downloaded: 0,
            total: None,
        },
    )?;
    let mut downloaded = 0_u64;
    let mut last_event = Instant::now();
    let bytes = update
        .download(
            |chunk, total| {
                downloaded = downloaded.saturating_add(chunk as u64);
                if last_event.elapsed() >= Duration::from_millis(100) || total == Some(downloaded) {
                    let _ = updates.publish(
                        &app,
                        UpdatePhase::Downloading {
                            version: version.clone(),
                            downloaded,
                            total,
                        },
                    );
                    last_event = Instant::now();
                }
            },
            || {},
        )
        .await;
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(_) => return updates.fail(&app, "Couldn’t download and verify this update. Nothing was installed. Check for updates to try again."),
    };
    updates.publish(&app, UpdatePhase::Installing { version })?;
    match tauri::async_runtime::spawn_blocking(move || update.install(bytes)).await {
        Ok(Ok(())) => app.restart(),
        _ => updates.fail(
            &app,
            "Couldn’t finish installing. Reopen Margin, or install the latest release from GitHub.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloads_are_limited_to_versioned_margin_release_assets() {
        let base = "https://github.com/Abulkosim/usagebar/releases/download/v0.2.0/";
        for file in [
            "Margin.app.tar.gz",
            "Margin_0.2.0_x64-setup.exe",
            "Margin_0.2.0_x64_en-US.msi",
        ] {
            assert!(trusted_download(
                &format!("{base}{file}").parse().unwrap(),
                "0.2.0"
            ));
        }
        for url in [
            format!("{base}Margin_0.1.1_x64-setup.exe"),
            format!("{base}Margin.app.tar.gz?token=anything"),
            format!("{base}Margin.app.tar.gz#fragment"),
            format!("{base}../Margin.app.tar.gz"),
            format!("{base}other.exe"),
            base.replace("https:", "http:") + "Margin.app.tar.gz",
            base.replace("github.com", "github.com.evil.example") + "Margin.app.tar.gz",
            base.replace("github.com", "user:pass@github.com") + "Margin.app.tar.gz",
            base.replace("usagebar", "another-repo") + "Margin.app.tar.gz",
            base.replace("v0.2.0", "v0.1.1") + "Margin.app.tar.gz",
        ] {
            assert!(!trusted_download(&url.parse().unwrap(), "0.2.0"), "{url}");
        }
    }

    #[test]
    fn download_progress_uses_the_frontend_contract() {
        let status = UpdateStatus {
            revision: 3,
            state: UpdatePhase::Downloading {
                version: "0.2.0".into(),
                downloaded: 10,
                total: None,
            },
        };
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            serde_json::json!({"revision": 3, "state": {"kind": "downloading", "version": "0.2.0", "downloaded": 10, "total": null}})
        );
    }

    #[tokio::test]
    async fn one_operation_owns_the_pending_update_at_a_time() {
        let updates = Updates::default();
        let guard = updates.pending.try_lock().unwrap();
        assert!(updates.pending.try_lock().is_err());
        assert!(matches!(updates.status().unwrap().state, UpdatePhase::Idle));
        drop(guard);
        assert!(updates.pending.try_lock().is_ok());
    }
}
