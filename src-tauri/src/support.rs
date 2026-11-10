use crate::{
    core::{autostart, install, report::ProblemReport},
    runtime::Runtime,
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

/// The report last shown to the user. Opening reuses it, so the issue matches
/// the preview and the interface cannot supply a URL or its contents.
#[derive(Default)]
pub struct ReportPreview(Mutex<Option<ProblemReport>>);

#[tauri::command]
pub fn get_install_warning() -> bool {
    let executable = std::env::current_exe().and_then(std::fs::canonicalize).ok();
    let home = dirs::home_dir().and_then(|home| std::fs::canonicalize(home).ok());
    install::warns_about_location(std::env::consts::OS, executable.as_deref(), home.as_deref())
}

/// Points an enabled login item at this copy when it was registered from a copy that
/// has since moved. Errors leave the registration as it was.
pub fn repair_autostart(app: &AppHandle, enabled: bool) {
    let Some(executable) = autostart_executable() else {
        return;
    };
    let registered = registered_program(app);
    let exists = registered
        .as_deref()
        .is_some_and(|program| Path::new(program).exists());
    let home = dirs::home_dir().and_then(|home| std::fs::canonicalize(home).ok());
    if autostart::needs_repair(
        std::env::consts::OS,
        enabled,
        &executable,
        registered.as_deref(),
        exists,
        home.as_deref(),
    ) {
        let _ = app.autolaunch().enable();
    }
}

fn autostart_executable() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    if cfg!(windows) {
        Some(executable)
    } else {
        std::fs::canonicalize(executable).ok()
    }
}

#[cfg(target_os = "macos")]
fn registered_program(app: &AppHandle) -> Option<String> {
    let plist = dirs::home_dir()?
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", app.package_info().name));
    let text = std::fs::read_to_string(plist).ok()?;
    autostart::launch_agent_program(&text).map(str::to_owned)
}

#[cfg(windows)]
fn registered_program(app: &AppHandle) -> Option<String> {
    let name = &app.package_info().name;
    [
        windows_registry::CURRENT_USER,
        windows_registry::LOCAL_MACHINE,
    ]
    .into_iter()
    .find_map(|root| {
        let value = root
            .open(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run")
            .and_then(|key| key.get_string(name))
            .ok()?;
        autostart::run_value_program(&value).map(str::to_owned)
    })
}

#[cfg(not(any(target_os = "macos", windows)))]
fn registered_program(_: &AppHandle) -> Option<String> {
    None
}

#[tauri::command]
pub async fn get_problem_report(
    runtime: State<'_, Arc<Runtime>>,
    preview: State<'_, ReportPreview>,
) -> Result<ProblemReport, String> {
    let accounts = runtime.inner.lock().await.view.accounts.clone();
    let os_version = os_info::get().version().to_string();
    let report = ProblemReport::build(std::env::consts::OS, &os_version, &accounts);
    *preview
        .0
        .lock()
        .map_err(|_| "Could not prepare the report.")? = Some(report.clone());
    Ok(report)
}

#[tauri::command]
pub fn open_problem_report(
    app: AppHandle,
    preview: State<'_, ReportPreview>,
) -> Result<(), String> {
    let report = preview
        .0
        .lock()
        .map_err(|_| "Could not prepare the report.")?
        .clone()
        .ok_or("Review the report first.")?;
    app.opener()
        .open_url(report.url()?, None::<&str>)
        .map_err(|_| "Could not open your browser.".into())
}
