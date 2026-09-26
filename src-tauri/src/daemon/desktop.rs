use super::*;

pub(super) fn read_desktop_info(process_ids: &[u32]) -> DesktopInfo {
    let mut info = read_desktop_config_info();
    if let Some(ui_info) = read_desktop_ui_info(process_ids, info.mode.as_deref()) {
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
) -> Option<DesktopInfo> {
    let hwnd = find_desktop_window(process_ids)?;
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if hr.is_err() {
        return None;
    }
    let info = unsafe { read_desktop_ui_info_from_window(hwnd, fallback_mode) }.ok();
    unsafe { CoUninitialize() };
    info
}

#[cfg(not(windows))]
pub(super) fn read_desktop_ui_info(
    _process_ids: &[u32],
    _fallback_mode: Option<&str>,
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
) -> windows::core::Result<DesktopInfo> {
    let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
    let root = automation.ElementFromHandle(hwnd)?;
    let condition = automation.CreateTrueCondition()?;
    let elements = root.FindAll(TreeScope_Descendants, &condition)?;
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
        let Ok(name_bstr) = element.CurrentName() else {
            continue;
        };
        let name = name_bstr.to_string();
        let name = name.trim();
        if name.is_empty() {
            continue;
        }

        names.push(name.to_string());

        let offscreen = element
            .CurrentIsOffscreen()
            .map(|value| value.as_bool())
            .unwrap_or(true);
        let is_button = element
            .CurrentControlType()
            .map(|value| value == UIA_ButtonControlTypeId)
            .unwrap_or(false);

        let norm = normalize_ui_label(name);
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
            let score = if explicit_model { 20 } else { 0 }
                + if candidate.effort.is_some() { 4 } else { 0 }
                + if !offscreen { 3 } else { 1 }
                + if is_button { 2 } else { 0 };
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
}
