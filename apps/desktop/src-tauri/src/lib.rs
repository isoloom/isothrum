//! Isothrum desktop: the editor from isothrum.com in a window, plus what a browser can't do:
//! running a lab on this machine and capturing its traffic. The editor finds these commands on
//! `window.__TAURI__` and shows Run, Stop and Capture only here.

mod runner;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

// The window shows the editor bundled with the app (`apps/desktop/editor`, built from the website
// by `npm run editor`): it works offline. `ISOTHRUM_URL` points it at a site instead (isothrum.com
// for accounts and saved projects, or a local build while developing).

/// Captures in progress, by `<lab>/<network>`.
#[derive(Default)]
struct Captures(Mutex<HashMap<String, runner::Capture>>);

/// Lab names come from the editor and become folder names: kebab-case only, nothing else.
fn checked(name: &str) -> Result<&str, String> {
    let ok = !name.is_empty()
        && name.len() <= 63
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(name)
    } else {
        Err(format!(
            "`{name}` isn't a lab name (lowercase letters, digits and dashes)"
        ))
    }
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct DesktopInfo {
    version: &'static str,
    /// Whether `docker` answers (installed, running, and this user may use it).
    docker: bool,
    wireshark: bool,
    /// Wireshark can show captures live (its `dumpcap` is usable by this account).
    live_capture: bool,
    labs: String,
}

#[tauri::command]
async fn desktop_info(app: AppHandle) -> Result<DesktopInfo, String> {
    let labs = data_dir(&app)?.join("labs").display().to_string();
    let docker = tauri::async_runtime::spawn_blocking(|| {
        std::process::Command::new("docker")
            .args(["info", "--format", "{{.ServerVersion}}"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false);
    Ok(DesktopInfo {
        version: env!("CARGO_PKG_VERSION"),
        docker,
        wireshark: runner::has("wireshark"),
        live_capture: runner::live_capture_possible(),
        labs,
    })
}

/// Writes the lab (its isoloom.yml and Docker files) and starts it.
#[tauri::command]
async fn lab_up(app: AppHandle, name: String, yaml: String) -> Result<String, String> {
    let dir = runner::lab_dir(&data_dir(&app)?, checked(&name)?);
    tauri::async_runtime::spawn_blocking(move || {
        runner::prepare(&dir, &yaml)?;
        runner::up(&dir)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn lab_down(
    app: AppHandle,
    captures: State<'_, Captures>,
    name: String,
) -> Result<String, String> {
    let name = checked(&name)?.to_string();
    // Stopping the lab ends its captures too.
    if let Ok(mut all) = captures.0.lock() {
        let prefix = format!("{name}/");
        for (_, mut c) in all.extract_if(|k, _| k.starts_with(&prefix)) {
            c.stop();
        }
    }
    let dir = runner::lab_dir(&data_dir(&app)?, &name);
    tauri::async_runtime::spawn_blocking(move || runner::down(&dir))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn lab_status(app: AppHandle, name: String) -> Result<String, String> {
    let dir = runner::lab_dir(&data_dir(&app)?, checked(&name)?);
    if !dir.join(".isoloom/docker/compose.yml").is_file() {
        return Ok(String::new());
    }
    tauri::async_runtime::spawn_blocking(move || runner::status(&dir))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct CaptureStarted {
    /// Where the packets are written when Wireshark isn't installed; `None`: Wireshark is open.
    file: Option<String>,
}

/// Captures a LAN's or a cable's traffic (`network` is its name in isoloom.yml).
#[tauri::command]
async fn capture_start(
    app: AppHandle,
    captures: State<'_, Captures>,
    name: String,
    network: String,
) -> Result<CaptureStarted, String> {
    let name = checked(&name)?.to_string();
    let data = data_dir(&app)?;
    let dir = runner::lab_dir(&data, &name);
    let key = format!("{name}/{network}");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file = data
        .join("captures")
        .join(format!("{name}-{network}-{stamp}.pcap"));
    let capture = tauri::async_runtime::spawn_blocking(
        move || -> Result<runner::Capture, runner::RunError> {
            let yaml = std::fs::read_to_string(dir.join("isoloom.yml"))
                .map_err(|_| runner::RunError::Spec("start the lab first".into()))?;
            let spec =
                isoloom_core::parse(&yaml).map_err(|e| runner::RunError::Spec(e.to_string()))?;
            let plan = runner::plan_capture(&dir, &spec, &network)?;
            runner::start_capture(&plan, &file)
        },
    )
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    let file = capture.file.as_ref().map(|f| f.display().to_string());
    let mut all = captures.0.lock().map_err(|e| e.to_string())?;
    if let Some(mut old) = all.insert(key, capture) {
        old.stop();
    }
    Ok(CaptureStarted { file })
}

#[tauri::command]
fn capture_stop(
    captures: State<'_, Captures>,
    name: String,
    network: String,
) -> Result<(), String> {
    let key = format!("{}/{network}", checked(&name)?);
    if let Some(mut c) = captures.0.lock().map_err(|e| e.to_string())?.remove(&key) {
        c.stop();
    }
    Ok(())
}

/// The lab's spec as last started (its folder's isoloom.yml).
fn lab_spec(dir: &std::path::Path) -> Result<isoloom_core::Spec, runner::RunError> {
    let yaml = std::fs::read_to_string(dir.join("isoloom.yml"))
        .map_err(|_| runner::RunError::Spec("start the lab first".into()))?;
    isoloom_core::parse(&yaml).map_err(|e| runner::RunError::Spec(e.to_string()))
}

/// Opens a terminal on one machine of the running lab (a shell, or SSH to a network appliance).
#[tauri::command]
async fn machine_console(app: AppHandle, name: String, machine: String) -> Result<(), String> {
    let name = checked(&name)?.to_string();
    let dir = runner::lab_dir(&data_dir(&app)?, &name);
    tauri::async_runtime::spawn_blocking(move || {
        let spec = lab_spec(&dir)?;
        let cmd = runner::console_command(&dir, &spec, &machine)?;
        runner::open_terminal(&format!("{machine} ({name})"), &cmd)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Starts, stops or restarts one machine of the running lab.
#[tauri::command]
async fn machine_action(
    app: AppHandle,
    name: String,
    machine: String,
    action: String,
) -> Result<String, String> {
    let dir = runner::lab_dir(&data_dir(&app)?, checked(&name)?);
    tauri::async_runtime::spawn_blocking(move || {
        let spec = lab_spec(&dir)?;
        runner::machine_action(&dir, &spec, &machine, &action)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// The lab's snapshots.
#[tauri::command]
fn snapshot_list(app: AppHandle, name: String) -> Result<Vec<runner::Snapshot>, String> {
    Ok(runner::snapshots(&runner::lab_dir(
        &data_dir(&app)?,
        checked(&name)?,
    )))
}

/// Saves one machine of the running lab.
#[tauri::command]
async fn snapshot_take(
    app: AppHandle,
    name: String,
    machine: String,
    label: String,
) -> Result<runner::Snapshot, String> {
    let name = checked(&name)?.to_string();
    let dir = runner::lab_dir(&data_dir(&app)?, &name);
    tauri::async_runtime::spawn_blocking(move || {
        runner::take_snapshot(&dir, &name, &machine, &label)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Runs a machine from one of its snapshots (`id`), or from its own image again (no `id`).
#[tauri::command]
async fn snapshot_use(
    app: AppHandle,
    name: String,
    machine: String,
    id: Option<String>,
) -> Result<String, String> {
    let dir = runner::lab_dir(&data_dir(&app)?, checked(&name)?);
    tauri::async_runtime::spawn_blocking(move || {
        runner::use_snapshot(&dir, &machine, id.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn snapshot_delete(app: AppHandle, name: String, id: String) -> Result<(), String> {
    let dir = runner::lab_dir(&data_dir(&app)?, checked(&name)?);
    tauri::async_runtime::spawn_blocking(move || runner::delete_snapshot(&dir, &id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// A newer release, if there is one (the updater checks GitHub's latest release, signed with
/// the app's key).
#[derive(Serialize)]
struct UpdateInfo {
    version: String,
    notes: Option<String>,
}

#[tauri::command]
async fn update_check(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    Ok(update.map(|u| UpdateInfo {
        version: u.version,
        notes: u.body,
    }))
}

/// Downloads and installs the newer release, then restarts the app on it.
#[tauri::command]
async fn update_install(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let Some(update) = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Captures::default())
        .invoke_handler(tauri::generate_handler![
            desktop_info,
            lab_up,
            lab_down,
            lab_status,
            capture_start,
            capture_stop,
            machine_console,
            machine_action,
            snapshot_list,
            snapshot_take,
            snapshot_use,
            snapshot_delete,
            update_check,
            update_install
        ])
        .setup(|app| {
            let page = match std::env::var("ISOTHRUM_URL") {
                Ok(url) => {
                    WebviewUrl::External(url.parse().map_err(|e| format!("ISOTHRUM_URL: {e}"))?)
                }
                Err(_) => WebviewUrl::App("index.html".into()),
            };
            WebviewWindowBuilder::new(app, "main", page)
                .title("Isothrum")
                .inner_size(1440.0, 900.0)
                .min_inner_size(900.0, 600.0)
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Isothrum desktop failed to start")
        .run(|app, event| {
            // Leaving the app ends every capture it started.
            if let tauri::RunEvent::Exit = event {
                if let Some(c) = app.try_state::<Captures>() {
                    if let Ok(mut all) = c.0.lock() {
                        for (_, mut cap) in all.drain() {
                            cap.stop();
                        }
                    }
                }
            }
        });
}
