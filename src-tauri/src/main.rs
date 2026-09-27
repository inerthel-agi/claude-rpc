#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod alerts;
mod config;
mod daemon;
mod launch;

use config::ClaudeConfig;
use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::UpdaterExt;

#[derive(Default)]
struct DaemonState {
    running: Arc<Mutex<bool>>,
    stop: Arc<AtomicBool>,
    force_refresh: Arc<AtomicBool>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

#[derive(Default)]
struct UpdateState {
    available: Mutex<Option<UpdateInfo>>,
    // Last failed install started from the tray, shown on the Updates item.
    last_error: Mutex<Option<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateInfo {
    version: String,
    notes: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrayInfo {
    dnd: bool,
    start_on_windows: bool,
    rpc_mode: String,
    update_version: Option<String>,
    update_error: Option<String>,
    app_version: String,
    pause_until_ms: u64,
    language: String,
    // Whether the tray's "Claude Desktop" / "Claude Code" items open the app or
    // its download / install page.
    desktop_installed: bool,
    code_installed: bool,
}

#[cfg(windows)]
const STARTUP_REG_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const STARTUP_REG_VALUE: &str = "Claude RPC";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeStatus {
    claude_line: String,
    model_line: String,
    limits_line: Option<String>,
    limits: Value,
    // Why nothing is published ("private", "chat"), sessions count, and the
    // 5-hour usage history for the tray chart.
    hidden_reason: Option<String>,
    sessions: u64,
    // A terminal Claude Code CLI runs beside Claude Desktop.
    cli_alongside: bool,
    history_5h: Value,
    provider_line: String,
    discord_line: String,
    preview_header: Option<String>,
    preview_primary: Option<String>,
    preview_secondary: Option<String>,
    preview_tertiary: Option<String>,
    started_at_ms: Option<u64>,
    // Extra usage spend: { enabled, used, limit, currency } or null.
    credits: Value,
}

#[tauri::command]
fn load_config() -> Result<ClaudeConfig, String> {
    read_config()
}

#[tauri::command]
fn save_config(config: ClaudeConfig) -> Result<(), String> {
    let config = config::normalize_config(config);
    write_config(&config)?;
    Ok(())
}

#[tauri::command]
fn load_status() -> Result<ClaudeStatus, String> {
    let value = fs::read_to_string(status_path()?)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(raw.trim_start_matches('\u{feff}')).ok())
        .unwrap_or(Value::Null);

    Ok(ClaudeStatus {
        claude_line: value
            .get("claudeLine")
            .and_then(Value::as_str)
            .unwrap_or("Claude: Off")
            .to_string(),
        model_line: value
            .get("modelLine")
            .and_then(Value::as_str)
            .unwrap_or("Auto-detect")
            .to_string(),
        limits_line: value
            .get("limitsLine")
            .and_then(Value::as_str)
            .map(str::to_string),
        limits: value
            .get("limits")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
        hidden_reason: value
            .get("hiddenReason")
            .and_then(Value::as_str)
            .map(str::to_string),
        sessions: value.get("sessions").and_then(Value::as_u64).unwrap_or(0),
        cli_alongside: value
            .get("cliAlongside")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        history_5h: value
            .get("history5h")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
        provider_line: value
            .get("providerLine")
            .and_then(Value::as_str)
            .unwrap_or("Provider: Unknown")
            .to_string(),
        discord_line: value
            .get("discordLine")
            .and_then(Value::as_str)
            .unwrap_or("Discord: RPC disabled")
            .to_string(),
        preview_header: value
            .get("previewHeader")
            .and_then(Value::as_str)
            .map(str::to_string),
        preview_primary: value
            .get("previewPrimary")
            .and_then(Value::as_str)
            .map(str::to_string),
        preview_secondary: value
            .get("previewSecondary")
            .and_then(Value::as_str)
            .map(str::to_string),
        preview_tertiary: value
            .get("previewTertiary")
            .and_then(Value::as_str)
            .map(str::to_string),
        started_at_ms: value.get("startedAtMs").and_then(Value::as_u64),
        credits: value.get("credits").cloned().unwrap_or(Value::Null),
    })
}

// Settings call this on open: it restarts the daemon thread if it has exited.
#[tauri::command]
fn start_daemon(state: tauri::State<'_, DaemonState>) {
    start_daemon_inner(&state);
}

#[tauri::command]
fn close_settings(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|err| err.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn refresh_limits(state: tauri::State<'_, DaemonState>) -> Result<(), String> {
    state.force_refresh.store(true, Ordering::SeqCst);
    Ok(())
}

async fn fetch_update(app: &tauri::AppHandle) -> Result<Option<UpdateInfo>, String> {
    let updater = app.updater().map_err(|err| err.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(Some(UpdateInfo {
            version: update.version.clone(),
            notes: update.body.clone().unwrap_or_default(),
        })),
        Ok(None) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

static INSTALLING: AtomicBool = AtomicBool::new(false);

async fn download_and_install(app: &tauri::AppHandle) -> Result<(), String> {
    // The tray and the settings window can both start an install.
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err("An update is already being installed".into());
    }
    let result = install_latest(app).await;
    INSTALLING.store(false, Ordering::SeqCst);
    result
}

async fn install_latest(app: &tauri::AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|err| err.to_string())?;
    let Some(update) = updater.check().await.map_err(|err| err.to_string())? else {
        // The release was pulled or is already installed: drop the stale offer.
        *app.state::<UpdateState>()
            .available
            .lock()
            .expect("update state mutex poisoned") = None;
        return Ok(());
    };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|err| err.to_string())?;
    app.restart();
}

#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> Result<Option<UpdateInfo>, String> {
    let info = fetch_update(&app).await?;
    *app.state::<UpdateState>()
        .available
        .lock()
        .expect("update state mutex poisoned") = info.clone();
    Ok(info)
}

#[tauri::command]
fn pending_update(state: tauri::State<'_, UpdateState>) -> Option<UpdateInfo> {
    state
        .available
        .lock()
        .expect("update state mutex poisoned")
        .clone()
}

#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    download_and_install(&app).await
}

fn read_tray_info(app: &tauri::AppHandle) -> TrayInfo {
    let config = read_config().unwrap_or_default();
    let update_version = app
        .state::<UpdateState>()
        .available
        .lock()
        .expect("update state mutex poisoned")
        .as_ref()
        .map(|info| info.version.clone());
    let update_error = app
        .state::<UpdateState>()
        .last_error
        .lock()
        .expect("update state mutex poisoned")
        .clone();
    TrayInfo {
        dnd: config.dnd,
        start_on_windows: is_start_on_windows_enabled(),
        rpc_mode: config.rpc_mode,
        update_version,
        update_error,
        app_version: app.package_info().version.to_string(),
        pause_until_ms: config.pause_until_ms,
        language: config.language,
        desktop_installed: launch::find_desktop().is_some(),
        code_installed: launch::find_code().is_some(),
    }
}

#[tauri::command]
async fn diagnostic(app: tauri::AppHandle) -> Result<String, String> {
    let version = app.package_info().version.to_string();
    let report = tauri::async_runtime::spawn_blocking(daemon::diagnostic_report)
        .await
        .map_err(|err| err.to_string())?;
    Ok(format!("Claude RPC v{version}\n{report}"))
}

// Async so reg.exe and the install checks never run on the UI thread (Tauri
// runs sync commands there); the tray calls this every time it opens.
#[tauri::command]
async fn tray_state(app: tauri::AppHandle) -> Result<TrayInfo, String> {
    tauri::async_runtime::spawn_blocking(move || read_tray_info(&app))
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
async fn tray_action(app: tauri::AppHandle, action: String) -> Result<TrayInfo, String> {
    let hide_tray = || {
        if let Some(window) = app.get_webview_window("tray") {
            let _ = window.hide();
        }
    };
    match action.as_str() {
        "close" => hide_tray(),
        "settings" => {
            hide_tray();
            show_settings(&app);
        }
        // "Always" in the tray pause row: permanent DND replaces a timed pause.
        "dnd" => {
            update_config(|config| {
                config.dnd = !config.dnd;
                config.pause_until_ms = 0;
            })?;
        }
        "startup" => {
            set_start_on_windows(!is_start_on_windows_enabled())?;
        }
        "resume" => {
            update_config(|config| {
                config.pause_until_ms = 0;
                config.dnd = false;
            })?;
        }
        // "pause:<unix ms>": tray.js computes the end time in local time
        // (30 min, 1 hour, next midnight); capped at 48 hours.
        action if action.starts_with("pause:") => {
            let until = action["pause:".len()..]
                .parse::<u64>()
                .map_err(|_| format!("invalid pause: {action}"))?;
            let now = daemon::now_ms();
            let until = until.clamp(now, now + 48 * 60 * 60 * 1000);
            update_config(|config| {
                config.pause_until_ms = until;
                config.dnd = false;
            })?;
        }
        "open_desktop" => {
            hide_tray();
            launch::open_desktop()?;
        }
        "open_code" => {
            hide_tray();
            launch::open_code()?;
        }
        "mode_playing" => set_mode("playing")?,
        "mode_watching" => set_mode("watching")?,
        "mode_listening" => set_mode("listening")?,
        "mode_competing" => set_mode("competing")?,
        "update" => {
            *app.state::<UpdateState>()
                .last_error
                .lock()
                .expect("update state mutex poisoned") = None;
            let pending = app
                .state::<UpdateState>()
                .available
                .lock()
                .expect("update state mutex poisoned")
                .is_some();
            if pending {
                hide_tray();
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(err) = download_and_install(&handle).await {
                        *handle
                            .state::<UpdateState>()
                            .last_error
                            .lock()
                            .expect("update state mutex poisoned") = Some(err);
                    }
                });
            } else {
                let info = fetch_update(&app).await?;
                *app.state::<UpdateState>()
                    .available
                    .lock()
                    .expect("update state mutex poisoned") = info;
            }
        }
        "quit" => {
            hide_tray();
            // The daemon clears the Discord activity when it stops; wait for
            // that, but never more than 2 s (an OAuth or IPC call can be slow).
            let state = app.state::<DaemonState>();
            let handle = request_daemon_stop(&state);
            tauri::async_runtime::spawn_blocking(move || join_with_timeout(handle, 2_000))
                .await
                .ok();
            app.exit(0);
        }
        other => return Err(format!("unknown tray action: {other}")),
    }
    tauri::async_runtime::spawn_blocking(move || read_tray_info(&app))
        .await
        .map_err(|err| err.to_string())
}

// About page links; only the names listed in launch::link_url are accepted.
#[tauri::command]
fn open_link(name: String) -> Result<(), String> {
    let url = launch::link_url(&name).ok_or_else(|| format!("unknown link: {name}"))?;
    launch::open_url(url)
}

fn main() {
    tauri::Builder::default()
        // Must be the first plugin: a second launch (autostart + manual, double
        // click, installer post-run) is rejected and instead focuses/opens the
        // existing instance's settings — so only one daemon ever drives Discord.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_settings(app);
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .manage(DaemonState::default())
        .manage(UpdateState::default())
        .invoke_handler(tauri::generate_handler![
            load_config,
            save_config,
            load_status,
            start_daemon,
            close_settings,
            refresh_limits,
            check_update,
            pending_update,
            install_update,
            tray_state,
            tray_action,
            tray_fit,
            open_link,
            diagnostic
        ])
        .setup(|app| {
            let state = app.state::<DaemonState>();
            start_daemon_inner(&state);
            create_tray(app)?;
            spawn_update_check(app.handle().clone());
            spawn_status_watcher(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Claude RPC tray");
}

fn spawn_update_check(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let Ok(Some(info)) = fetch_update(&app).await else {
            return;
        };
        *app.state::<UpdateState>()
            .available
            .lock()
            .expect("update state mutex poisoned") = Some(info);
    });
}

const TRAY_ID: &str = "main";

// Every 2 s: refresh the tray icon tooltip from status.txt and raise the
// 5-hour usage notifications. Runs beside the daemon; reads files only.
fn spawn_status_watcher(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut alerts = alerts::AlertState::default();
        let mut last_tooltip = String::new();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let Some(status) = status_path()
                .ok()
                .and_then(|path| fs::read_to_string(path).ok())
                .and_then(|raw| {
                    serde_json::from_str::<Value>(raw.trim_start_matches('\u{feff}')).ok()
                })
            else {
                continue;
            };
            let config = read_config().unwrap_or_default();
            let french = alerts::is_french(&config.language);

            let tooltip = alerts::tray_tooltip(&status, french);
            if tooltip != last_tooltip {
                if let Some(tray) = app.tray_by_id(TRAY_ID) {
                    let _ = tray.set_tooltip(Some(&tooltip));
                }
                last_tooltip = tooltip;
            }

            if let Some(percent) = alerts::five_hour_percent(&status) {
                for alert in alerts.update(percent, &config) {
                    let (title, body) = alerts::alert_text(alert, percent, french);
                    let _ = app.notification().builder().title(title).body(body).show();
                }
            }
        }
    });
}

fn create_tray(app: &mut tauri::App) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Claude RPC")
        .icon(app.default_window_icon().unwrap().clone())
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = event
            {
                match button {
                    MouseButton::Left => show_settings(tray.app_handle()),
                    MouseButton::Right => show_tray_menu(tray.app_handle(), position),
                    MouseButton::Middle => {}
                }
            }
        })
        .build(app)?;

    Ok(())
}

// Logical size of the custom tray menu window. It opens at the maximum height;
// tray.js then reports the menu's real height through `tray_fit`, which shrinks
// the window upward from its bottom edge so no transparent area catches clicks.
const TRAY_MENU_WIDTH: f64 = 316.0;
const TRAY_MENU_HEIGHT: f64 = 720.0;

#[tauri::command]
fn tray_fit(app: tauri::AppHandle, height: f64) -> Result<(), String> {
    let window = app
        .get_webview_window("tray")
        .ok_or_else(|| "tray window missing".to_string())?;
    let scale = window.scale_factor().map_err(|err| err.to_string())?;
    let position = window.outer_position().map_err(|err| err.to_string())?;
    let size = window.outer_size().map_err(|err| err.to_string())?;
    let area = window
        .current_monitor()
        .ok()
        .flatten()
        .map(|monitor| *monitor.work_area());
    let mut height = (height.clamp(120.0, TRAY_MENU_HEIGHT) * scale).round() as u32;
    if let Some(area) = area {
        height = height.min(area.size.height);
    }
    if height == size.height {
        return Ok(());
    }
    // Grow upward from the bottom edge, never above the top of the work area.
    let bottom = position.y + size.height as i32;
    let mut top = bottom - height as i32;
    if let Some(area) = area {
        top = top.max(area.position.y);
    }
    window
        .set_size(tauri::PhysicalSize::new(size.width, height))
        .map_err(|err| err.to_string())?;
    window
        .set_position(tauri::PhysicalPosition::new(position.x, top))
        .map_err(|err| err.to_string())
}

fn show_tray_menu(app: &tauri::AppHandle, cursor: tauri::PhysicalPosition<f64>) {
    let window = match app.get_webview_window("tray") {
        Some(window) => window,
        None => {
            let Ok(window) = tauri::WebviewWindowBuilder::new(
                app,
                "tray",
                tauri::WebviewUrl::App("tray.html".into()),
            )
            .inner_size(TRAY_MENU_WIDTH, TRAY_MENU_HEIGHT)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .build() else {
                return;
            };
            let window_to_hide = window.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::Focused(false) = event {
                    let _ = window_to_hide.hide();
                }
            });
            window
        }
    };

    place_tray(app, &window, cursor);
    let _ = window.show();
    let _ = window.set_focus();
}

// Bottom-right corner at the cursor, kept inside the work area of the monitor
// under the cursor (which can have negative coordinates or another DPI scale).
fn place_tray(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    cursor: tauri::PhysicalPosition<f64>,
) {
    // Logical height the menu last reported (tray_fit), independent of DPI.
    let logical_height = match (window.outer_size(), window.scale_factor()) {
        (Ok(size), Ok(scale)) if size.height > 0 => f64::from(size.height) / scale,
        _ => TRAY_MENU_HEIGHT,
    };
    let Some(monitor) = app.monitor_from_point(cursor.x, cursor.y).ok().flatten() else {
        let width = TRAY_MENU_WIDTH * window.scale_factor().unwrap_or(1.0);
        let height = logical_height * window.scale_factor().unwrap_or(1.0);
        let _ = window.set_position(tauri::PhysicalPosition::new(
            cursor.x - width,
            cursor.y - height,
        ));
        return;
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let (left, top) = (f64::from(area.position.x), f64::from(area.position.y));
    let (area_width, area_height) = (f64::from(area.size.width), f64::from(area.size.height));
    let width = (TRAY_MENU_WIDTH * scale).round().min(area_width);
    let height = (logical_height * scale).round().min(area_height);
    let x = (cursor.x - width).clamp(left, left + area_width - width);
    let y = (cursor.y - height).clamp(top, top + area_height - height);
    // Move first so Windows applies this monitor's DPI, then size and re-place.
    let position = tauri::PhysicalPosition::new(x, y);
    let _ = window.set_position(position);
    let _ = window.set_size(tauri::PhysicalSize::new(width, height));
    let _ = window.set_position(position);
}

fn set_mode(mode: &str) -> Result<(), String> {
    update_config(|config| config.rpc_mode = mode.into()).map(|_| ())
}

fn show_settings(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }
    if let Ok(window) =
        tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
            .title("Claude RPC Settings")
            .inner_size(790.0, 640.0)
            .min_inner_size(680.0, 480.0)
            .resizable(true)
            .decorations(false)
            .build()
    {
        let window_to_hide = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window_to_hide.hide();
            }
        });
    }
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
fn is_start_on_windows_enabled() -> bool {
    use std::os::windows::process::CommandExt;
    // A value left by a moved or reinstalled copy does not count: turning the
    // option on again rewrites it with this executable's path.
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    std::process::Command::new("reg.exe")
        .args(["query", STARTUP_REG_KEY, "/v", STARTUP_REG_VALUE])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .to_lowercase()
                    .contains(&exe.to_string_lossy().to_lowercase())
        })
        .unwrap_or(false)
}

// Any "Claude RPC" Run value, whatever path it points to.
#[cfg(windows)]
fn startup_value_exists() -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("reg.exe")
        .args(["query", STARTUP_REG_KEY, "/v", STARTUP_REG_VALUE])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn is_start_on_windows_enabled() -> bool {
    false
}

#[cfg(windows)]
fn set_start_on_windows(enabled: bool) -> Result<(), String> {
    if enabled {
        let exe = std::env::current_exe().map_err(|err| err.to_string())?;
        let command = format!("\"{}\"", exe.to_string_lossy());
        run_reg(&[
            "add",
            STARTUP_REG_KEY,
            "/v",
            STARTUP_REG_VALUE,
            "/t",
            "REG_SZ",
            "/d",
            command.as_str(),
            "/f",
        ])
    } else if startup_value_exists() {
        run_reg(&["delete", STARTUP_REG_KEY, "/v", STARTUP_REG_VALUE, "/f"])
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn set_start_on_windows(_enabled: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
fn run_reg(args: &[&str]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("reg.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|err| err.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        Err(format!("reg.exe failed: {}", output.status))
    } else {
        Err(stderr)
    }
}

fn start_daemon_inner(state: &DaemonState) {
    let mut running = state.running.lock().expect("daemon state mutex poisoned");
    if *running {
        return;
    }

    state.stop.store(false, Ordering::SeqCst);
    state.force_refresh.store(false, Ordering::SeqCst);
    *running = true;

    let stop = Arc::clone(&state.stop);
    let force_refresh = Arc::clone(&state.force_refresh);
    let running_flag = Arc::clone(&state.running);
    let config_path = config_path().ok();
    let status_path = status_path().ok();
    if let Some(handle) = state
        .handle
        .lock()
        .expect("daemon handle mutex poisoned")
        .take()
    {
        let _ = handle.join();
    }

    let handle = std::thread::spawn(move || {
        // Reset the flag even if the daemon panics, so opening the settings
        // (start_daemon) can restart it instead of believing it still runs.
        struct Stopped(Arc<Mutex<bool>>);
        impl Drop for Stopped {
            fn drop(&mut self) {
                *self.0.lock().unwrap_or_else(|err| err.into_inner()) = false;
            }
        }
        let _stopped = Stopped(running_flag);
        daemon::run(stop, force_refresh, config_path, status_path);
    });
    *state.handle.lock().expect("daemon handle mutex poisoned") = Some(handle);
}

fn request_daemon_stop(state: &DaemonState) -> Option<std::thread::JoinHandle<()>> {
    state.stop.store(true, Ordering::SeqCst);
    state
        .handle
        .lock()
        .expect("daemon handle mutex poisoned")
        .take()
}

fn join_with_timeout(handle: Option<std::thread::JoinHandle<()>>, timeout_ms: u64) {
    let Some(handle) = handle else {
        return;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    while !handle.is_finished() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    if handle.is_finished() {
        let _ = handle.join();
    }
}

fn update_config<F>(mutator: F) -> Result<ClaudeConfig, String>
where
    F: FnOnce(&mut ClaudeConfig),
{
    // Strict read: a tray click must not replace an unreadable config.json
    // with defaults (that would drop private projects and other privacy keys).
    let mut config = config::load_config(&config_path()?)?;
    mutator(&mut config);
    let config = config::normalize_config(config);
    write_config(&config)?;
    Ok(config)
}

fn read_config() -> Result<ClaudeConfig, String> {
    // An unparsable file still opens the settings with defaults so it can be
    // repaired from the UI; the daemon keeps its last good copy meanwhile.
    Ok(config::load_config(&config_path()?).unwrap_or_default())
}

fn write_config(config: &ClaudeConfig) -> Result<(), String> {
    config::write_config_atomic(&config_path()?, config)
}

fn config_path() -> Result<PathBuf, String> {
    Ok(app_dir()?.join("config.json"))
}

fn status_path() -> Result<PathBuf, String> {
    Ok(app_dir()?.join("status.txt"))
}

fn app_dir() -> Result<PathBuf, String> {
    Ok(daemon::app_dir())
}
