//! Isothrum desktop: the editor from isothrum.com in a window, plus what a browser can't do:
//! running a lab on this machine and capturing its traffic. The editor finds these commands on
//! `window.__TAURI__` and shows Run, Stop and Capture only here.

mod runner;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

/// The editor's address; `ISOTHRUM_URL` points the window at a local build while developing.
const EDITOR_URL: &str = "https://www.isothrum.com/en";

/// Captures in progress, by `<lab>/<network>`.
#[derive(Default)]
struct Captures(Mutex<HashMap<String, runner::Capture>>);

/// Lab names come from the editor and become folder names: kebab-case only, nothing else.
fn checked(name: &str) -> Result<&str, String> {
    let ok = !name.is_empty() && name.len() <= 63 && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok { Ok(name) } else { Err(format!("`{name}` isn't a lab name (lowercase letters, digits and dashes)")) }
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
    Ok(DesktopInfo { version: env!("CARGO_PKG_VERSION"), docker, wireshark: runner::has("wireshark"), labs })
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
async fn lab_down(app: AppHandle, captures: State<'_, Captures>, name: String) -> Result<String, String> {
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
async fn capture_start(app: AppHandle, captures: State<'_, Captures>, name: String, network: String) -> Result<CaptureStarted, String> {
    let name = checked(&name)?.to_string();
    let data = data_dir(&app)?;
    let dir = runner::lab_dir(&data, &name);
    let key = format!("{name}/{network}");
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let file = data.join("captures").join(format!("{name}-{network}-{stamp}.pcap"));
    let capture = tauri::async_runtime::spawn_blocking(move || -> Result<runner::Capture, runner::RunError> {
        let yaml = std::fs::read_to_string(dir.join("isoloom.yml")).map_err(|_| runner::RunError::Spec("start the lab first".into()))?;
        let spec = isoloom_core::parse(&yaml).map_err(|e| runner::RunError::Spec(e.to_string()))?;
        let plan = runner::plan_capture(&dir, &spec, &network)?;
        runner::start_capture(&plan, &file)
    })
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
fn capture_stop(captures: State<'_, Captures>, name: String, network: String) -> Result<(), String> {
    let key = format!("{}/{network}", checked(&name)?);
    if let Some(mut c) = captures.0.lock().map_err(|e| e.to_string())?.remove(&key) {
        c.stop();
    }
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .manage(Captures::default())
        .invoke_handler(tauri::generate_handler![desktop_info, lab_up, lab_down, lab_status, capture_start, capture_stop])
        .setup(|app| {
            let url = std::env::var("ISOTHRUM_URL").unwrap_or_else(|_| EDITOR_URL.to_string());
            let url = url.parse().map_err(|e| format!("ISOTHRUM_URL: {e}"))?;
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
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
