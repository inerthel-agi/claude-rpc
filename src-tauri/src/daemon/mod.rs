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

use crate::config::{load_config, normalize_mode, ClaudeConfig};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
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
                CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
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
                UIA_ButtonControlTypeId, UIA_ControlTypePropertyId, UIA_IsOffscreenPropertyId,
                UIA_NamePropertyId, UIA_TogglePatternId,
            },
            WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible},
        },
    },
};

const DEFAULT_DISCORD_CLIENT_ID: &str = "1483898157854363799";
const SCAN_INTERVAL_MS: u64 = 1_000;
const RPC_REFRESH_INTERVAL_MS: u64 = 15_000;
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
    // Every recently active session folder: a private one hides the activity
    // even when another session's log is the latest.
    recent_project_dirs: Vec<String>,
    // Title and git branch of the active coding session (None in Chat mode or
    // when the session log is stale).
    session_title: Option<String>,
    git_branch: Option<String>,
    // [unix ms, percent] samples of the 5-hour bucket, for the tray chart.
    history_5h: Vec<[u64; 2]>,
    code_instances: usize,
    // Claude Desktop is the client and a Claude Code CLI also runs in a terminal.
    cli_alongside: bool,
    started_at_ms: Option<u64>,
    // Extra usage spend (private: tray and settings only, never Discord).
    credits: Option<UsageCredits>,
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
            recent_project_dirs: Vec::new(),
            session_title: None,
            git_branch: None,
            history_5h: Vec::new(),
            code_instances: 0,
            cli_alongside: false,
            started_at_ms: None,
            credits: None,
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
    title: Option<String>,
    branch: Option<String>,
    // Session effort from the log tail (model::session_effort), and whether that
    // tail still holds an `/effort` command.
    effort: Option<String>,
    effort_command_seen: bool,
    // Working directories of every session written in the last 10 minutes.
    recent_cwds: Vec<String>,
}

// The process scan runs every second, but walking the Desktop UI tree and
// reading the provider files are far costlier and change rarely: cache them.
// The Desktop UI cache lives between these bounds, see `desktop_ui_ttl`.
const DESKTOP_UI_TTL_MS: u64 = 2_000;
const DESKTOP_UI_MAX_TTL_MS: u64 = 15_000;
const PROVIDER_TTL_MS: u64 = 5_000;
const SESSION_TTL_MS: u64 = 1_000;

// A full Desktop accessibility tree takes 100-500 ms to walk and makes Claude
// rebuild it: keep the scan under 5 % of the time by waiting 20x its duration.
fn desktop_ui_ttl(scan_ms: u64) -> u64 {
    scan_ms
        .saturating_mul(20)
        .clamp(DESKTOP_UI_TTL_MS, DESKTOP_UI_MAX_TTL_MS)
}

#[derive(Default)]
pub(crate) struct StateMachine {
    // (stamped at, TTL, Desktop process ids, info)
    desktop_cache: Option<(u64, u64, Vec<u32>, DesktopInfo)>,
    oauth_inflight: Option<std::sync::mpsc::Receiver<Result<OAuthUsage, OAuthFetchError>>>,
    credits: Option<UsageCredits>,
    provider_cache: Option<(u64, String, Option<String>)>,
    session_cache: Option<(u64, Option<SessionInfo>)>,
    history_5h: Vec<[u64; 2]>,
    history_loaded: bool,
    cached_limits: Vec<UsageLimitEntry>,
    limits_cache_loaded: bool,
    limits_cache_written_at: u64,
    oauth_last_attempt_ms: u64,
    oauth_backoff_until_ms: u64,
    // Credentials mtime seen when the token endpoint refused the refresh
    // token; no fetch starts while the file still has it.
    oauth_refused_mtime: Option<Option<u64>>,
    oauth_sign_in_checked_ms: u64,
    // Credentials mtime when the in-flight request started.
    oauth_attempt_mtime: Option<u64>,
    last_session_mtime: u64,
    pending_activity_refresh: bool,
    cached_code_model: Option<String>,
    cached_code_model_session: Option<PathBuf>,
    cached_code_effort: Option<String>,
    cached_code_effort_session: Option<PathBuf>,
    // The title is re-logged often but can scroll out of the read tail.
    cached_session_title: Option<(PathBuf, String)>,
    session_prompts: PromptCache,
    process_cache: ProcessCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiscordState {
    Connected,
    Disconnected,
    Paused,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResetKind {
    None,
    Iso,
    Text,
}

#[derive(Debug, Clone)]
struct DiagBucket {
    label: String,
    used_percent: u8,
    updated_at_ms: u64,
    reset: ResetKind,
}

// Daemon-thread state that diagnostic_report() cannot otherwise see: the
// daemon records it here and the report copies it.
#[derive(Debug, Clone)]
struct DaemonDiag {
    oauth: Option<(Result<(), OAuthFetchError>, u64)>,
    oauth_backoff_until_ms: u64,
    oauth_blocked: bool,
    discord: Option<(DiscordState, u64)>,
    config_error: Option<String>,
    buckets: Vec<DiagBucket>,
}

static DIAG: Mutex<DaemonDiag> = Mutex::new(DaemonDiag {
    oauth: None,
    oauth_backoff_until_ms: 0,
    oauth_blocked: false,
    discord: None,
    config_error: None,
    buckets: Vec::new(),
});

fn diag() -> MutexGuard<'static, DaemonDiag> {
    DIAG.lock().unwrap_or_else(|err| err.into_inner())
}

fn record_discord_state(state: DiscordState) {
    let mut diag = diag();
    if diag.discord.map(|(current, _)| current) != Some(state) {
        diag.discord = Some((state, now_ms()));
    }
}

fn record_oauth_outcome(
    outcome: Result<(), OAuthFetchError>,
    at_ms: u64,
    backoff_until_ms: u64,
    blocked: bool,
) {
    let mut diag = diag();
    diag.oauth = Some((outcome, at_ms));
    diag.oauth_backoff_until_ms = backoff_until_ms;
    diag.oauth_blocked = blocked;
}

// An ISO reset comes from the OAuth API; Desktop shows free text.
fn reset_kind(reset: Option<&str>) -> ResetKind {
    match reset {
        None => ResetKind::None,
        Some(reset) if reset.contains('T') && parse_iso_ms(reset).is_some() => ResetKind::Iso,
        Some(_) => ResetKind::Text,
    }
}

fn record_limit_buckets(entries: &[UsageLimitEntry]) {
    let buckets = entries
        .iter()
        .map(|entry| DiagBucket {
            label: entry.label.clone(),
            used_percent: entry.used_percent,
            updated_at_ms: entry.updated_at_ms,
            reset: reset_kind(entry.reset.as_deref()),
        })
        .collect();
    diag().buckets = buckets;
}

// serde errors quote the offending value, which can be a private project path.
fn redact_quoted(text: &str) -> String {
    let mut out = String::new();
    let mut open = None;
    let mut escaped = false;
    for ch in text.chars() {
        match open {
            None => {
                out.push(ch);
                if matches!(ch, '"' | '`') {
                    open = Some(ch);
                }
            }
            Some(_) if escaped => escaped = false,
            Some(_) if ch == '\\' => escaped = true,
            Some(quote) if ch == quote => {
                out.push_str("...");
                out.push(ch);
                open = None;
            }
            Some(_) => {}
        }
    }
    out
}

fn format_diag(diag: &DaemonDiag, now: u64) -> Vec<String> {
    let ago = |at: u64| format!("{} s ago", now.saturating_sub(at) / 1_000);
    let mut lines = Vec::new();
    lines.push(match &diag.oauth {
        None => "OAuth usage: never".to_string(),
        Some((outcome, at)) => {
            let outcome = match outcome {
                Ok(()) => "Ok".to_string(),
                Err(error) => error.to_string(),
            };
            let mut line = format!("OAuth usage: {outcome}, {}", ago(*at));
            if diag.oauth_blocked {
                line.push_str(", waiting for a new Claude Code sign-in");
            } else if diag.oauth_backoff_until_ms > now {
                let wait = (diag.oauth_backoff_until_ms - now).div_ceil(1_000);
                line.push_str(&format!(", retry in {wait} s"));
            }
            line
        }
    });
    lines.push(match diag.discord {
        None => "Discord: no state yet".to_string(),
        Some((state, at)) => {
            let state = format!("{state:?}").to_lowercase();
            format!("Discord: {state}, changed {}", ago(at))
        }
    });
    lines.push(match &diag.config_error {
        None => "Config: loaded".to_string(),
        Some(error) => format!("Config: load error ({})", redact_quoted(error)),
    });
    if diag.buckets.is_empty() {
        lines.push("Usage buckets: none known".into());
    }
    for bucket in &diag.buckets {
        let updated = if bucket.updated_at_ms == 0 {
            "never".to_string()
        } else {
            ago(bucket.updated_at_ms)
        };
        let reset = match bucket.reset {
            ResetKind::None => "none",
            ResetKind::Iso => "ISO (OAuth)",
            ResetKind::Text => "text (Desktop)",
        };
        lines.push(format!(
            "Usage bucket {}: {}%, updated {updated}, reset {reset}",
            bucket.label, bucket.used_percent
        ));
    }
    lines
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

    let mut machine = StateMachine::default();
    let mut ipc: Option<DiscordIpc> = None;
    let mut last_key = String::new();
    let mut last_rpc_refresh_at = 0;
    let mut config_modified = modified_ms(&config_path);
    let mut config = ClaudeConfig::default();
    // Same as config::load_or_paused, without reading the file twice. An
    // unreadable file is read again every second (it may be mid-write).
    if !apply_loaded_config(&mut config, load_config(&config_path)) {
        config_modified = None;
    }
    let mut config_retry_at = 0;
    let mut result = detect(&mut machine, limit_visibility(&config));
    let mut last_scan_at = 0;
    let mut last_status = Value::Null;

    while !stop.load(Ordering::SeqCst) {
        // UI "Refresh limits" button: bypass the OAuth poll interval/backoff and
        // force an immediate usage re-fetch on this iteration.
        if force_refresh.swap(false, Ordering::SeqCst) {
            machine.oauth_last_attempt_ms = 0;
            machine.oauth_backoff_until_ms = 0;
            machine.oauth_refused_mtime = None;
            diag().oauth_backoff_until_ms = 0;
            diag().oauth_blocked = false;
            machine.pending_activity_refresh = true;
            machine.desktop_cache = None;
            machine.provider_cache = None;
            machine.session_cache = None;
            last_scan_at = 0;
        }

        let current_modified = modified_ms(&config_path);
        if current_modified != config_modified && now_ms() >= config_retry_at {
            if apply_loaded_config(&mut config, load_config(&config_path)) {
                config_modified = current_modified;
            } else {
                config_retry_at = now_ms() + 1_000;
            }
            result = detect(&mut machine, limit_visibility(&config));
            last_scan_at = now_ms();
            last_key.clear();
        }

        let paused = config.is_paused(now_ms());
        if ipc.is_none() && !paused {
            ipc = DiscordIpc::connect(&client_id).ok();
            if ipc.is_some() {
                last_key.clear();
                record_discord_state(DiscordState::Connected);
            } else {
                record_discord_state(DiscordState::Disconnected);
            }
        }

        let now = now_ms();
        if now.saturating_sub(last_scan_at) >= scan_interval_ms {
            result = detect(&mut machine, limit_visibility(&config));
            last_scan_at = now;
        }

        let status = build_status(
            &result,
            ipc.as_ref().and_then(|client| client.username.as_deref()),
            &config,
        );
        // A failed write (file held open by antivirus/indexer) is retried on
        // the next tick instead of being recorded as done.
        if status != last_status && write_status(&status_path, &status) {
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
            record_discord_state(DiscordState::Paused);
        } else if key != last_key
            || now.saturating_sub(last_rpc_refresh_at) >= RPC_REFRESH_INTERVAL_MS
        {
            if let Some(client) = ipc.as_mut() {
                let sent = match build_activity(&result, &config) {
                    Some(activity) => client.set_activity(activity),
                    None => client.clear_activity(),
                };

                match sent {
                    Ok(()) => {
                        last_key = key;
                        last_rpc_refresh_at = now;
                        record_discord_state(DiscordState::Connected);
                    }
                    // Discord answered but rejected this activity: keep the
                    // connection and wait for the next change or refresh
                    // instead of reconnecting every 250 ms.
                    Err(err) if err.kind() == std::io::ErrorKind::InvalidData => {
                        last_key = key;
                        last_rpc_refresh_at = now;
                        record_discord_state(DiscordState::Rejected);
                    }
                    Err(_) => {
                        ipc = None;
                        last_key.clear();
                        last_rpc_refresh_at = 0;
                        record_discord_state(DiscordState::Disconnected);
                    }
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

fn detect(machine: &mut StateMachine, limit_visibility: LimitVisibility) -> DetectionResult {
    let mut desktop_found = false;
    let mut desktop_process_ids = Vec::new();
    // Every Claude Code process (it makes Claude Code the client when Desktop
    // is closed), and among them the Desktop Code-tab sessions and the CLIs
    // started in a terminal; IDE and SDK/background workers are neither.
    let mut code_count = 0usize;
    let mut desktop_sessions = 0usize;
    let mut cli_count = 0usize;
    let mut oldest = None;

    for process in scan_claude_processes(&mut machine.process_cache) {
        if is_desktop_process(&process) {
            desktop_found = true;
            desktop_process_ids.push(process.process_id);
            oldest = min_option(oldest, process.creation_date_ms);
        } else if is_code_process(&process) {
            code_count += 1;
            if is_desktop_code_session(&process) {
                desktop_sessions += 1;
            } else if is_interactive_cli(&process) {
                cli_count += 1;
            }
            oldest = min_option(oldest, process.creation_date_ms);
        }
    }

    // On platforms with process detection, a recently written log is not
    // evidence that a Claude client is still running.
    if cfg!(windows) && !desktop_found && code_count == 0 {
        // Usage limits and the plan stay tracked for the tray card even with
        // Claude closed.
        let (provider, plan) = cached_provider(machine);
        let limits = current_limits(machine, &[], plan_has_5h_limit(plan.as_deref()));
        return DetectionResult {
            provider,
            plan,
            limits,
            history_5h: machine.history_5h.clone(),
            credits: fresh_credits(machine, now_ms()),
            ..DetectionResult::default()
        };
    }

    let session = cached_session_info(machine);
    if code_count == 0 {
        if let Some(session) = &session {
            if modified_ms(&session.file)
                .map(|mtime| now_ms().saturating_sub(mtime) <= 15_000)
                .unwrap_or(false)
            {
                code_count = 1;
                // With Desktop open, a fresh log is most likely its Code tab.
                if !desktop_found {
                    cli_count = 1;
                }
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
    let (provider, plan) = cached_provider(machine);
    let limits = current_limits(machine, &desktop.limits, plan_has_5h_limit(plan.as_deref()));
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
        recent_project_dirs: session
            .as_ref()
            .map(|session| session.recent_cwds.clone())
            .unwrap_or_default(),
        session_title: None,
        git_branch: None,
        history_5h: machine.history_5h.clone(),
        // Sessions shown on the profile: Desktop Code tabs plus terminal CLIs.
        code_instances: if client == ClientType::Code {
            cli_count.max(1)
        } else {
            desktop_sessions + cli_count
        },
        cli_alongside: client == ClientType::Desktop && cli_count > 0,
        started_at_ms: activity_start(
            session.as_ref().and_then(|session| session.started_at_ms),
            session
                .as_ref()
                .and_then(|session| modified_ms(&session.file)),
            oldest,
            now_ms(),
        ),
        credits: fresh_credits(machine, now_ms()),
    };

    if client == ClientType::Code {
        let effort = resolve_code_effort(machine, session.as_ref());
        result.model = append_code_effort(result.model, effort);
    }
    if client == ClientType::Desktop {
        let effort = resolve_session_effort(machine, session.as_ref());
        result.model = desktop_model_with_session_effort(
            result.model,
            desktop.effort.as_deref(),
            result.mode.as_deref(),
            effort,
            result.cli_alongside,
        );
    }

    let coding = client == ClientType::Code || result.mode.as_deref() == Some("Code");
    if let Some(session) = session.as_ref().filter(|session| {
        coding
            && modified_ms(&session.file)
                .is_some_and(|mtime| now_ms().saturating_sub(mtime) <= ACTIVE_SESSION_MS)
    }) {
        result.session_title = resolve_session_title(machine, session);
        result.git_branch = session.branch.clone();
    }

    if result.client != ClientType::Idle {
        return result;
    }

    DetectionResult {
        provider: result.provider,
        plan: result.plan,
        limits: result.limits,
        history_5h: result.history_5h,
        credits: result.credits,
        ..DetectionResult::default()
    }
}

// A readable config.json replaces the settings; an unreadable one pauses
// publishing (the settings window says nothing is published) and keeps the rest.
// Returns whether the file was read.
fn apply_loaded_config(config: &mut ClaudeConfig, loaded: Result<ClaudeConfig, String>) -> bool {
    diag().config_error = loaded.as_ref().err().cloned();
    match loaded {
        Ok(fresh) => {
            *config = fresh;
            true
        }
        Err(_) => {
            config.dnd = true;
            false
        }
    }
}

fn cached_desktop_info(machine: &mut StateMachine, process_ids: &[u32]) -> DesktopInfo {
    let now = now_ms();
    if let Some((at, ttl, ids, info)) = &machine.desktop_cache {
        if ids == process_ids && now.saturating_sub(*at) < *ttl {
            return info.clone();
        }
    }
    let start = std::time::Instant::now();
    let info = read_desktop_info(process_ids, None);
    let ttl = desktop_ui_ttl(start.elapsed().as_millis() as u64);
    // Stamp after the read: a slow UI Automation walk (long chat, busy Claude
    // window) must not leave the cache already expired and rescan every tick.
    machine.desktop_cache = Some((now_ms(), ttl, process_ids.to_vec(), info.clone()));
    info
}

// A session log written within this delay is the session being worked in.
const ACTIVE_SESSION_MS: u64 = 60 * 60 * 1_000;

// Discord's "elapsed" timer: from the start of the Claude Code session in use,
// else (Chat mode, no recent session) from the oldest running Claude client.
fn activity_start(
    session_start: Option<u64>,
    session_modified: Option<u64>,
    oldest_process: Option<u64>,
    now: u64,
) -> Option<u64> {
    session_start
        .filter(|start| *start <= now)
        .filter(|_| {
            session_modified.is_some_and(|mtime| now.saturating_sub(mtime) <= ACTIVE_SESSION_MS)
        })
        .or(oldest_process)
}

fn resolve_session_title(machine: &mut StateMachine, session: &SessionInfo) -> Option<String> {
    match &session.title {
        Some(title) => {
            machine.cached_session_title = Some((session.file.clone(), title.clone()));
            Some(title.clone())
        }
        None => machine
            .cached_session_title
            .as_ref()
            .filter(|(file, _)| *file == session.file)
            .map(|(_, title)| title.clone()),
    }
}

// The session folder walk and log tails cost far more than a 250 ms tick:
// reuse the result for a second.
fn cached_session_info(machine: &mut StateMachine) -> Option<SessionInfo> {
    let now = now_ms();
    if let Some((at, info)) = &machine.session_cache {
        if now.saturating_sub(*at) < SESSION_TTL_MS {
            return info.clone();
        }
    }
    let info = read_session_info(&mut machine.session_prompts);
    machine.session_cache = Some((now_ms(), info.clone()));
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
    let processes = scan_claude_processes(&mut ProcessCache::new());
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
    let session = read_session_info(&mut PromptCache::new());
    if desktop_ids.is_empty() {
        lines.push("Desktop: not running".into());
    } else {
        let mut scan = DesktopScanDebug::default();
        let info = read_desktop_info(&desktop_ids, Some(&mut scan));
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
        lines.extend(format_scan_debug(&scan));
    }
    match &session {
        Some(session) => lines.push(format!(
            "Latest session: {} (model {:?}, effort {:?})",
            session
                .file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            session.model,
            session.effort
        )),
        None => lines.push("Latest session: none in the last 24 h".into()),
    }
    let provider = detect_provider();
    let plan = (provider == "Subscription").then(detect_plan).flatten();
    lines.push(format!("Provider: {provider}, plan {plan:?}"));
    let snapshot = diag().clone();
    lines.extend(format_diag(&snapshot, now_ms()));
    lines.join(
        "
",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_starts_with_the_active_session() {
        let now = 10 * 60 * 60 * 1_000;
        let desktop_opened = Some(now - 9 * 60 * 60 * 1_000);
        let session_start = Some(now - 20 * 60 * 1_000);
        // Session log written a minute ago: its start wins.
        assert_eq!(
            activity_start(session_start, Some(now - 60_000), desktop_opened, now),
            session_start
        );
        // Last written two hours ago (Chat mode now): back to the client start.
        assert_eq!(
            activity_start(
                session_start,
                Some(now - 2 * ACTIVE_SESSION_MS),
                desktop_opened,
                now
            ),
            desktop_opened
        );
        // No session, or a start in the future (clock skew): client start.
        assert_eq!(
            activity_start(None, Some(now), desktop_opened, now),
            desktop_opened
        );
        assert_eq!(
            activity_start(Some(now + 5_000), Some(now), desktop_opened, now),
            desktop_opened
        );
    }

    #[test]
    fn formats_daemon_diagnostic() {
        let now = 1_000_000;
        let empty = DaemonDiag {
            oauth: None,
            oauth_backoff_until_ms: 0,
            oauth_blocked: false,
            discord: None,
            config_error: None,
            buckets: Vec::new(),
        };
        assert_eq!(
            format_diag(&empty, now),
            [
                "OAuth usage: never",
                "Discord: no state yet",
                "Config: loaded",
                "Usage buckets: none known",
            ]
        );

        let bucket = |label: &str, reset| DiagBucket {
            label: label.into(),
            used_percent: 42,
            updated_at_ms: now - 30_000,
            reset,
        };
        let failing = DaemonDiag {
            oauth: Some((Err(OAuthFetchError::Unauthorized(None)), now - 412_000)),
            oauth_backoff_until_ms: now + 188_000,
            oauth_blocked: false,
            discord: Some((DiscordState::Rejected, now - 5_000)),
            config_error: Some(
                r#"invalid type: string "D:\\private\"x", expected a sequence at line 3 column 20"#
                    .into(),
            ),
            buckets: vec![
                bucket("5h", ResetKind::Iso),
                bucket("All", ResetKind::Text),
                bucket("Fable", ResetKind::None),
            ],
        };
        assert_eq!(
            format_diag(&failing, now),
            [
                "OAuth usage: Unauthorized, 412 s ago, retry in 188 s",
                "Discord: rejected, changed 5 s ago",
                r#"Config: load error (invalid type: string "...", expected a sequence at line 3 column 20)"#,
                "Usage bucket 5h: 42%, updated 30 s ago, reset ISO (OAuth)",
                "Usage bucket All: 42%, updated 30 s ago, reset text (Desktop)",
                "Usage bucket Fable: 42%, updated 30 s ago, reset none",
            ]
        );

        // A successful fetch whose backoff has already passed shows no retry.
        let ok = DaemonDiag {
            oauth: Some((Ok(()), now - 1_000)),
            oauth_backoff_until_ms: now - 1,
            ..empty
        };
        assert_eq!(format_diag(&ok, now)[0], "OAuth usage: Ok, 1 s ago");

        // A refused renewal waits for the credentials to change, not a timer.
        let blocked = DaemonDiag {
            oauth: Some((
                Err(OAuthFetchError::NoToken(
                    TokenError::Expired,
                    RenewError::Status(400, OAuthErrorCode::InvalidGrant),
                )),
                now - 148_000,
            )),
            oauth_backoff_until_ms: now + 452_000,
            oauth_blocked: true,
            ..ok
        };
        assert_eq!(
            format_diag(&blocked, now)[0],
            "OAuth usage: NoToken (token expired; renewal refused: HTTP 400 invalid_grant), \
             148 s ago, waiting for a new Claude Code sign-in"
        );
    }

    #[test]
    fn desktop_ui_ttl_bounds_scan_duty_cycle() {
        assert_eq!(desktop_ui_ttl(0), 2_000);
        assert_eq!(desktop_ui_ttl(10), 2_000);
        assert_eq!(desktop_ui_ttl(223), 4_460);
        assert_eq!(desktop_ui_ttl(1_000), 15_000);
    }

    #[test]
    fn an_unreadable_config_pauses_publishing() {
        let mut config = ClaudeConfig {
            private_projects: vec!["secret".into()],
            ..ClaudeConfig::default()
        };
        assert!(!apply_loaded_config(&mut config, Err("bad json".into())));
        assert!(config.dnd);
        assert_eq!(config.private_projects, ["secret"]);
        // Readable again: its own settings apply, DND included.
        assert!(apply_loaded_config(
            &mut config,
            Ok(ClaudeConfig::default())
        ));
        assert!(!config.dnd);
    }

    #[test]
    fn classifies_bucket_resets() {
        assert_eq!(reset_kind(None), ResetKind::None);
        assert_eq!(
            reset_kind(Some("2026-09-25T20:19:59+00:00")),
            ResetKind::Iso
        );
        assert_eq!(reset_kind(Some("in 4 hr 33 min")), ResetKind::Text);
        assert_eq!(reset_kind(Some("Tue 9:00 AM")), ResetKind::Text);
    }

    // Manual profiling on a real machine; prints timings, asserts nothing:
    // cargo test --release --manifest-path src-tauri/Cargo.toml -- --ignored profile_detection_stages --nocapture
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn profile_detection_stages() {
        use std::time::Instant;

        fn time_ms<T>(f: impl FnOnce() -> T) -> (f64, T) {
            let start = Instant::now();
            let value = f();
            (start.elapsed().as_secs_f64() * 1000.0, value)
        }
        // Prints the stats and returns the implied ms of work per second.
        fn report(label: &str, samples: &[f64], per_second: f64) -> f64 {
            let avg = samples.iter().sum::<f64>() / samples.len() as f64;
            let min = samples.iter().copied().fold(f64::INFINITY, f64::min);
            let max = samples.iter().copied().fold(0.0, f64::max);
            let implied = avg * per_second;
            println!(
                "{label}: runs {}, avg {avg:.3} ms, min {min:.3} ms, max {max:.3} ms, \
                 at {per_second}/s -> {implied:.3} ms/s",
                samples.len()
            );
            implied
        }

        // 1. Toolhelp process list alone.
        let samples: Vec<f64> = (0..40).map(|_| time_ms(list_process_entries).0).collect();
        report("1 list_process_entries", &samples, 1.0);

        // 2. Claude process scan with one persistent cache.
        let mut cache = ProcessCache::new();
        let (cold, snapshots) = time_ms(|| scan_claude_processes(&mut cache));
        println!(
            "2 scan_claude_processes cold: {cold:.3} ms, snapshots {}",
            snapshots.len()
        );
        let mut snapshots = snapshots;
        let samples: Vec<f64> = (0..40)
            .map(|_| {
                let (ms, found) = time_ms(|| scan_claude_processes(&mut cache));
                snapshots = found;
                ms
            })
            .collect();
        println!(
            "2 scan_claude_processes warm snapshots: {}",
            snapshots.len()
        );
        let scan = report("2 scan_claude_processes warm", &samples, 1.0);
        let desktop_pids: Vec<u32> = snapshots
            .iter()
            .filter(|process| is_desktop_process(process))
            .map(|process| process.process_id)
            .collect();

        // 3. Session log candidate listing alone.
        let root = claude_dir().join("projects");
        let mut candidates = 0;
        let samples: Vec<f64> = (0..10)
            .map(|_| {
                let (ms, found) = time_ms(|| collect_jsonl_candidates(&root, 24 * 60 * 60 * 1000));
                candidates = found.len();
                ms
            })
            .collect();
        println!("3 collect_jsonl_candidates candidates: {candidates}");
        report("3 collect_jsonl_candidates", &samples, 1.0);

        // 4. Full session read with one persistent prompt cache.
        let mut prompts = PromptCache::new();
        let (cold, session) = time_ms(|| read_session_info(&mut prompts));
        println!(
            "4 read_session_info cold: {cold:.3} ms, session found {}",
            session.is_some()
        );
        let samples: Vec<f64> = (0..10)
            .map(|_| time_ms(|| read_session_info(&mut prompts)).0)
            .collect();
        let session = report("4 read_session_info warm", &samples, 1.0);

        // 5. Desktop config + UI Automation read.
        let desktop = if desktop_pids.is_empty() {
            println!("5 read_desktop_info: skipped, no Desktop process running");
            0.0
        } else {
            let samples: Vec<f64> = (0..5)
                .map(|_| time_ms(|| read_desktop_info(&desktop_pids, None)).0)
                .collect();
            let avg = samples.iter().sum::<f64>() / samples.len() as f64;
            let per_second = 1000.0 / desktop_ui_ttl(avg as u64) as f64;
            report("5 read_desktop_info", &samples, per_second)
        };

        // Stage 1 runs inside stage 2 and stage 3 inside stage 4, so they are
        // left out of the sum to avoid counting them twice.
        let total = scan + session + desktop;
        println!(
            "Total (stages 2+4+5): {total:.3} ms/s = {:.3} % of one core",
            total / 10.0
        );
        println!("{}", diagnostic_report());
    }
}
