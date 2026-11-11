use super::*;

// What the Desktop UI walk saw, for the diagnostic report only.
#[derive(Debug, Default)]
pub(super) struct DesktopScanDebug {
    elements: i32,
    scanned: i32,
    duration_ms: u128,
    // Top model candidates by score, best first.
    candidates: Vec<ScanCandidate>,
    // Wording of the "Usage: ..." buttons, so the report shows what the limits
    // parser has to match; see usage_label_shape.
    usage_labels: Vec<String>,
}

#[derive(Debug)]
struct ScanCandidate {
    score: i32,
    is_button: bool,
    offscreen: bool,
    chars: usize,
    // The label itself, or "<text, N chars>" when it may be chat text.
    label: String,
}

pub(super) fn format_scan_debug(scan: &DesktopScanDebug) -> Vec<String> {
    let yes_no = |value: bool| if value { "yes" } else { "no" };
    let mut lines = vec![format!(
        "Desktop UI scan: {} elements, {} scanned, {} ms",
        scan.elements, scan.scanned, scan.duration_ms
    )];
    for candidate in &scan.candidates {
        lines.push(format!(
            "  model candidate score {} (button {}, offscreen {}, {} chars): {}",
            candidate.score,
            yes_no(candidate.is_button),
            yes_no(candidate.offscreen),
            candidate.chars,
            candidate.label
        ));
    }
    for label in &scan.usage_labels {
        lines.push(format!("  usage label: {label}"));
    }
    lines
}

#[cfg(any(windows, test))]
fn model_candidate_score(
    explicit_model: bool,
    has_effort: bool,
    offscreen: bool,
    is_button: bool,
) -> i32 {
    (if explicit_model { 20 } else { 0 })
        + if has_effort { 4 } else { 0 }
        + if !offscreen { 3 } else { 1 }
        + if is_button { 2 } else { 0 }
}

// True when nothing but model-picker vocabulary is left: any other word may
// be a conversation title or a chat message, which must never be reported.
#[cfg(any(windows, test))]
fn is_pure_model_label(label: &str) -> bool {
    const WORDS: &[&str] = &[
        "claude",
        "model",
        "fable",
        "opus",
        "sonnet",
        "haiku",
        "plan",
        "1m",
        "context",
        "adaptive",
        "extended",
        "thinking",
        "low",
        "medium",
        "high",
        "extra",
        "xhigh",
        "max",
        "ultracode",
        "default",
    ];
    label
        .to_lowercase()
        .split(|ch: char| ch.is_whitespace() || ch.is_ascii_punctuation())
        .filter(|token| !token.is_empty())
        .all(|token| token.chars().all(|ch| ch.is_ascii_digit()) || WORDS.contains(&token))
}

// A usage label with every word outside the usage-button vocabulary replaced
// by "?": a chat line that starts with "Usage:" can never reach the report.
#[cfg(any(windows, test))]
fn usage_label_shape(label: &str) -> String {
    const WORDS: &[&str] = &[
        "usage", "context", "of", "5-hour", "hour", "limit", "weekly", "all", "models", "fable",
        "opus", "sonnet", "haiku", "current", "session", "resets", "in", "at", "hr", "hrs", "min",
        "mins", "day", "days", "am", "pm", "mon", "tue", "wed", "thu", "fri", "sat", "sun", "·",
        "/",
    ];
    let known = |token: &str| {
        let core = token
            .trim_matches(|ch: char| matches!(ch, '(' | ')' | ',' | ':'))
            .to_lowercase();
        core.is_empty()
            || WORDS.contains(&core.as_str())
            || (core.chars().any(|ch| ch.is_ascii_digit())
                && core
                    .chars()
                    .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ':' | '%' | 'k' | 'm')))
    };
    label
        .split_whitespace()
        .map(|token| if known(token) { token } else { "?" })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(any(windows, test))]
fn candidate_label(label: &str) -> String {
    if is_pure_model_label(label) {
        label.to_string()
    } else {
        format!("<text, {} chars>", label.chars().count())
    }
}

pub(super) fn read_desktop_info(
    process_ids: &[u32],
    debug: Option<&mut DesktopScanDebug>,
) -> DesktopInfo {
    let mut info = read_desktop_config_info();
    if let Some(ui_info) = read_desktop_ui_info(process_ids, info.mode.as_deref(), debug) {
        if ui_info.mode.is_some() {
            info.mode = ui_info.mode;
        }
        if ui_info.submode.is_some() {
            info.submode = ui_info.submode;
        }
        if ui_info.model.is_some() {
            info.model = ui_info.model;
            info.model_explicit = ui_info.model_explicit;
        }
        if ui_info.effort.is_some() {
            info.effort = ui_info.effort;
        }
        if !ui_info.limits.is_empty() {
            info.limits = ui_info.limits;
        }
        info.adaptive |= ui_info.adaptive;
        info.extended |= ui_info.extended;
    }
    info
}

pub(super) fn read_desktop_config_info() -> DesktopInfo {
    let path = roaming_app_data()
        .join("Claude")
        .join("claude_desktop_config.json");
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return DesktopInfo::default(),
    };
    let value: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    let raw_mode = value
        .get("preferences")
        .and_then(|prefs| prefs.get("sidebarMode"))
        .and_then(Value::as_str);
    DesktopInfo {
        mode: raw_mode.and_then(map_desktop_mode),
        ..DesktopInfo::default()
    }
}

#[cfg(windows)]
pub(super) fn read_desktop_ui_info(
    process_ids: &[u32],
    fallback_mode: Option<&str>,
    debug: Option<&mut DesktopScanDebug>,
) -> Option<DesktopInfo> {
    let hwnd = find_desktop_window(process_ids)?;
    unsafe { read_desktop_ui_info_from_window(hwnd, fallback_mode, debug) }.ok()
}

#[cfg(windows)]
thread_local! {
    static COM_INITIALIZED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static UI_AUTOMATION: std::cell::RefCell<Option<IUIAutomation>> =
        const { std::cell::RefCell::new(None) };
}

// Setting up COM and the UI Automation object cost more than walking a small
// Desktop tree: do it once per thread. COM stays initialized until the thread
// ends; a failed creation is retried on the next scan.
#[cfg(windows)]
fn ui_automation() -> windows::core::Result<IUIAutomation> {
    if !COM_INITIALIZED.get() {
        // S_FALSE (already initialized in this mode) is a success; a changed
        // mode is an error.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
        COM_INITIALIZED.set(true);
    }
    if let Some(automation) = UI_AUTOMATION.with_borrow(Option::clone) {
        return Ok(automation);
    }
    let automation: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }?;
    UI_AUTOMATION.set(Some(automation.clone()));
    Ok(automation)
}

#[cfg(not(windows))]
pub(super) fn read_desktop_ui_info(
    _process_ids: &[u32],
    _fallback_mode: Option<&str>,
    _debug: Option<&mut DesktopScanDebug>,
) -> Option<DesktopInfo> {
    None
}

#[cfg(windows)]
pub(super) fn find_desktop_window(process_ids: &[u32]) -> Option<HWND> {
    if process_ids.is_empty() {
        return None;
    }

    struct WindowSearch<'a> {
        process_ids: &'a [u32],
        hwnd: Option<HWND>,
    }

    unsafe extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = &mut *(lparam.0 as *mut WindowSearch);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        if search.process_ids.contains(&pid) && IsWindowVisible(hwnd).as_bool() {
            search.hwnd = Some(hwnd);
            return BOOL(0);
        }

        BOOL(1)
    }

    let mut search = WindowSearch {
        process_ids,
        hwnd: None,
    };
    let _ = unsafe {
        EnumWindows(
            Some(enum_window),
            LPARAM((&mut search as *mut WindowSearch).cast::<()>() as isize),
        )
    };
    search.hwnd
}

#[cfg(windows)]
pub(super) unsafe fn read_desktop_ui_info_from_window(
    hwnd: HWND,
    fallback_mode: Option<&str>,
    mut debug: Option<&mut DesktopScanDebug>,
) -> windows::core::Result<DesktopInfo> {
    let started = debug.is_some().then(std::time::Instant::now);
    let automation = ui_automation()?;
    let root = automation.ElementFromHandle(hwnd)?;
    let condition = automation.CreateTrueCondition()?;
    // One cross-process fetch for the three properties read below, instead of
    // three calls per element. The elements keep full references, so the
    // live toggle-pattern lookups in is_toggle_on still work on them.
    let cache_request = automation.CreateCacheRequest()?;
    cache_request.AddProperty(UIA_NamePropertyId)?;
    cache_request.AddProperty(UIA_ControlTypePropertyId)?;
    cache_request.AddProperty(UIA_IsOffscreenPropertyId)?;
    let elements = root.FindAllBuildCache(TreeScope_Descendants, &condition, &cache_request)?;
    let length = elements.Length()?;
    // The sidebar (mode markers) is at the start of the tree and the composer
    // (model, effort, usage buttons) at the end, with the chat transcript in
    // between; a long chat used to push the composer past a flat 1,500 cap.
    let head = length.min(1_500);
    let tail_start = head.max(length.saturating_sub(1_000));
    let indices = (0..head).chain(tail_start..length);

    let mut names = Vec::new();
    let mut best_model: Option<(DesktopModelCandidate, i32)> = None;
    let mut adaptive = false;
    let mut extended = false;
    let mut explicit_effort = None;

    for index in indices {
        let Ok(element) = elements.GetElement(index) else {
            continue;
        };
        let Ok(name_bstr) = element.CachedName() else {
            continue;
        };
        let name = name_bstr.to_string();
        let name = name.trim();
        if name.is_empty() {
            continue;
        }

        names.push(name.to_string());

        let offscreen = element
            .CachedIsOffscreen()
            .map(|value| value.as_bool())
            .unwrap_or(true);
        let is_button = element
            .CachedControlType()
            .map(|value| value == UIA_ButtonControlTypeId)
            .unwrap_or(false);

        let norm = normalize_ui_label(name);
        if let Some(debug) = debug.as_mut() {
            if is_button && norm.starts_with("usage:") && debug.usage_labels.len() < 3 {
                debug
                    .usage_labels
                    .push(truncate(usage_label_shape(name), 200));
            }
        }
        // The Code tab composer exposes its pickers as "Model: Opus 5.5" and
        // "Effort: Medium" buttons. Those are authoritative; free text (chat
        // messages, the usage popover's "Weekly · Fable" / "Max (5x)") must not
        // outrank them, so long labels are ignored as model sources.
        let explicit_model = is_button && norm.starts_with("model: ");
        if is_button {
            if let Some(effort) = norm.strip_prefix("effort: ").and_then(effort_label) {
                explicit_effort = Some(effort);
            }
        }

        if let Some(candidate) = (explicit_model || name.len() <= 80)
            .then(|| parse_desktop_model_name(name))
            .flatten()
        {
            let score = model_candidate_score(
                explicit_model,
                candidate.effort.is_some(),
                offscreen,
                is_button,
            );
            if let Some(debug) = debug.as_mut() {
                debug.candidates.push(ScanCandidate {
                    score,
                    is_button,
                    offscreen,
                    chars: name.chars().count(),
                    label: candidate_label(name),
                });
            }
            if best_model
                .as_ref()
                .map(|(_, best_score)| score > *best_score)
                .unwrap_or(true)
            {
                best_model = Some((candidate, score));
            }
        }

        match norm.as_str() {
            "adaptive thinking" => {
                adaptive |= is_toggle_on(&automation, &element).unwrap_or(false);
            }
            "extended thinking" => {
                extended |= is_toggle_on(&automation, &element).unwrap_or(false);
            }
            _ => {}
        }
    }

    let mut info = desktop_info_from_ui_names(&names, fallback_mode);
    info.limits = parse_usage_limits(&names);
    if let Some((candidate, score)) = best_model {
        info.model = Some(candidate.model);
        info.model_explicit = score >= 20;
        info.adaptive |= candidate.adaptive;
        info.extended |= candidate.extended;
        if candidate.effort.is_some() {
            info.effort = candidate.effort;
        }
    }
    if explicit_effort.is_some() {
        info.effort = explicit_effort;
    }
    info.adaptive |= adaptive;
    info.extended |= extended;
    if let Some(debug) = debug {
        debug.elements = length;
        debug.scanned = head + (length - tail_start);
        debug.duration_ms = started.map_or(0, |started| started.elapsed().as_millis());
        // Stable: equal scores keep tree order, as the winner selection does.
        debug
            .candidates
            .sort_by_key(|candidate| std::cmp::Reverse(candidate.score));
        debug.candidates.truncate(5);
    }
    Ok(info)
}

#[cfg(windows)]
pub(super) unsafe fn is_toggle_on(
    automation: &IUIAutomation,
    element: &IUIAutomationElement,
) -> Option<bool> {
    if let Some(state) = read_toggle_state(element) {
        return Some(state);
    }

    let walker = automation.RawViewWalker().ok()?;
    if let Ok(parent) = walker.GetParentElement(element) {
        if let Some(state) = read_toggle_state(&parent) {
            return Some(state);
        }
        if let Some(state) = read_child_toggle_state(&walker, &parent, 12) {
            return Some(state);
        }
    }

    read_child_toggle_state(&walker, element, 12)
}

#[cfg(windows)]
pub(super) unsafe fn read_toggle_state(element: &IUIAutomationElement) -> Option<bool> {
    let pattern: IUIAutomationTogglePattern =
        element.GetCurrentPatternAs(UIA_TogglePatternId).ok()?;
    pattern
        .CurrentToggleState()
        .ok()
        .map(|state| state == ToggleState_On)
}

#[cfg(windows)]
pub(super) unsafe fn read_child_toggle_state(
    walker: &IUIAutomationTreeWalker,
    element: &IUIAutomationElement,
    limit: usize,
) -> Option<bool> {
    let mut child = walker.GetFirstChildElement(element).ok();
    let mut count = 0usize;

    while let Some(current) = child {
        if let Some(state) = read_toggle_state(&current) {
            return Some(state);
        }
        count += 1;
        if count >= limit {
            break;
        }
        child = walker.GetNextSiblingElement(&current).ok();
    }

    None
}

pub(super) fn detect_desktop_model(
    info: &DesktopInfo,
    session: Option<&SessionInfo>,
) -> Option<String> {
    // In the Code tab the session log names the model exactly; a scraped UI
    // label only wins over it when it is the composer's "Model: ..." button
    // (the usage popover, for one, lists "Weekly · Fable ... 0%").
    if info.mode.as_deref() == Some("Code") && !info.model_explicit {
        if let Some(model) = detect_code_model(session) {
            return format_desktop_model(&DesktopInfo {
                model: Some(model),
                ..info.clone()
            });
        }
    }
    format_desktop_model(info)
        .or_else(read_settings_model)
        .or_else(|| {
            std::env::var("CLAUDE_MODEL")
                .ok()
                .and_then(|v| format_model_name(&v))
        })
}

#[cfg(any(windows, test))]
pub(super) fn desktop_info_from_ui_names(
    names: &[String],
    fallback_mode: Option<&str>,
) -> DesktopInfo {
    let mut chat_score = 0;
    let mut cowork_score = 0;
    let mut code_score = 0;
    let mut dispatch = false;

    for name in names {
        let norm = normalize_ui_label(name);

        match norm.as_str() {
            "new task" => cowork_score += 5,
            "work in a project" => cowork_score += 3,
            "computer use" => cowork_score += 4,
            "code permissions" => cowork_score += 4,
            "outputs" => cowork_score += 4,
            "keep awake" => cowork_score += 4,
            "allow all browser actions" => cowork_score += 4,
            "sync tasks and refresh memory" => cowork_score += 3,
            "initialize productivity system" => cowork_score += 3,
            "dispatch" => cowork_score += 1,
            "scheduled" => cowork_score += 1,
            "new session" => code_score += 5,
            "routines" => code_score += 4,
            "overview" => code_score += 3,
            "models" => code_score += 3,
            "favorite model" => code_score += 3,
            "current streak" => code_score += 3,
            "longest streak" => code_score += 3,
            "peak hour" => code_score += 3,
            "total tokens" => code_score += 3,
            "active days" => code_score += 3,
            "messages" => code_score += 2,
            "sessions" => code_score += 2,
            "new chat" => chat_score += 5,
            "artifacts" => chat_score += 4,
            "learn" => chat_score += 4,
            "write" => chat_score += 4,
            "from calendar" => chat_score += 4,
            "from gmail" => chat_score += 4,
            _ => {}
        }

        if norm.starts_with("lets knock something off your list")
            || norm.starts_with("let's knock something off your list")
        {
            cowork_score += 6;
        }
        if norm.starts_with("get to work with productivity") {
            cowork_score += 3;
        }
        if norm.starts_with("whats up next") || norm.starts_with("what's up next") {
            code_score += 5;
        }
        // Inside an open Code session the home-screen markers are gone; the
        // sidebar and header still carry these Code-only controls.
        if norm.starts_with("new session in ") {
            code_score += 5;
        }
        if norm == "remote control" || norm == "create pr" {
            code_score += 3;
        }
        if norm.starts_with("back at it") {
            chat_score += 4;
        }
        if norm.starts_with("dispatch background conversation")
            || norm.starts_with("dispatch to claude and check in")
            || norm.starts_with("files claude shares will appear here")
        {
            cowork_score += 1;
            dispatch = true;
        }
    }

    let mut ranked = [
        ("Chat", chat_score),
        ("Cowork", cowork_score),
        ("Code", code_score),
    ];
    ranked.sort_by_key(|(_, score)| std::cmp::Reverse(*score));

    let mode = if ranked[0].1 <= 0 {
        fallback_mode.map(str::to_string)
    } else if ranked[0].1 == ranked[1].1 {
        fallback_mode
            .map(str::to_string)
            .or_else(|| Some(ranked[0].0.to_string()))
    } else {
        Some(ranked[0].0.to_string())
    };

    let submode = if dispatch && mode.as_deref() == Some("Cowork") {
        Some("Dispatch".into())
    } else {
        None
    };

    DesktopInfo {
        mode,
        submode,
        ..DesktopInfo::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_session_model_unless_model_button_seen() {
        let session = SessionInfo {
            file: std::path::PathBuf::from("session.jsonl"),
            started_at_ms: None,
            model: Some("Claude Opus 5.5".into()),
            cwd: None,
            title: None,
            branch: None,
            effort: None,
            recent_cwds: Vec::new(),
        };
        // A loose UI match in the Code tab loses to the session log.
        let loose = DesktopInfo {
            mode: Some("Code".into()),
            model: Some("Claude Fable 5.1".into()),
            effort: Some("High".into()),
            ..DesktopInfo::default()
        };
        assert_eq!(
            detect_desktop_model(&loose, Some(&session)).as_deref(),
            Some("Claude Opus 5.5 | High")
        );
        // The composer's "Model:" button wins.
        let explicit = DesktopInfo {
            model_explicit: true,
            ..loose.clone()
        };
        assert_eq!(
            detect_desktop_model(&explicit, Some(&session)).as_deref(),
            Some("Claude Fable 5.1 | High")
        );
    }

    #[test]
    fn maps_desktop_sidebar_modes() {
        assert_eq!(map_desktop_mode("chat").as_deref(), Some("Chat"));
        assert_eq!(map_desktop_mode("cowork").as_deref(), Some("Cowork"));
        assert_eq!(map_desktop_mode("task").as_deref(), Some("Cowork"));
        assert_eq!(map_desktop_mode("epitaxy").as_deref(), Some("Code"));
    }
    #[test]
    fn scores_desktop_ui_modes() {
        let names = vec![
            "New task".to_string(),
            "Dispatch background conversation".to_string(),
            "Code permissions".to_string(),
        ];
        let info = desktop_info_from_ui_names(&names, Some("Chat"));
        assert_eq!(info.mode.as_deref(), Some("Cowork"));
        assert_eq!(info.submode.as_deref(), Some("Dispatch"));

        // An open Code session has no home-screen markers.
        let names = vec![
            "New session in claude-rpc".to_string(),
            "Remote Control".to_string(),
            "Create PR".to_string(),
        ];
        let info = desktop_info_from_ui_names(&names, Some("Chat"));
        assert_eq!(info.mode.as_deref(), Some("Code"));
    }

    #[test]
    fn scores_model_candidates() {
        // "Model: ..." button, effort named, on screen.
        assert_eq!(
            model_candidate_score(true, true, false, true),
            20 + 4 + 3 + 2
        );
        // Bare offscreen text.
        assert_eq!(model_candidate_score(false, false, true, false), 1);
        assert_eq!(model_candidate_score(false, true, false, false), 4 + 3);
        assert_eq!(model_candidate_score(false, false, false, true), 3 + 2);
    }

    #[test]
    fn keeps_only_usage_button_wording() {
        for label in [
            "Usage: Context 146.8k / 1M (15%), 6% of 5-hour limit, Resets in 4 hr 33 min",
            "Usage: Weekly · all models: 44%, Resets Sun 3:00 AM",
            "Usage: 52% of 5-hour limit, Resets at 10:10 PM",
        ] {
            assert_eq!(usage_label_shape(label), label);
        }
        assert_eq!(
            usage_label_shape("Usage: my password is hunter2, 10% done"),
            "Usage: ? ? ? ? 10% ?"
        );
    }

    #[test]
    fn shows_only_pure_model_labels() {
        for label in [
            "Opus 5.5",
            "Model: Opus 5.5",
            "Claude Sonnet 5 Adaptive",
            "Opus 5 (1M context) (default)",
            "Model: Fable 5.1, Extra high",
        ] {
            assert!(is_pure_model_label(label), "{label}");
            assert_eq!(candidate_label(label), label);
        }
        for label in [
            "Opus vs Sonnet max tokens",
            "claude-rpc",
            "Fix the opus bug",
            "Opus r\u{e9}sum\u{e9}",
        ] {
            assert!(!is_pure_model_label(label), "{label}");
        }
        assert_eq!(candidate_label("Fix the opus bug"), "<text, 16 chars>");
    }
}
