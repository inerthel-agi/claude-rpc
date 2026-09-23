use super::*;

pub(super) fn build_activity(result: &DetectionResult, config: &ClaudeConfig) -> Option<Value> {
    if result.client == ClientType::Idle && !config.show_idle {
        return None;
    }

    let mode = normalize_mode(&config.rpc_mode);
    let activity_type = match mode.as_str() {
        "watching" => ACTIVITY_WATCHING,
        "listening" => ACTIVITY_LISTENING,
        "competing" => ACTIVITY_COMPETING,
        _ => ACTIVITY_PLAYING,
    };

    let mut activity = json!({
        "name": "Clawd",
        "type": activity_type,
        "created_at": now_ms(),
        "instance": false,
        "details": build_details(result, &mode, config),
        "state": build_state(result, config),
        "assets": {
            "large_image": logo_image(),
            "large_text": "Powered by Anthropic",
            "small_image": "terminal_icon",
            "small_text": small_image_text(result),
        },
    });

    if let Some(started_at_ms) = result.started_at_ms {
        activity["timestamps"] = json!({ "start": started_at_ms / 1000 });
    }
    if mode == "watching" && !config.buttons.is_empty() {
        activity["buttons"] =
            serde_json::to_value(config.buttons.iter().take(2).collect::<Vec<_>>()).ok()?;
    }

    Some(activity)
}

pub(super) fn build_details(result: &DetectionResult, mode: &str, config: &ClaudeConfig) -> String {
    let base = match (result.client, mode) {
        (ClientType::Desktop, "watching") => "Watching Claude",
        (ClientType::Code, "watching") => "Watching Claude Code",
        (ClientType::Desktop, _) => "Claude Desktop",
        (ClientType::Code, _) => "Claude Code",
        (ClientType::Idle, _) => "Claude",
    };

    if result.client == ClientType::Desktop {
        if let Some(mode_label) = desktop_mode_label(result) {
            return format!("{base} ({mode_label})");
        }
    }

    if result.client == ClientType::Code && config.show_session_title {
        if let Some(title) = sanitize_field(result.session_title.as_deref(), 64) {
            let candidate = format!("{base} - {title}");
            if candidate.len() <= 96 {
                return candidate;
            }
            if base.len() + 5 < 96 {
                let budget = 96 - base.len() - 5;
                let trimmed: String = title.chars().take(budget).collect();
                return format!("{base} - {trimmed}…");
            }
        }
        if let Some(repo) = sanitize_field(result.project_name.as_deref(), 32) {
            let candidate = format!("{base} - {repo}");
            if candidate.len() <= 96 {
                return candidate;
            }
        }
    }

    base.to_string()
}

pub(super) fn build_state(result: &DetectionResult, config: &ClaudeConfig) -> String {
    let model = format_rpc_model(
        result.model.as_deref().unwrap_or("Claude"),
        config.show_effort,
    );
    let mut parts = vec![model];
    if config.show_provider {
        parts.push(result.provider.clone());
    }
    if let Some(limits) = result.limits_line.as_deref() {
        parts.push(limits.to_string());
    }
    truncate(parts.join(" | "), 128)
}

pub(super) fn format_rpc_model(model: &str, show_effort: bool) -> String {
    if show_effort {
        return model.to_string();
    }

    let parts = model
        .split(" | ")
        .filter(|part| !is_effort_label(part))
        .collect::<Vec<_>>();
    if parts.is_empty() {
        model.to_string()
    } else {
        parts.join(" | ")
    }
}

pub(super) fn is_effort_label(value: &str) -> bool {
    matches!(
        normalize_ui_label(value).as_str(),
        "low" | "medium" | "high" | "extra high" | "xhigh" | "max" | "ultracode"
    )
}

pub(super) fn small_image_text(result: &DetectionResult) -> String {
    match result.client {
        ClientType::Desktop => "Claude Desktop".into(),
        ClientType::Code => "Claude Code CLI".into(),
        ClientType::Idle => "Claude".into(),
    }
}

pub(super) fn logo_image() -> String {
    "https://raw.githubusercontent.com/inerthel-agi/claude-rpc/main/logo/clawd.png".into()
}

pub(super) fn activity_verb(mode: &str) -> &'static str {
    match mode {
        "watching" => "Watching",
        "listening" => "Listening to",
        "competing" => "Competing in",
        _ => "Playing",
    }
}

pub(super) fn desktop_mode_label(result: &DetectionResult) -> Option<String> {
    let mode = result.mode.as_deref()?;
    if mode.is_empty() {
        return None;
    }
    if let Some(submode) = result.submode.as_deref().filter(|value| !value.is_empty()) {
        Some(format!("{mode} - {submode}"))
    } else {
        Some(mode.to_string())
    }
}

pub(super) fn presence_key(result: &DetectionResult, config: &ClaudeConfig) -> String {
    serde_json::to_string(&json!({
        "client": format!("{:?}", result.client),
        "mode": result.mode,
        "submode": result.submode,
        "model": result.model,
        "limits": result.limits_line,
        "provider": result.provider,
        "project": result.project_name,
        "rpcMode": config.rpc_mode,
        "dnd": config.dnd,
        "showLimits": config.show_limits,
        "showLimit5h": config.show_limit_5h,
        "showLimitAll": config.show_limit_all,
        "showProvider": config.show_provider,
        "showEffort": config.show_effort,
        "showSessionTitle": config.show_session_title,
        "showIdle": config.show_idle,
        "buttons": config.buttons,
    }))
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_presence_without_a_client_when_idle_is_disabled() {
        let mut config = ClaudeConfig::default();
        let mut result = DetectionResult::default();
        for mode in ["playing", "watching", "listening", "competing"] {
            config.rpc_mode = mode.into();
            result.client = ClientType::Idle;
            assert!(build_activity(&result, &config).is_none());
            assert!(build_status(&result, None, &config)["previewHeader"].is_null());
            for client in [ClientType::Code, ClientType::Desktop] {
                result.client = client;
                assert!(build_activity(&result, &config).is_some());
            }
        }
    }

    #[test]
    fn normalizes_rpc_modes() {
        assert_eq!(normalize_mode("watching"), "watching");
        assert_eq!(normalize_mode("tv"), "watching");
        assert_eq!(normalize_mode("unknown"), "playing");
    }
    #[test]
    fn formats_rpc_state_visibility() {
        let result = DetectionResult {
            client: ClientType::Desktop,
            model: Some("Claude Opus 4.7 (1M) | Extra high".into()),
            provider: "Anthropic API".into(),
            limits_line: Some("Limits (1): 5h 3%".into()),
            ..DetectionResult::default()
        };
        let mut config = ClaudeConfig::default();

        assert_eq!(
            build_state(&result, &config),
            "Claude Opus 4.7 (1M) | Extra high | Anthropic API | Limits (1): 5h 3%"
        );

        config.show_provider = false;
        assert_eq!(
            build_state(&result, &config),
            "Claude Opus 4.7 (1M) | Extra high | Limits (1): 5h 3%"
        );

        config.show_effort = false;
        assert_eq!(
            build_state(&result, &config),
            "Claude Opus 4.7 (1M) | Limits (1): 5h 3%"
        );
    }
}
