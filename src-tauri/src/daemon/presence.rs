use super::*;

pub(super) fn build_activity(result: &DetectionResult, config: &ClaudeConfig) -> Option<Value> {
    if result.client == ClientType::Idle || hidden_reason(result, config).is_some() {
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
        "details": final_details(result, &mode, config),
        "state": final_state(result, config),
        "assets": {
            "large_image": logo_image(),
            "large_text": "Powered by Anthropic",
            "small_image": "terminal_icon",
            "small_text": small_image_text(result),
        },
    });
    if config.model_icon {
        if let Some(family) = model_family(result.model.as_deref()) {
            activity["assets"]["small_image"] = json!(model_icon_image(family));
            activity["assets"]["small_text"] = json!(result
                .model
                .as_deref()
                .and_then(|model| model.split(" | ").next())
                .unwrap_or("Claude"));
        }
    }

    if let Some(started_at_ms) = result.started_at_ms {
        activity["timestamps"] = json!({ "start": started_at_ms / 1000 });
    }
    if mode == "watching" && !config.buttons.is_empty() {
        activity["buttons"] =
            serde_json::to_value(config.buttons.iter().take(2).collect::<Vec<_>>()).ok()?;
    }

    Some(activity)
}

pub(super) fn build_details(result: &DetectionResult, mode: &str) -> String {
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

    base.to_string()
}

pub(super) fn build_state(result: &DetectionResult, config: &ClaudeConfig) -> String {
    let model = format_rpc_model(
        result.model.as_deref().unwrap_or("Claude"),
        config.show_effort,
    );
    let mut parts = vec![model];
    if config.show_provider {
        // The plan ("Claude Max (5x)") replaces the generic "Subscription".
        match result.plan.as_ref().filter(|_| config.show_plan) {
            Some(plan) => parts.push(plan.clone()),
            None => parts.push(result.provider.clone()),
        }
    }
    if config.show_sessions && result.code_instances > 1 {
        parts.push(format!("{} sessions", result.code_instances));
    }
    if let Some(limits) = result.limits_line.as_deref() {
        parts.push(limits.to_string());
    }
    truncate(parts.join(" | "), 128)
}

// Discord lines as published: the user's template when set, else built-in.
pub(super) fn final_details(result: &DetectionResult, mode: &str, config: &ClaudeConfig) -> String {
    render_template(&config.details_template, result).unwrap_or_else(|| build_details(result, mode))
}

pub(super) fn final_state(result: &DetectionResult, config: &ClaudeConfig) -> String {
    render_template(&config.state_template, result).unwrap_or_else(|| build_state(result, config))
}

// Replaces {model} {effort} {plan} {provider} {limits} {limit5h} {limitWeekly}
// {limitFable} {sessions} {project} {client} {mode}. None when the template is
// empty or renders to under 2 characters (Discord's minimum).
pub(super) fn render_template(template: &str, result: &DetectionResult) -> Option<String> {
    let template = template.trim();
    if template.is_empty() {
        return None;
    }
    let model_line = result.model.as_deref().unwrap_or("Claude");
    let model = model_line.split(" | ").next().unwrap_or("Claude");
    let effort = model_line
        .split(" | ")
        .find(|part| is_effort_label(part))
        .unwrap_or("");
    let limit = |label: &str| {
        result
            .limits
            .iter()
            .find(|entry| entry.label == label)
            .map(|entry| format!("{}%", entry.used_percent))
            .unwrap_or_default()
    };
    let client = match result.client {
        ClientType::Desktop => "Claude Desktop",
        ClientType::Code => "Claude Code",
        ClientType::Idle => "Claude",
    };
    let vars = [
        ("{model}", model.to_string()),
        ("{effort}", effort.to_string()),
        (
            "{plan}",
            result
                .plan
                .clone()
                .unwrap_or_else(|| result.provider.clone()),
        ),
        ("{provider}", result.provider.clone()),
        (
            "{limits}",
            result
                .limits_line
                .as_deref()
                .map(|line| line.trim_start_matches("Limits: ").to_string())
                .unwrap_or_default(),
        ),
        ("{limit5h}", limit("5h")),
        ("{limitWeekly}", limit("All")),
        ("{limitFable}", limit("Fable")),
        ("{sessions}", result.code_instances.max(1).to_string()),
        (
            "{project}",
            project_name(result.project_dir.as_deref()).unwrap_or_default(),
        ),
        ("{client}", client.to_string()),
        ("{mode}", result.mode.clone().unwrap_or_default()),
    ];
    let mut rendered = template.to_string();
    for (key, value) in vars {
        rendered = rendered.replace(key, &value);
    }
    let rendered = sanitize_field(Some(&rendered), 128)?;
    (rendered.chars().count() >= 2).then_some(rendered)
}

pub(super) fn project_name(dir: Option<&str>) -> Option<String> {
    dir?.replace('\\', "/")
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

// Why nothing is published for this result, if anything hides it.
pub(super) fn hidden_reason(
    result: &DetectionResult,
    config: &ClaudeConfig,
) -> Option<&'static str> {
    if config.hide_in_chat
        && result.client == ClientType::Desktop
        && result.mode.as_deref() == Some("Chat")
    {
        return Some("chat");
    }
    let coding = result.client == ClientType::Code || result.mode.as_deref() == Some("Code");
    if coding
        && result
            .project_dir
            .as_deref()
            .is_some_and(|dir| is_private_project(dir, &config.private_projects))
    {
        return Some("private");
    }
    None
}

// An entry without a slash matches the project folder name; one with a slash
// matches that path and everything below it. Case-insensitive.
pub(super) fn is_private_project(dir: &str, entries: &[String]) -> bool {
    let dir = dir
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    let name = dir.rsplit('/').next().unwrap_or("");
    entries.iter().any(|entry| {
        let entry = entry
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_ascii_lowercase();
        if entry.contains('/') {
            dir == entry || dir.starts_with(&format!("{entry}/"))
        } else {
            !entry.is_empty() && name == entry
        }
    })
}

pub(super) fn model_family(model: Option<&str>) -> Option<&'static str> {
    let model = model?.to_ascii_lowercase();
    ["fable", "opus", "sonnet", "haiku"]
        .into_iter()
        .find(|family| model.contains(family))
}

pub(super) fn model_icon_image(family: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/inerthel-agi/claude-rpc/main/logo/model-{family}.png"
    )
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
        "plan": result.plan,
        "rpcMode": config.rpc_mode,
        "dnd": config.dnd,
        "showLimits": config.show_limits,
        "showLimit5h": config.show_limit_5h,
        "showLimitAll": config.show_limit_all,
        "showLimitFable": config.show_limit_fable,
        "showProvider": config.show_provider,
        "showPlan": config.show_plan,
        "showEffort": config.show_effort,
        "buttons": config.buttons,
        "showSessions": config.show_sessions,
        "sessions": result.code_instances,
        "modelIcon": config.model_icon,
        "detailsTemplate": config.details_template,
        "stateTemplate": config.state_template,
        "hidden": hidden_reason(result, config),
        "project": result.project_dir,
        "limitBuckets": result
            .limits
            .iter()
            .map(|entry| (entry.label.clone(), entry.used_percent))
            .collect::<Vec<_>>(),
    }))
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_custom_templates() {
        let result = DetectionResult {
            client: ClientType::Desktop,
            mode: Some("Code".into()),
            model: Some("Claude Opus 5.5 | High".into()),
            provider: "Subscription".into(),
            plan: Some("Claude Max (5x)".into()),
            project_dir: Some(r"D:\work\claude-rpc".into()),
            limits: vec![UsageLimitEntry {
                label: "5h".into(),
                used_percent: 66,
                reset: None,
                updated_at_ms: 0,
            }],
            ..DetectionResult::default()
        };
        assert_eq!(
            render_template("{model} · {effort} | {plan} | 5h {limit5h}", &result).as_deref(),
            Some("Claude Opus 5.5 · High | Claude Max (5x) | 5h 66%")
        );
        assert_eq!(
            render_template("In {project} ({client}, {mode})", &result).as_deref(),
            Some("In claude-rpc (Claude Desktop, Code)")
        );
        // Empty or too-short templates fall back to the built-in text.
        assert_eq!(render_template("  ", &result), None);
        assert_eq!(render_template("{limitWeekly}", &result), None);
        let config = ClaudeConfig {
            state_template: "{model}".into(),
            ..ClaudeConfig::default()
        };
        assert_eq!(final_state(&result, &config), "Claude Opus 5.5");
    }

    #[test]
    fn hides_private_projects_and_chat() {
        let entries = vec!["secret-app".to_string(), r"D:\clients".to_string()];
        assert!(is_private_project(r"D:\work\Secret-App", &entries));
        assert!(is_private_project("D:/clients/acme/api", &entries));
        assert!(!is_private_project(r"D:\clientsx\acme", &entries));
        assert!(!is_private_project(r"D:\work\claude-rpc", &entries));

        let config = ClaudeConfig {
            private_projects: entries,
            hide_in_chat: true,
            ..ClaudeConfig::default()
        };
        let coding = DetectionResult {
            client: ClientType::Code,
            project_dir: Some(r"D:\work\secret-app".into()),
            ..DetectionResult::default()
        };
        assert_eq!(hidden_reason(&coding, &config), Some("private"));
        assert!(build_activity(&coding, &config).is_none());
        let chat = DetectionResult {
            client: ClientType::Desktop,
            mode: Some("Chat".into()),
            project_dir: Some(r"D:\work\secret-app".into()),
            ..DetectionResult::default()
        };
        assert_eq!(hidden_reason(&chat, &config), Some("chat"));
        assert_eq!(model_family(Some("Claude Fable 5.1 | High")), Some("fable"));
        assert_eq!(model_family(None), None);
    }

    #[test]
    fn displays_client_labels_without_a_project() {
        let mut config = ClaudeConfig::default();
        for mode in ["playing", "watching", "listening", "competing"] {
            config.rpc_mode = mode.into();
            for (client, expected) in [
                (
                    ClientType::Code,
                    if mode == "watching" {
                        "Watching Claude Code"
                    } else {
                        "Claude Code"
                    },
                ),
                (
                    ClientType::Desktop,
                    if mode == "watching" {
                        "Watching Claude (Code)"
                    } else {
                        "Claude Desktop (Code)"
                    },
                ),
            ] {
                let result = DetectionResult {
                    client,
                    mode: Some("Code".into()),
                    ..DetectionResult::default()
                };
                assert_eq!(
                    build_activity(&result, &config).unwrap()["details"],
                    expected
                );
                let preview_field = if mode == "playing" {
                    "previewSecondary"
                } else {
                    "previewPrimary"
                };
                assert_eq!(
                    build_status(&result, None, &config)[preview_field],
                    expected
                );
            }
        }
    }

    #[test]
    fn hides_presence_without_a_client() {
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
            limits_line: Some("Limits: 5h 3%".into()),
            ..DetectionResult::default()
        };
        let mut config = ClaudeConfig::default();

        assert_eq!(
            build_state(&result, &config),
            "Claude Opus 4.7 (1M) | Extra high | Anthropic API | Limits: 5h 3%"
        );

        config.show_provider = false;
        assert_eq!(
            build_state(&result, &config),
            "Claude Opus 4.7 (1M) | Extra high | Limits: 5h 3%"
        );

        config.show_effort = false;
        assert_eq!(
            build_state(&result, &config),
            "Claude Opus 4.7 (1M) | Limits: 5h 3%"
        );
    }
}
