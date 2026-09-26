use super::*;

pub(super) fn build_status(
    result: &DetectionResult,
    discord_user: Option<&str>,
    config: &ClaudeConfig,
) -> Value {
    let claude_line = match result.client {
        ClientType::Desktop => desktop_mode_label(result)
            .map(|mode| format!("Claude: Desktop ({mode})"))
            .unwrap_or_else(|| "Claude: Desktop".into()),
        ClientType::Code => {
            if result.code_instances > 1 {
                format!("Claude: CLI (Code) [{}]", result.code_instances)
            } else {
                "Claude: CLI (Code)".into()
            }
        }
        ClientType::Idle => "Claude: Off".into(),
    };
    let discord_line = match discord_user {
        Some(user) => format!("Discord: Connected ({user})"),
        None => "Discord: RPC disabled".into(),
    };

    // Mirror Discord's per-activity-type card layout:
    // - Playing (type 0): header is just the verb; the `name` ("Clawd")
    //   takes the bold line, then details, then state.
    // - Watching/Listening/Competing: header is "{verb} {name}", then details
    //   (bold), then state, then large_text ("Powered by Anthropic").
    let hidden = hidden_reason(result, config);
    let (preview_header, preview_primary, preview_secondary, preview_tertiary) =
        if result.client == ClientType::Idle || hidden.is_some() {
            (None, None, None, None)
        } else {
            let mode = normalize_mode(&config.rpc_mode);
            let verb = activity_verb(&mode);
            let details = final_details(result, &mode, config);
            let state = final_state(result, config);
            if mode == "playing" {
                (
                    Some("Playing".to_string()),
                    Some("Clawd".to_string()),
                    Some(details),
                    Some(state),
                )
            } else {
                (
                    Some(format!("{verb} Clawd")),
                    Some(details),
                    Some(state),
                    Some("Powered by Anthropic".to_string()),
                )
            }
        };

    json!({
        "claudeLine": claude_line,
        "modelLine": result.model.clone().unwrap_or_else(|| "Auto-detect".into()),
        "limitsLine": result.limits_line.clone(),
        "limits": result
            .limits
            .iter()
            .map(|entry| json!({
                "label": entry.label,
                "usedPercent": entry.used_percent,
                "reset": entry.reset,
            }))
            .collect::<Vec<_>>(),
        "providerLine": match result.plan.as_deref() {
            Some(plan) => format!("Provider: {} ({plan})", result.provider),
            None => format!("Provider: {}", result.provider),
        },
        "discordLine": discord_line,
        "hiddenReason": hidden,
        "sessions": result.code_instances,
        "history5h": result.history_5h,
        "previewHeader": preview_header,
        "previewPrimary": preview_primary,
        "previewSecondary": preview_secondary,
        "previewTertiary": preview_tertiary,
    })
}

pub(super) fn write_status(path: &Path, value: &Value) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("txt.tmp");
    if fs::write(&tmp, value.to_string()).is_ok() {
        let _ = fs::rename(tmp, path);
    }
}

pub(super) fn clear_status(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_status_lines() {
        let config = ClaudeConfig::default();
        let result = DetectionResult {
            client: ClientType::Desktop,
            mode: Some("Code".into()),
            provider: "Subscription".into(),
            plan: Some("Claude Max (5x)".into()),
            ..DetectionResult::default()
        };
        let status = build_status(&result, Some("inerthel"), &config);
        assert_eq!(status["claudeLine"], "Claude: Desktop (Code)");
        assert_eq!(
            status["providerLine"],
            "Provider: Subscription (Claude Max (5x))"
        );
        assert_eq!(status["discordLine"], "Discord: Connected (inerthel)");

        let result = DetectionResult {
            client: ClientType::Code,
            code_instances: 2,
            ..DetectionResult::default()
        };
        let status = build_status(&result, None, &config);
        assert_eq!(status["claudeLine"], "Claude: CLI (Code) [2]");
        assert_eq!(status["discordLine"], "Discord: RPC disabled");
        assert_eq!(status["providerLine"], "Provider: Unknown");
    }
}
