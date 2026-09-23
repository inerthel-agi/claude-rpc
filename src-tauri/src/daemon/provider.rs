use super::*;

pub(super) fn detect_provider() -> String {
    let settings_env = read_settings_env();
    let lookup = |key: &str| {
        std::env::var(key)
            .ok()
            .or_else(|| settings_env.get(key).cloned())
    };
    let has_value = |key: &str| is_present(lookup(key).as_deref());

    if is_truthy(lookup("CLAUDE_CODE_USE_BEDROCK").as_deref())
        || has_value("ANTHROPIC_BEDROCK_BASE_URL")
        || has_value("AWS_BEARER_TOKEN_BEDROCK")
    {
        "Amazon Bedrock".into()
    } else if is_truthy(lookup("CLAUDE_CODE_USE_VERTEX").as_deref())
        || has_value("ANTHROPIC_VERTEX_PROJECT_ID")
    {
        "Google GCP Vertex".into()
    } else if is_truthy(lookup("CLAUDE_CODE_USE_FOUNDRY").as_deref()) {
        "Microsoft Foundry".into()
    } else if has_value("ANTHROPIC_API_KEY") || has_value("CLAUDE_API_KEY") {
        "Anthropic API".into()
    } else {
        let config_texts = claude_config_texts();
        if config_texts.iter().any(|raw| has_anthropic_api_auth(raw)) {
            "Anthropic API".into()
        } else if config_texts.iter().any(|raw| has_claude_account_auth(raw)) {
            "Claude Account".into()
        } else {
            "Unknown".into()
        }
    }
}

pub(super) fn is_present(value: Option<&str>) -> bool {
    value.map(|value| !value.trim().is_empty()).unwrap_or(false)
}

pub(super) fn read_settings_env() -> HashMap<String, String> {
    let mut result = HashMap::new();
    read_settings_env_file(&claude_dir().join("settings.json"), &mut result);
    read_settings_env_file(&claude_dir().join("settings.local.json"), &mut result);
    result
}

pub(super) fn read_settings_env_file(path: &Path, result: &mut HashMap<String, String>) {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return,
    };
    let value: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    let Some(env) = value.get("env").and_then(Value::as_object) else {
        return;
    };
    result.extend(
        env.iter().filter_map(|(key, value)| {
            value.as_str().map(|value| (key.clone(), value.to_string()))
        }),
    );
}

pub(super) fn claude_config_texts() -> Vec<String> {
    let mut paths = vec![
        claude_dir().join("config.json"),
        claude_dir().join(".credentials.json"),
        claude_dir().join("settings.json"),
        claude_dir().join("settings.local.json"),
    ];
    paths.push(home_dir().join(".claude.json"));

    paths
        .into_iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .collect()
}

pub(super) fn has_anthropic_api_auth(raw: &str) -> bool {
    raw.contains("sk-ant-") || raw.contains("\"apiKeyHelper\"")
}

pub(super) fn has_claude_account_auth(raw: &str) -> bool {
    raw.contains("\"claudeAiOauth\"")
        || raw.contains("\"oauthAccount\"")
        || raw.contains("\"CLAUDE_CODE_OAUTH_TOKEN\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_provider_auth_markers() {
        assert!(has_anthropic_api_auth(
            r#"{"apiKeyHelper":"/usr/local/bin/anthropic-key"}"#
        ));
        assert!(has_anthropic_api_auth(r#"{"key":"sk-ant-redacted"}"#));
        assert!(has_claude_account_auth(
            r#"{"oauthAccount":{"email":"user@example.com"}}"#
        ));
        assert!(has_claude_account_auth(r#"{"claudeAiOauth":{}}"#));
        assert!(!is_present(Some("  ")));
    }
}
