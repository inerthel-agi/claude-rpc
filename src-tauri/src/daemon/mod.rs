mod desktop;
mod ipc;
mod model;
mod presence;
mod process;
mod provider;
mod session;
mod status;
mod usage;
mod util;

use desktop::*;
pub(crate) use ipc::*;
use model::*;
use presence::*;
use process::*;
use provider::*;
use session::*;
use status::*;
pub(crate) use usage::*;
use util::*;
pub(crate) use util::{app_dir, now_ms};

use crate::config::{load_config, normalize_mode, read_config, ClaudeConfig};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
use windows::{
    core::{BOOL, PWSTR},
    Win32::{
        Foundation::{CloseHandle, FILETIME, HANDLE, HWND, LPARAM},
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                COINIT_APARTMENTTHREADED,
            },
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
        UI::{
            Accessibility::{
                CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTogglePattern,
                IUIAutomationTreeWalker, ToggleState_On, TreeScope_Descendants,
                UIA_ButtonControlTypeId, UIA_TogglePatternId,
            },
            WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible},
        },
    },
};

const DEFAULT_DISCORD_CLIENT_ID: &str = "1483898157854363799";
const SCAN_INTERVAL_MS: u64 = 250;
const RPC_REFRESH_INTERVAL_MS: u64 = 15_000;
const IDLE_GRACE_MS: u64 = 10_000;
const ACTIVITY_PLAYING: u8 = 0;
const ACTIVITY_LISTENING: u8 = 2;
const ACTIVITY_WATCHING: u8 = 3;
const ACTIVITY_COMPETING: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientType {
    Idle,
    Code,
    Desktop,
}

#[derive(Debug, Clone)]
struct DetectionResult {
    client: ClientType,
    mode: Option<String>,
    submode: Option<String>,
    model: Option<String>,
    limits_line: Option<String>,
    // Every known bucket in display order, for the tray menu's usage bars.
    // Independent of the Discord visibility toggles: the tray is private.
    limits: Vec<UsageLimitEntry>,
    provider: String,
    plan: Option<String>,
    // Working directory of the active Claude Code session (private projects,
    // the {project} template variable).
    project_dir: Option<String>,
    // [unix ms, percent] samples of the 5-hour bucket, for the tray chart.
    history_5h: Vec<[u64; 2]>,
    code_instances: usize,
    started_at_ms: Option<u64>,
}

impl Default for DetectionResult {
    fn default() -> Self {
        Self {
            client: ClientType::Idle,
            mode: None,
            submode: None,
            model: None,
            limits_line: None,
            limits: Vec::new(),
            provider: "Unknown".into(),
            plan: None,
            project_dir: None,
            history_5h: Vec::new(),
            code_instances: 0,
            started_at_ms: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct DesktopInfo {
    mode: Option<String>,
    submode: Option<String>,
    model: Option<String>,
    adaptive: bool,
    extended: bool,
    effort: Option<String>,
    limits: Vec<UsageLimitEntry>,
    // The model came from the Code composer's "Model: ..." button rather than a
    // looser UI label match.
    model_explicit: bool,
}

#[derive(Debug, Clone)]
struct SessionInfo {
    file: PathBuf,
    started_at_ms: Option<u64>,
    model: Option<String>,
    cwd: Option<String>,
}

// The process scan runs every 250 ms, but walking the Desktop UI tree and
// reading the provider files are far costlier and change rarely: cache them.
const DESKTOP_UI_TTL_MS: u64 = 2_000;
const PROVIDER_TTL_MS: u64 = 5_000;

#[derive(Default)]
pub(crate) struct StateMachine {
    desktop_cache: Option<(u64, Vec<u32>, DesktopInfo)>,
    oauth_inflight:
        Option<std::sync::mpsc::Receiver<Result<Vec<UsageLimitEntry>, OAuthFetchError>>>,
    provider_cache: Option<(u64, String, Option<String>)>,
    history_5h: Vec<[u64; 2]>,
    history_loaded: bool,
    last_non_idle: Option<DetectionResult>,
    last_non_idle_at_ms: u64,
    cached_limits: Vec<UsageLimitEntry>,
    oauth_last_attempt_ms: u64,
    oauth_backoff_until_ms: u64,
    last_session_mtime: u64,
    pending_activity_refresh: bool,
    cached_code_model: Option<String>,
    cached_code_model_session: Option<PathBuf>,
    cached_code_effort: Option<String>,
    cached_code_effort_session: Option<PathBuf>,
}

pub fn run(
    stop: Arc<AtomicBool>,
    force_refresh: Arc<AtomicBool>,
    config_path: Option<PathBuf>,
    status_path: Option<PathBuf>,
) {
    let config_path = config_path.unwrap_or_else(|| app_dir().join("config.json"));
    let status_path = status_path.unwrap_or_else(|| app_dir().join("status.txt"));
    let client_id = std::env::var("DISCORD_CLIENT_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_DISCORD_CLIENT_ID.to_string());
    let scan_interval_ms = parse_env_u64("SCAN_INTERVAL_MS", SCAN_INTERVAL_MS, 250);
    let idle_grace_ms = parse_env_u64("IDLE_GRACE_MS", IDLE_GRACE_MS, 0);

    let mut machine = StateMachine::default();
    let mut ipc: Option<DiscordIpc> = None;
    let mut last_key = String::new();
    let mut last_rpc_refresh_at = 0;
    let mut config_modified = modified_ms(&config_path);
    let mut config = read_config(&config_path);
    let mut result = detect(&mut machine, idle_grace_ms, limit_visibility(&config));
    let mut last_scan_at = 0;
    let mut last_status = Value::Null;

    while !stop.load(Ordering::SeqCst) {
        // UI "Refresh limits" button: bypass the OAuth poll interval/backoff and
        // force an immediate usage re-fetch on this iteration.
        if force_refresh.swap(false, Ordering::SeqCst) {
            machine.oauth_last_attempt_ms = 0;
            machine.oauth_backoff_until_ms = 0;
            machine.pending_activity_refresh = true;
            machine.desktop_cache = None;
            machine.provider_cache = None;
            last_scan_at = 0;
        }

        let current_modified = modified_ms(&config_path);
        if current_modified != config_modified {
            config_modified = current_modified;
            if let Ok(fresh) = load_config(&config_path) {
                config = fresh;
            }
            result = detect(&mut machine, idle_grace_ms, limit_visibility(&config));
            last_scan_at = now_ms();
            last_key.clear();
        }

        let paused = config.is_paused(now_ms());
        if ipc.is_none() && !paused {
            ipc = DiscordIpc::connect(&client_id).ok();
            if ipc.is_some() {
                last_key.clear();
            }
        }

        let now = now_ms();
        if now.saturating_sub(last_scan_at) >= scan_interval_ms {
            result = detect(&mut machine, idle_grace_ms, limit_visibility(&config));
            last_scan_at = now;
        }

        let status = build_status(
            &result,
            ipc.as_ref().and_then(|client| client.username.as_deref()),
            &config,
        );
        if status != last_status {
            write_status(&status_path, &status);
            last_status = status;
        }

        let key = presence_key(&result, &config);
        if paused {
            if let Some(client) = ipc.as_mut() {
                let _ = client.clear_activity();
            }
            ipc = None;
            last_key.clear();
            last_rpc_refresh_at = 0;
        } else if key != last_key
            || now.saturating_sub(last_rpc_refresh_at) >= RPC_REFRESH_INTERVAL_MS
        {
            if let Some(client) = ipc.as_mut() {
                let sent = match build_activity(&result, &config) {
                    Some(activity) => client.set_activity(activity),
                    None => client.clear_activity(),
                };

                if sent.is_ok() {
                    last_key = key;
                    last_rpc_refresh_at = now;
                } else {
                    ipc = None;
                    last_key.clear();
                    last_rpc_refresh_at = 0;
                }
            }
        }

        sleep_polling(&stop, 250);
    }

    if let Some(client) = ipc.as_mut() {
        let _ = client.clear_activity();
    }
    clear_status(&status_path);
}

fn detect(
    machine: &mut StateMachine,
    idle_grace_ms: u64,
    limit_visibility: LimitVisibility,
) -> DetectionResult {
    let mut desktop_found = false;
    let mut desktop_process_ids = Vec::new();
    let mut code_count = 0usize;
    let mut oldest = None;

    for process in scan_claude_processes() {
        if is_desktop_process(&process) {
            desktop_found = true;
            desktop_process_ids.push(process.process_id);
            oldest = min_option(oldest, process.creation_date_ms);
        } else if is_code_process(&process) {
            code_count += 1;
            oldest = min_option(oldest, process.creation_date_ms);
        }
    }

    // On platforms with process detection, neither a recently written log nor
    // the idle grace cache is evidence that a Claude client is still running.
    if cfg!(windows) && !desktop_found && code_count == 0 {
        // Usage limits stay tracked for the tray card even with Claude closed.
        let limits = current_limits(machine, &[]);
        return DetectionResult {
            limits,
            history_5h: machine.history_5h.clone(),
            ..DetectionResult::default()
        };
    }

    let session = read_session_info();
    if code_count == 0 {
        if let Some(session) = &session {
            if modified_ms(&session.file)
                .map(|mtime| now_ms().saturating_sub(mtime) <= 15_000)
                .unwrap_or(false)
            {
                code_count = 1;
                oldest = min_option(oldest, session.started_at_ms);
            }
        }
    }

    let client = if desktop_found {
        ClientType::Desktop
    } else if code_count > 0 {
        ClientType::Code
    } else {
        ClientType::Idle
    };

    // Any live Claude client (Desktop included, even with no churning CLI session
    // file) counts as activity, so OAuth usage polls at the 60s cadence instead of
    // the 10-minute idle floor and the displayed percentages stay fresh.
    if client != ClientType::Idle {
        machine.pending_activity_refresh = true;
    }

    let desktop = if client == ClientType::Desktop {
        cached_desktop_info(machine, &desktop_process_ids)
    } else {
        DesktopInfo::default()
    };
    if let Some(session) = &session {
        if let Some(mtime) = modified_ms(&session.file) {
            if machine.last_session_mtime != 0 && mtime != machine.last_session_mtime {
                machine.pending_activity_refresh = true;
            }
            machine.last_session_mtime = mtime;
        }
    }
    let limits = current_limits(machine, &desktop.limits);
    let limits_line = if limit_visibility.enabled {
        limits_line(&limits, limit_visibility)
    } else {
        None
    };
    let desktop_model = if client == ClientType::Desktop {
        detect_desktop_model(&desktop, session.as_ref())
    } else {
        None
    };

    let (provider, plan) = cached_provider(machine);
    let mut result = DetectionResult {
        client,
        mode: desktop.mode,
        submode: desktop.submode,
        model: match client {
            ClientType::Desktop => desktop_model,
            ClientType::Code => resolve_code_model(machine, session.as_ref()),
            ClientType::Idle => None,
        },
        limits_line,
        limits,
        plan,
        provider,
        project_dir: session.as_ref().and_then(|session| session.cwd.clone()),
        history_5h: machine.history_5h.clone(),
        code_instances: code_count,
        started_at_ms: oldest
            .or_else(|| session.as_ref().and_then(|session| session.started_at_ms)),
    };

    if client == ClientType::Code {
        let effort = resolve_code_effort(machine, session.as_ref());
        result.model = append_code_effort(result.model, effort);
    }

    let now = now_ms();
    if result.client != ClientType::Idle {
        machine.last_non_idle = Some(result.clone());
        machine.last_non_idle_at_ms = now;
        return result;
    }

    if now.saturating_sub(machine.last_non_idle_at_ms) <= idle_grace_ms {
        if let Some(last) = machine.last_non_idle.clone() {
            return last;
        }
    }

    DetectionResult {
        provider: result.provider,
        limits: result.limits,
        history_5h: result.history_5h,
        ..DetectionResult::default()
    }
}

fn cached_desktop_info(machine: &mut StateMachine, process_ids: &[u32]) -> DesktopInfo {
    let now = now_ms();
    if let Some((at, ids, info)) = &machine.desktop_cache {
        if ids == process_ids && now.saturating_sub(*at) < DESKTOP_UI_TTL_MS {
            return info.clone();
        }
    }
    let info = read_desktop_info(process_ids);
    machine.desktop_cache = Some((now, process_ids.to_vec(), info.clone()));
    info
}

fn cached_provider(machine: &mut StateMachine) -> (String, Option<String>) {
    let now = now_ms();
    if let Some((at, provider, plan)) = &machine.provider_cache {
        if now.saturating_sub(*at) < PROVIDER_TTL_MS {
            return (provider.clone(), plan.clone());
        }
    }
    let provider = detect_provider();
    let plan = (provider == "Subscription").then(detect_plan).flatten();
    machine.provider_cache = Some((now, provider.clone(), plan.clone()));
    (provider, plan)
}

// Plain-text snapshot of what detection sees, for the settings "Copy
// diagnostic" button. No chat text, paths or credentials: process names,
// detected labels and which source won.
pub(crate) fn diagnostic_report() -> String {
    let mut lines = Vec::new();
    let processes = scan_claude_processes();
    lines.push(format!("Claude processes: {}", processes.len()));
    let mut desktop_ids = Vec::new();
    for process in &processes {
        let kind = if is_desktop_process(process) {
            desktop_ids.push(process.process_id);
            "desktop"
        } else if is_code_process(process) {
            "code"
        } else {
            "ignored"
        };
        lines.push(format!(
            "  {} (pid {}) -> {kind}",
            process.name, process.process_id
        ));
    }
    let session = read_session_info();
    if desktop_ids.is_empty() {
        lines.push("Desktop: not running".into());
    } else {
        let info = read_desktop_info(&desktop_ids);
        lines.push(format!(
            "Desktop: mode {:?}, UI model {:?} (Model button: {}), effort {:?}, usage rows read {}",
            info.mode,
            info.model,
            info.model_explicit,
            info.effort,
            info.limits.len()
        ));
        lines.push(format!(
            "Desktop model shown: {:?}",
            detect_desktop_model(&info, session.as_ref())
        ));
    }
    match &session {
        Some(session) => lines.push(format!(
            "Latest session: {} (model {:?})",
            session
                .file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            session.model
        )),
        None => lines.push("Latest session: none in the last 24 h".into()),
    }
    let provider = detect_provider();
    let plan = (provider == "Subscription").then(detect_plan).flatten();
    lines.push(format!("Provider: {provider}, plan {plan:?}"));
    lines.join(
        "
",
    )
}
