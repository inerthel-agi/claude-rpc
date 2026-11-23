// No console window, in development builds too: closing one would end the app.
#![windows_subsystem = "windows"]

mod alerts;
mod config;
mod daemon;
mod launch;
mod panel;
mod styles;
mod view;

slint::include_modules!();

use config::{ClaudeConfig, RpcButton};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    cell::RefCell,
    fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock, Mutex,
    },
    time::Duration,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Identity of the app's notifications; the installer registers it.
const APP_ID: &str = "eu.inerthel.claude-rpc";
const RELEASES_API: &str = "https://api.github.com/repos/inerthel-agi/claude-rpc/releases/latest";
const STARTUP_REG_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_REG_VALUE: &str = "Claude RPC";
// Where Task Manager and Settings > Apps > Startup record a disabled Run entry.
const STARTUP_APPROVED_KEY: &str =
    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const CREATE_NO_WINDOW: u32 = 0x08000000;
/// Tray panel sections the user can hide (General page); Settings and Quit always stay.
pub(crate) const TRAY_SECTIONS: [&str; 11] = [
    "session", "usage", "chart", "limits", "credits", "pause", "activity", "desktop", "code",
    "update", "footer",
];

#[derive(Default)]
struct DaemonState {
    running: Arc<Mutex<bool>>,
    stop: Arc<AtomicBool>,
    force_refresh: Arc<AtomicBool>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

static DAEMON: LazyLock<DaemonState> = LazyLock::new(DaemonState::default);

// ---------- Files ----------

fn app_dir() -> PathBuf {
    daemon::app_dir()
}

fn config_path() -> PathBuf {
    app_dir().join("config.json")
}

fn status_path() -> PathBuf {
    app_dir().join("status.txt")
}

pub(crate) fn now_ms() -> u64 {
    daemon::now_ms()
}

/// The daemon's status file; `Null` while it has not written one.
pub(crate) fn read_status() -> Value {
    fs::read_to_string(status_path())
        .ok()
        .and_then(|raw| serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok())
        .unwrap_or(Value::Null)
}

/// Tray and status watcher: an unreadable file reads as paused (DND on), the
/// same state the daemon falls back to.
pub(crate) fn read_config() -> ClaudeConfig {
    config::load_or_paused(&config_path())
}

fn update_config(change: impl FnOnce(&mut ClaudeConfig)) -> Result<ClaudeConfig, String> {
    // Strict read: a tray click must not replace an unreadable config.json
    // with defaults (that would drop private projects and other privacy keys).
    let mut config = config::load_config(&config_path())?;
    change(&mut config);
    let config = config::normalize_config(config);
    config::write_config_atomic(&config_path(), &config)?;
    Ok(config)
}

/// Choices that belong to this interface, not to the daemon: kept apart from
/// config.json, which the daemon watches.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct UiPrefs {
    /// dark | oled | system | light
    theme: String,
    /// One of styles::DESIGNS.
    design: String,
    tray_hidden: Vec<String>,
    /// Settings page shown last.
    page: i32,
    /// Version whose "Updated to" card was dismissed.
    seen_version: String,
}

pub(crate) fn read_prefs() -> UiPrefs {
    fs::read_to_string(app_dir().join("ui.json"))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn update_prefs(change: impl FnOnce(&mut UiPrefs)) {
    let mut prefs = read_prefs();
    change(&mut prefs);
    if let Ok(json) = serde_json::to_string_pretty(&prefs) {
        let _ = fs::create_dir_all(app_dir());
        let _ = fs::write(app_dir().join("ui.json"), json);
    }
}

pub(crate) fn tray_shown(prefs: &UiPrefs) -> ModelRc<bool> {
    ModelRc::new(VecModel::from(
        TRAY_SECTIONS
            .iter()
            .map(|id| !prefs.tray_hidden.iter().any(|hidden| hidden == id))
            .collect::<Vec<_>>(),
    ))
}

// ---------- Updates ----------

/// Result of the update check. A newer release is offered as a link to its
/// download page; nothing is installed from here.
#[derive(Debug, Clone, Default)]
pub(crate) struct Update {
    pub(crate) latest: Option<String>,
    pub(crate) checking: bool,
    pub(crate) error: Option<String>,
    /// When a check asked for by the user ended; 0 before the first one.
    pub(crate) checked_ms: u64,
}

static UPDATE: Mutex<Update> = Mutex::new(Update {
    latest: None,
    checking: false,
    error: None,
    checked_ms: 0,
});

pub(crate) fn update_state() -> Update {
    UPDATE
        .lock()
        .map(|update| update.clone())
        .unwrap_or_default()
}

fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |value: &str| -> Vec<u64> {
        value
            .split('.')
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };
    parse(latest) > parse(current)
}

fn fetch_latest_version() -> Result<String, String> {
    let body: Value = ureq::get(RELEASES_API)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "claude-rpc")
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|err| err.to_string())?
        .into_json()
        .map_err(|err| err.to_string())?;
    let tag = body
        .get("tag_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim_start_matches('v');
    let valid = !tag.is_empty()
        && tag.len() <= 16
        && tag.chars().all(|ch| ch.is_ascii_digit() || ch == '.');
    if valid {
        Ok(tag.to_string())
    } else {
        Err("unexpected release tag".into())
    }
}

/// Runs on its own thread; the windows read the result at their next tick.
/// `manual`: asked for by the user, so a failure is shown.
pub(crate) fn check_update(manual: bool) {
    {
        let Ok(mut update) = UPDATE.lock() else {
            return;
        };
        if update.checking {
            return;
        }
        update.checking = true;
    }
    std::thread::spawn(move || {
        let result = fetch_latest_version();
        let Ok(mut update) = UPDATE.lock() else {
            return;
        };
        update.checking = false;
        match result {
            Ok(latest) => {
                update.error = None;
                update.latest = Some(latest).filter(|latest| is_newer(latest, VERSION));
            }
            Err(error) if manual => update.error = Some(error),
            Err(_) => return,
        }
        if manual {
            update.checked_ms = now_ms();
        }
    });
}

// ---------- Actions ----------

/// Opens one of the app's fixed links; the interface only passes a name.
pub(crate) fn open_link(name: &str) -> Result<(), String> {
    let url = launch::link_url(name).ok_or_else(|| format!("unknown link: {name}"))?;
    launch::open_url(url)
}

/// Whether this copy was put there by the installer (it sits beside its uninstaller).
fn is_installed_copy() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("uninstall.exe").is_file()))
        .unwrap_or(false)
}

/// Returns whether Windows accepted the notification.
fn notify(title: &str, body: &str) -> bool {
    let mut notification = notify_rust::Notification::new();
    notification.summary(title).body(body);
    // Windows only attributes a toast to the app once the installer has
    // registered its id; a portable or development copy goes without.
    if is_installed_copy() {
        notification.app_id(APP_ID);
    }
    notification.show().is_ok()
}

fn diagnostic() -> String {
    let yes_no = |value: bool| if value { "yes" } else { "no" };
    let approval = match startup_approval() {
        Some(true) => "enabled",
        Some(false) => "disabled",
        None => "not set",
    };
    format!(
        "Claude RPC v{VERSION}\n{}\nStart with Windows: Run value present: {}, points to this executable: {}, Windows approval: {approval}",
        daemon::diagnostic_report(),
        yes_no(startup_value_exists()),
        yes_no(run_value_points_here())
    )
}

// The tray panel's actions. Each one changes a single field of the file on disk.

/// The switch pauses until resumed (Do Not Disturb); a timed pause ends with it too.
pub(crate) fn toggle_pause() {
    let paused = read_config().is_paused(now_ms());
    let _ = update_config(|config| {
        config.dnd = !paused;
        config.pause_until_ms = 0;
    });
    panel::refresh();
}

/// `value`: minutes, or "tomorrow" for the next local midnight; capped at 48 hours.
pub(crate) fn pause(value: &str) {
    let now = now_ms();
    let until = match value {
        "tomorrow" => view::next_midnight(now, view::local_offset_ms()),
        minutes => now + minutes.parse::<u64>().unwrap_or(30) * 60_000,
    }
    .clamp(now, now + 48 * 60 * 60 * 1000);
    let _ = update_config(|config| {
        config.pause_until_ms = until;
        config.dnd = false;
    });
    panel::refresh();
}

pub(crate) fn set_mode(mode: &str) {
    let _ = update_config(|config| config.rpc_mode = mode.into());
    panel::refresh();
}

pub(crate) fn quit() {
    let _ = slint::quit_event_loop();
}

// ---------- Start with Windows ----------

fn reg(args: &[&str]) -> std::io::Result<std::process::Output> {
    std::process::Command::new("reg.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
}

// `reg query` output for the StartupApproved value: Windows marks a disabled
// entry with an odd first byte (03 00 00 00 ...); even means enabled.
fn parse_startup_approved(reg_output: &str) -> Option<bool> {
    let (_, data) = reg_output
        .lines()
        .find_map(|line| line.split_once("REG_BINARY"))?;
    let byte = u8::from_str_radix(data.trim().get(..2)?, 16).ok()?;
    Some(byte % 2 == 0)
}

// None when Windows has no StartupApproved value for us (never toggled).
fn startup_approval() -> Option<bool> {
    let output = reg(&["query", STARTUP_APPROVED_KEY, "/v", STARTUP_REG_VALUE]).ok()?;
    parse_startup_approved(&String::from_utf8_lossy(&output.stdout))
}

fn is_start_on_windows_enabled() -> bool {
    run_value_points_here() && startup_approval() != Some(false)
}

fn run_value_points_here() -> bool {
    // A value left by a moved or reinstalled copy does not count: turning the
    // option on again rewrites it with this executable's path.
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    reg(&["query", STARTUP_REG_KEY, "/v", STARTUP_REG_VALUE])
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .to_lowercase()
                    .contains(&exe.to_string_lossy().to_lowercase())
        })
        .unwrap_or(false)
}

// Any "Claude RPC" Run value, whatever path it points to.
fn startup_value_exists() -> bool {
    reg(&["query", STARTUP_REG_KEY, "/v", STARTUP_REG_VALUE])
        .map(|output| output.status.success())
        .unwrap_or(false)
}

// The installed copy takes over a "Start with Windows" entry left pointing at
// another copy, such as an old portable build: else every login starts that
// older version, with the bugs it still has. An entry the user disabled in
// Task Manager stays disabled (StartupApproved untouched).
fn repair_start_on_windows() {
    if is_installed_copy() && startup_value_exists() && !run_value_points_here() {
        let _ = write_run_value();
    }
}

fn set_start_on_windows(enabled: bool) -> Result<(), String> {
    if enabled {
        write_run_value()?;
        // Turning it on is explicit: lift a "disabled" set in Task Manager.
        if startup_approval().is_some() {
            run_reg(&[
                "delete",
                STARTUP_APPROVED_KEY,
                "/v",
                STARTUP_REG_VALUE,
                "/f",
            ])?;
        }
        Ok(())
    } else if startup_value_exists() {
        run_reg(&["delete", STARTUP_REG_KEY, "/v", STARTUP_REG_VALUE, "/f"])
    } else {
        Ok(())
    }
}

fn write_run_value() -> Result<(), String> {
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
}

fn run_reg(args: &[&str]) -> Result<(), String> {
    let output = reg(args).map_err(|err| err.to_string())?;
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

// ---------- Daemon thread ----------

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
        // can restart it instead of believing it still runs.
        struct Stopped(Arc<Mutex<bool>>);
        impl Drop for Stopped {
            fn drop(&mut self) {
                *self.0.lock().unwrap_or_else(|err| err.into_inner()) = false;
            }
        }
        let _stopped = Stopped(running_flag);
        daemon::run(
            stop,
            force_refresh,
            Some(config_path()),
            Some(status_path()),
        );
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
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    while !handle.is_finished() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
    }
    if handle.is_finished() {
        let _ = handle.join();
    }
}

// ---------- Status watcher ----------

/// Whether the tray's "Claude Desktop" / "Claude Code" items open the app or
/// its download / install page. Looked up off the event-loop thread.
pub(crate) static DESKTOP_INSTALLED: AtomicBool = AtomicBool::new(true);
pub(crate) static CODE_INSTALLED: AtomicBool = AtomicBool::new(true);

// Every 2 s: refresh the tray icon tooltip from status.txt and raise the
// usage notifications. Runs beside the daemon; reads files only.
fn spawn_status_watcher() {
    std::thread::spawn(|| {
        let mut alerts = alerts::AlertState::default();
        let mut last_tooltip = String::new();
        for tick in 0u64.. {
            if tick % 15 == 0 {
                DESKTOP_INSTALLED.store(launch::find_desktop().is_some(), Ordering::Relaxed);
                CODE_INSTALLED.store(launch::find_code().is_some(), Ordering::Relaxed);
            }
            std::thread::sleep(Duration::from_secs(2));
            let status = read_status();
            if status.is_null() {
                continue;
            }
            let config = read_config();
            let french = alerts::is_french(&config.language);

            let tooltip = alerts::tray_tooltip(&status, french);
            if tooltip != last_tooltip {
                last_tooltip = tooltip.clone();
                // The icon lives on the event-loop thread.
                let _ = slint::invoke_from_event_loop(move || panel::set_tip(&tooltip));
            }

            if let Some((label, percent)) = alerts::alert_limit(&status) {
                for alert in alerts.update(label, percent, &config) {
                    let (title, body) = alerts::alert_text(alert, label, percent, french);
                    notify(&title, &body);
                }
            }
        }
    });
}

// ---------- Windows ----------

thread_local! {
    /// Lives on the event-loop thread; other threads reach it through
    /// `slint::invoke_from_event_loop`.
    static SETTINGS: RefCell<Option<SettingsUi>> = const { RefCell::new(None) };
}

/// Colours, motion and fonts for a newly created window.
pub(crate) fn apply_theme(theme: &Theme) {
    let prefs = read_prefs();
    let mode = match prefs.theme.as_str() {
        mode @ ("oled" | "system" | "light") => mode.to_string(),
        _ => "dark".into(),
    };
    theme.set_mode(mode.into());
    theme.set_motion(animations_enabled());
    theme.set_mono(styles::mono_font().into());
    styles::apply(theme, &prefs.design);
}

/// Follows Windows' "Animation effects" setting.
fn animations_enabled() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };
    let mut enabled: i32 = 1;
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut i32 as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_err() || enabled != 0
}

/// Gives the window's title bar and border the theme's colours (title bar colour
/// needs Windows 11; Windows 10 only switches it between light and dark), so
/// the frame matches the app instead of the Windows theme.
/// Returns the colours applied, to repaint only when the theme changes.
pub(crate) fn paint_frame(window: &slint::Window, theme: &Theme) -> [slint::Color; 3] {
    use slint::winit_030::winit::{platform::windows::WindowExtWindows, window::Theme as Frame};
    use slint::winit_030::{winit::platform::windows::Color, WinitWindowAccessor};
    let colors = [theme.get_panel(), theme.get_text(), theme.get_frame()];
    let rgb = |color: slint::Color| Color::from_rgb(color.red(), color.green(), color.blue());
    let dark = theme.get_dark();
    window.with_winit_window(|frame| {
        // Light or dark caption buttons, which keep their own colours.
        frame.set_theme(Some(if dark { Frame::Dark } else { Frame::Light }));
        frame.set_title_background_color(Some(rgb(colors[0])));
        frame.set_title_text_color(rgb(colors[1]));
        frame.set_border_color(Some(rgb(colors[2])));
        hide_caption(frame);
    });
    colors
}

/// Leaves the title bar empty: the sidebar already shows the name and the logo.
/// The window keeps both for the taskbar and Alt+Tab.
fn hide_caption(frame: &slint::winit_030::winit::window::Window) {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::Controls::{
        SetWindowThemeAttribute, WTA_NONCLIENT, WTA_OPTIONS, WTNCA_NODRAWCAPTION, WTNCA_NODRAWICON,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageW, ICON_SMALL, WM_GETICON, WM_SETICON,
    };
    let Ok(RawWindowHandle::Win32(handle)) = frame.window_handle().map(|handle| handle.as_raw())
    else {
        return;
    };
    let window = HWND(handle.hwnd.get() as _);
    let flags = WTNCA_NODRAWCAPTION | WTNCA_NODRAWICON;
    let options = WTA_OPTIONS {
        dwFlags: flags,
        dwMask: flags,
    };
    let small = Some(WPARAM(ICON_SMALL as usize));
    unsafe {
        let _ = SetWindowThemeAttribute(
            window,
            WTA_NONCLIENT,
            (&raw const options).cast(),
            size_of::<WTA_OPTIONS>() as u32,
        );
        // Windows 11 keeps drawing an icon set before the call until that icon
        // changes: take it off and put it straight back.
        let icon = SendMessageW(window, WM_GETICON, small, None);
        SendMessageW(window, WM_SETICON, small, None);
        SendMessageW(window, WM_SETICON, small, Some(LPARAM(icon.0)));
    }
}

fn strings(values: &[String]) -> ModelRc<slint::SharedString> {
    ModelRc::new(VecModel::from(
        values
            .iter()
            .map(|value| value.as_str().into())
            .collect::<Vec<slint::SharedString>>(),
    ))
}

pub(crate) fn limit(view: &view::LimitView) -> Limit {
    Limit {
        name: view.name.as_str().into(),
        figure: view.figure.as_str().into(),
        percent: view.percent,
        level: view.level,
        reset_in: view.reset_in.as_str().into(),
        reset_at: view.reset_at.as_str().into(),
        chart: view.chart,
    }
}

pub(crate) fn limits(views: &[view::LimitView]) -> ModelRc<Limit> {
    ModelRc::new(VecModel::from(views.iter().map(limit).collect::<Vec<_>>()))
}

// ---------- Settings window ----------

/// The line at the bottom of the settings window.
enum Note {
    Auto,
    Saving,
    Saved(String),
    Broken,
    Error(String),
}

/// The settings window exists only while it is open; closing it frees its memory.
struct SettingsUi {
    window: SettingsWindow,
    /// What the form currently holds; `pause_until_ms` has no control and keeps its saved value.
    settings: ClaudeConfig,
    dirty: bool,
    /// config.json exists but cannot be read: never save over it with what the
    /// form holds (defaults) until a later read succeeds.
    broken: bool,
    /// The tray can change these two as well: a save keeps the value on disk
    /// unless it was changed in this window.
    touched_dnd: bool,
    touched_mode: bool,
    french: bool,
    note: Note,
    shown: Option<view::StatusView>,
    /// Colours last given to the title bar; "System" can change them while the window is open.
    frame: Option<[slint::Color; 3]>,
    save: slint::Timer,
    _tick: slint::Timer,
}

/// `texts`: also fill the text fields. Only when the window opens: afterwards
/// they belong to this window (the tray never changes them), and rewriting
/// them from disk would erase a half-typed button or move the caret.
fn fill_form(app: &App, config: &ClaudeConfig, texts: bool) {
    app.set_mode(config.rpc_mode.as_str().into());
    app.set_dnd(config.dnd);
    app.set_show_provider(config.show_provider);
    app.set_show_plan(config.show_plan);
    app.set_show_effort(config.show_effort);
    app.set_show_sessions(config.show_sessions);
    app.set_show_session_name(config.show_session_name);
    app.set_show_branch(config.show_branch);
    app.set_model_icon(config.model_icon);
    app.set_show_limits(config.show_limits);
    app.set_show_limit_5h(config.show_limit_5h);
    app.set_show_limit_all(config.show_limit_all);
    app.set_show_limit_fable(config.show_limit_fable);
    app.set_alert_80(config.alert_80);
    app.set_alert_95(config.alert_95);
    app.set_alert_reset(config.alert_reset);
    app.set_hide_in_chat(config.hide_in_chat);
    app.set_language(config.language.as_str().into());
    if !texts {
        return;
    }
    let button = |index: usize| {
        config
            .buttons
            .get(index)
            .map(|button| (button.label.clone(), button.url.clone()))
            .unwrap_or_default()
    };
    app.set_label0(button(0).0.into());
    app.set_url0(button(0).1.into());
    app.set_label1(button(1).0.into());
    app.set_url1(button(1).1.into());
    app.set_details_template(config.details_template.as_str().into());
    app.set_state_template(config.state_template.as_str().into());
    app.set_private_projects(config.private_projects.join("\n").into());
}

/// Copies the form into `config`; fields without a control keep their value.
fn read_form(app: &App, config: &mut ClaudeConfig) {
    config.rpc_mode = app.get_mode().into();
    config.dnd = app.get_dnd();
    config.show_provider = app.get_show_provider();
    config.show_plan = app.get_show_plan();
    config.show_effort = app.get_show_effort();
    config.show_sessions = app.get_show_sessions();
    config.show_session_name = app.get_show_session_name();
    config.show_branch = app.get_show_branch();
    config.model_icon = app.get_model_icon();
    config.show_limits = app.get_show_limits();
    config.show_limit_5h = app.get_show_limit_5h();
    config.show_limit_all = app.get_show_limit_all();
    config.show_limit_fable = app.get_show_limit_fable();
    config.alert_80 = app.get_alert_80();
    config.alert_95 = app.get_alert_95();
    config.alert_reset = app.get_alert_reset();
    config.hide_in_chat = app.get_hide_in_chat();
    config.language = app.get_language().into();
    config.details_template = app.get_details_template().trim().into();
    config.state_template = app.get_state_template().trim().into();
    config.private_projects = app
        .get_private_projects()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    config.buttons = [
        (app.get_label0(), app.get_url0()),
        (app.get_label1(), app.get_url1()),
    ]
    .into_iter()
    .map(|(label, url)| RpcButton {
        label: label.trim().into(),
        url: url.trim().into(),
    })
    .filter(|button| !button.label.is_empty() && !button.url.is_empty())
    .collect();
}

fn with_settings(update: impl FnOnce(&mut SettingsUi)) {
    SETTINGS.with(|slot| {
        if let Some(ui) = slot.borrow_mut().as_mut() {
            update(ui);
        }
    });
}

impl SettingsUi {
    fn note(&mut self, note: Note) {
        self.note = note;
        self.render_note();
    }

    fn render_note(&self) {
        let (text, kind) = match &self.note {
            Note::Auto => (
                view::tr(
                    self.french,
                    "Changes save automatically",
                    "Enregistrement automatique",
                ),
                0,
            ),
            Note::Saving => (view::tr(self.french, "Saving…", "Enregistrement…"), 0),
            Note::Saved(time) if self.french => (format!("Enregistré · {time}"), 1),
            Note::Saved(time) => (format!("Saved · {time}"), 1),
            Note::Broken => (view::broken_config(self.french), 2),
            Note::Error(error) => (error.clone(), 2),
        };
        let app = self.window.global::<App>();
        app.set_message(text.into());
        app.set_message_kind(kind);
    }

    /// Follows the `language` setting; texts written from Rust are redone.
    fn apply_language(&mut self) {
        let french = alerts::is_french(&self.settings.language);
        self.window.global::<L>().set_french(french);
        if french != self.french {
            self.french = french;
            self.shown = None;
            self.render_note();
        }
        let app = self.window.global::<App>();
        app.set_error0(view::button_error(&app.get_label0(), &app.get_url0(), french).into());
        app.set_error1(view::button_error(&app.get_label1(), &app.get_url1(), french).into());
    }

    /// Status, limits, the Discord preview and the update check.
    fn refresh(&mut self) {
        let now = now_ms();
        let status = read_status();
        let app = self.window.global::<App>();
        let view = view::StatusView::new(
            &status,
            &self.settings,
            self.broken,
            self.window.get_chart_width(),
            now,
            view::local_offset_ms(),
            self.french,
        );
        let live = !view.preview.off;
        if self.shown.as_ref() != Some(&view) {
            app.set_client_on(view.client_on);
            app.set_client_label(view.client_label.as_str().into());
            app.set_discord_user(view.discord_user.as_str().into());
            app.set_plan(view.plan.as_str().into());
            app.set_has_5h(view.has_5h);
            app.set_limits(limits(&view.limits));
            let spark = view.spark.clone();
            let spark = spark.as_ref();
            app.set_spark_line(spark.map(|s| s.line.as_str()).unwrap_or("").into());
            app.set_spark_area(spark.map(|s| s.area.as_str()).unwrap_or("").into());
            app.set_spark_peak(spark.map(|s| s.peak.as_str()).unwrap_or("").into());
            app.set_credits_shown(view.credits.is_some());
            if let Some(credits) = &view.credits {
                app.set_credits_value(credits.value.as_str().into());
                app.set_credits_of(credits.of.as_str().into());
                app.set_credits_percent(credits.percent);
                app.set_credits_level(credits.level);
                app.set_credits_note(credits.note.as_str().into());
            }
            app.set_pv_off(view.preview.off);
            app.set_pv_activity(view.preview.activity.as_str().into());
            app.set_pv_primary(view.preview.primary.as_str().into());
            app.set_pv_secondary(view.preview.secondary.as_str().into());
            app.set_pv_tertiary(view.preview.tertiary.as_str().into());
            app.set_pv_buttons(strings(&view.preview.buttons));
            self.shown = Some(view);
        }
        app.set_pv_elapsed(if live {
            view::elapsed(&status, now, self.french).into()
        } else {
            "".into()
        });
        let update = update_state();
        app.set_update_status(view::update_status(&update, self.french).into());
        app.set_checking(update.checking);
        app.set_update_version(update.latest.unwrap_or_default().into());
    }

    fn set_broken(&mut self) {
        self.broken = true;
        self.shown = None;
        self.note(Note::Broken);
    }

    /// Saves on top of the config on disk: the tray may have changed DND, the
    /// pause or the activity type since this form was filled.
    fn save_now(&mut self) {
        self.save.stop();
        if !self.dirty {
            return;
        }
        self.dirty = false;
        if self.broken {
            self.note(Note::Broken);
            return;
        }
        let Ok(disk) = config::load_config(&config_path()) else {
            self.set_broken();
            return;
        };
        let mut config = self.settings.clone();
        config.pause_until_ms = disk.pause_until_ms;
        if !self.touched_dnd {
            config.dnd = disk.dnd;
        }
        if !self.touched_mode {
            config.rpc_mode = disk.rpc_mode;
        }
        let config = config::normalize_config(config);
        match config::write_config_atomic(&config_path(), &config) {
            Ok(()) => {
                (self.touched_dnd, self.touched_mode) = (false, false);
                fill_form(&self.window.global::<App>(), &config, false);
                self.settings = config;
                self.note(Note::Saved(view::local_clock(
                    now_ms(),
                    view::local_offset_ms(),
                )));
            }
            Err(error) => self.note(Note::Error(error)),
        }
    }

    /// Picks up what the tray changed (DND, pause, activity type) while this
    /// window is open. Never while an edit waits for its save.
    fn sync_from_disk(&mut self) {
        if self.dirty {
            return;
        }
        match config::load_config(&config_path()) {
            Ok(fresh) if self.broken => {
                // Repaired: the form still holds defaults, so refill all of it.
                self.broken = false;
                self.shown = None;
                fill_form(&self.window.global::<App>(), &fresh, true);
                self.settings = fresh;
                self.apply_language();
                self.note(Note::Auto);
            }
            Ok(fresh) if fresh != self.settings => {
                fill_form(&self.window.global::<App>(), &fresh, false);
                // The text fields keep what was typed; read them back.
                self.settings = fresh;
                read_form(&self.window.global::<App>(), &mut self.settings);
                self.apply_language();
            }
            Ok(_) => {}
            Err(_) if !self.broken => self.set_broken(),
            Err(_) => {}
        }
    }

    fn repaint_frame(&mut self) {
        let theme = self.window.global::<Theme>();
        let wanted = [theme.get_panel(), theme.get_text(), theme.get_frame()];
        if self.frame != Some(wanted) {
            self.frame = Some(paint_frame(self.window.window(), &theme));
        }
    }

    fn tick(&mut self) {
        self.repaint_frame();
        self.sync_from_disk();
        self.refresh();
    }
}

fn report(result: Result<(), String>) {
    if let Err(error) = result {
        with_settings(|ui| ui.note(Note::Error(error)));
    }
}

/// Reads the registry on another thread (reg.exe takes a moment), then shows the result.
fn refresh_startup(toggle: bool) {
    std::thread::spawn(move || {
        let result = if toggle {
            set_start_on_windows(!is_start_on_windows_enabled())
        } else {
            Ok(())
        };
        let enabled = is_start_on_windows_enabled();
        let _ = slint::invoke_from_event_loop(move || {
            with_settings(|ui| {
                ui.window.global::<App>().set_startup_enabled(enabled);
                if let Err(error) = result {
                    ui.note(Note::Error(error));
                }
            });
        });
    });
}

pub(crate) fn open_settings() {
    let open = SETTINGS.with(|slot| slot.borrow().as_ref().map(|ui| ui.window.clone_strong()));
    if let Some(window) = open {
        // Showing it again is what brings an already open window to the front.
        let _ = window.hide();
        let _ = window.show();
        return;
    }
    // Restarts the daemon thread if it has exited.
    start_daemon_inner(&DAEMON);
    let Ok(window) = SettingsWindow::new() else {
        return;
    };
    apply_theme(&window.global::<Theme>());
    let prefs = read_prefs();
    window.set_page(prefs.page.clamp(0, 5));
    let app = window.global::<App>();
    app.set_version(VERSION.into());
    app.set_tray_shown(tray_shown(&prefs));
    app.set_whats_new(prefs.seen_version != VERSION);

    app.on_changed(|| {
        with_settings(|ui| {
            let before = (ui.settings.dnd, ui.settings.rpc_mode.clone());
            read_form(&ui.window.global::<App>(), &mut ui.settings);
            ui.touched_dnd |= ui.settings.dnd != before.0;
            ui.touched_mode |= ui.settings.rpc_mode != before.1;
            ui.apply_language();
            ui.dirty = true;
            if !ui.broken {
                ui.note(Note::Saving);
            }
            ui.save.start(
                slint::TimerMode::SingleShot,
                Duration::from_millis(300),
                || with_settings(SettingsUi::save_now),
            );
            ui.refresh();
        });
    });
    app.on_set_theme(|mode| {
        update_prefs(|prefs| prefs.theme = mode.to_string());
        with_settings(|ui| {
            ui.window.global::<Theme>().set_mode(mode);
            ui.repaint_frame();
        });
    });
    app.on_set_design(|design| {
        let design = styles::known(&design);
        update_prefs(|prefs| prefs.design = design.into());
        with_settings(|ui| {
            styles::apply(&ui.window.global::<Theme>(), design);
            ui.shown = None;
            ui.repaint_frame();
            // Split Studio adds a column on the right: make room for it.
            let window = ui.window.window();
            let scale = window.scale_factor();
            let size = window.size().to_logical(scale);
            if design == "split" && size.width < 1080.0 {
                window.set_size(slint::LogicalSize::new(1080.0, size.height));
            }
        });
    });
    app.on_toggle_startup(|| refresh_startup(true));
    app.on_toggle_tray(|index| {
        let Some(id) = TRAY_SECTIONS.get(index as usize) else {
            return;
        };
        update_prefs(
            |prefs| match prefs.tray_hidden.iter().position(|hidden| hidden == id) {
                Some(at) => drop(prefs.tray_hidden.remove(at)),
                None => prefs.tray_hidden.push((*id).into()),
            },
        );
        with_settings(|ui| {
            ui.window
                .global::<App>()
                .set_tray_shown(tray_shown(&read_prefs()))
        });
    });
    app.on_refresh_limits(|| {
        with_settings(|ui| {
            ui.save_now();
            ui.window.global::<App>().set_refreshing(true);
        });
        DAEMON.force_refresh.store(true, Ordering::SeqCst);
        slint::Timer::single_shot(Duration::from_millis(1500), || {
            with_settings(|ui| {
                ui.window.global::<App>().set_refreshing(false);
                ui.refresh();
            });
        });
    });
    app.on_check_update(|| {
        check_update(true);
        with_settings(SettingsUi::refresh);
    });
    app.on_copy_diagnostic(|| {
        // The report scans processes and windows: not on the event-loop thread.
        std::thread::spawn(|| {
            let text = diagnostic();
            let _ = slint::invoke_from_event_loop(move || {
                with_settings(|ui| {
                    let app = ui.window.global::<App>();
                    app.set_diagnostic_text(text.into());
                    ui.window.invoke_copy_diagnostic();
                    app.set_diagnostic_copied(true);
                });
            });
        });
    });
    app.on_dismiss_whats_new(|| {
        update_prefs(|prefs| prefs.seen_version = VERSION.into());
        with_settings(|ui| ui.window.global::<App>().set_whats_new(false));
    });
    app.on_open_link(|name| report(open_link(&name)));
    app.on_page_changed(|page| update_prefs(|prefs| prefs.page = page));
    app.on_close(close_settings);
    window.window().on_close_requested(|| {
        close_settings();
        slint::CloseRequestResponse::HideWindow
    });

    let tick = slint::Timer::default();
    tick.start(slint::TimerMode::Repeated, Duration::from_secs(1), || {
        with_settings(SettingsUi::tick)
    });
    let (settings, broken) = match config::load_config(&config_path()) {
        Ok(settings) => (settings, false),
        Err(_) => (ClaudeConfig::default(), true),
    };
    let mut ui = SettingsUi {
        window,
        settings,
        dirty: false,
        broken,
        touched_dnd: false,
        touched_mode: false,
        french: false,
        note: if broken { Note::Broken } else { Note::Auto },
        shown: None,
        frame: None,
        save: slint::Timer::default(),
        _tick: tick,
    };
    fill_form(&ui.window.global::<App>(), &ui.settings, true);
    ui.apply_language();
    ui.render_note();
    ui.refresh();
    let _ = ui.window.show();
    // Slint creates the real window when this event-loop turn ends: painting the
    // title bar before that does nothing and leaves it in the Windows colours.
    let weak = ui.window.as_weak();
    let _ = slint::spawn_local(async move {
        use slint::winit_030::WinitWindowAccessor;
        let Some(window) = weak.upgrade() else {
            return;
        };
        if window.window().winit_window().await.is_ok() {
            with_settings(SettingsUi::repaint_frame);
        }
    });
    SETTINGS.with(|slot| *slot.borrow_mut() = Some(ui));
    refresh_startup(false);
}

fn close_settings() {
    with_settings(SettingsUi::save_now);
    // Dropped from a fresh event-loop turn: this runs inside one of the window's own callbacks.
    slint::Timer::single_shot(Duration::ZERO, || {
        if let Some(ui) = SETTINGS.with(|slot| slot.borrow_mut().take()) {
            let _ = ui.window.hide();
        }
    });
}

// ---------- Startup ----------

fn acquire_instance_lock(path: &Path) -> std::io::Result<Option<fs::File>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(error)) => Err(error),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A second launch (Start menu, double-click, autostart) asks the running
    // app for its settings window, so only one daemon ever drives Discord.
    let open_request = app_dir().join("open-settings.request");
    // Keep the OS lock alive until the app exits; a crash releases it too.
    let Some(_instance) = acquire_instance_lock(&app_dir().join("app.lock"))? else {
        fs::write(&open_request, b"")?;
        return Ok(());
    };
    let _ = fs::remove_file(&open_request);
    // A first install has nothing to announce.
    if read_prefs().seen_version.is_empty() && !config_path().exists() {
        update_prefs(|prefs| prefs.seen_version = VERSION.into());
    }

    start_daemon_inner(&DAEMON);
    panel::select_backend()?;
    let icon = panel::tray_rgba().ok_or("the tray picture could not be decoded")?;
    panel::add_icon(&icon.0, icon.1, icon.2, "Claude RPC")?;
    check_update(false);
    spawn_status_watcher();
    std::thread::spawn(repair_start_on_windows);

    // Normally the app starts quietly in the tray.
    if std::env::args().any(|arg| arg == "--settings") {
        open_settings();
    }
    let open_requests = slint::Timer::default();
    open_requests.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(500),
        move || {
            if fs::remove_file(&open_request).is_ok() {
                open_settings();
            }
        },
    );

    slint::run_event_loop_until_quit()?;

    // "Quit" can come while a change still waits for its delayed save.
    with_settings(SettingsUi::save_now);
    SETTINGS.with(|slot| slot.borrow_mut().take());
    panel::shutdown();
    // The daemon clears the Discord activity when it stops; wait for that, but
    // never more than 2 s (an OAuth or IPC call can be slow).
    join_with_timeout(request_daemon_stop(&DAEMON), 2_000);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_startup_approved_state() {
        let query = |data: &str| {
            format!(
                "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run\r\n    Claude RPC    REG_BINARY    {data}\r\n\r\n"
            )
        };
        assert_eq!(
            parse_startup_approved(&query("030000F15D04ECD860DD01")),
            Some(false)
        );
        assert_eq!(
            parse_startup_approved(&query("020000000000000000000000")),
            Some(true)
        );
        assert_eq!(
            parse_startup_approved(&query("060000000000000000000000")),
            Some(true)
        );
        assert_eq!(parse_startup_approved(""), None);
        assert_eq!(
            parse_startup_approved(
                "ERROR: The system was unable to find the specified registry key or value."
            ),
            None
        );
    }

    #[test]
    fn instance_lock_rejects_duplicates_and_releases_on_drop() {
        let path = std::env::temp_dir().join(format!(
            "claude-rpc-instance-test-{}.lock",
            std::process::id()
        ));
        let first = acquire_instance_lock(&path).unwrap().unwrap();
        assert!(acquire_instance_lock(&path).unwrap().is_none());
        drop(first);
        let next = acquire_instance_lock(&path).unwrap().unwrap();
        drop(next);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn compares_release_versions() {
        assert!(is_newer("4.1.0", "4.0.5"));
        assert!(is_newer("4.10.0", "4.9.9"));
        assert!(!is_newer("4.1.0", "4.1.0"));
        assert!(!is_newer("4.0.5", "4.1.0"));
    }

    /// Two form fields wired to each other's setting would save the wrong option.
    #[test]
    fn settings_form_round_trips_every_field() {
        i_slint_backend_testing::init_no_event_loop();
        let window = SettingsWindow::new().unwrap();
        let app = window.global::<App>();
        let base = serde_json::to_value(ClaudeConfig::default()).unwrap();
        let flags: Vec<String> = base
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, value)| value.is_boolean())
            .map(|(key, _)| key.clone())
            .collect();
        assert_eq!(flags.len(), 16);
        // One flag on at a time: a swap or a missing field changes the result.
        for on in &flags {
            let mut value = base.clone();
            for flag in &flags {
                value[flag] = serde_json::json!(flag == on);
            }
            value["rpcMode"] = "competing".into();
            value["language"] = "fr".into();
            value["detailsTemplate"] = "{client} · {project}".into();
            value["stateTemplate"] = "{model} | {limits}".into();
            value["privateProjects"] = serde_json::json!(["client-acme", r"D:\work\secret"]);
            value["buttons"] = serde_json::json!([
                {"label": "A", "url": "https://a.example"},
                {"label": "B", "url": "https://b.example"},
            ]);
            let config: ClaudeConfig = serde_json::from_value(value).unwrap();
            fill_form(&app, &config, true);
            let mut read = ClaudeConfig::default();
            read_form(&app, &mut read);
            assert_eq!(
                serde_json::to_value(&read).unwrap(),
                serde_json::to_value(&config).unwrap(),
                "with only {on} on"
            );
        }
    }

    #[test]
    #[ignore = "Shows a desktop notification"]
    fn live_notification() {
        assert!(notify("Claude RPC", "Notification test"));
    }
}
