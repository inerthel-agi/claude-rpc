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
        "AWS Bedrock".into()
    } else if is_truthy(lookup("CLAUDE_CODE_USE_VERTEX").as_deref())
        || has_value("ANTHROPIC_VERTEX_PROJECT_ID")
    {
        "Google Vertex".into()
    } else if is_truthy(lookup("CLAUDE_CODE_USE_FOUNDRY").as_deref()) {
        "Microsoft Azure".into()
    } else if has_value("ANTHROPIC_API_KEY") || has_value("CLAUDE_API_KEY") {
        "Anthropic API".into()
    } else if has_value("CLAUDE_CODE_OAUTH_TOKEN") {
        "Subscription".into()
    } else {
        let config_texts = claude_config_texts();
        let any = |check: fn(&str) -> bool| config_texts.iter().any(|raw| check(raw));
        // Precedence mirrors Claude Code: an apiKeyHelper beats the login, and a
        // live subscription OAuth login beats an API key left over from an
        // earlier Console login (both can sit on disk at the same time).
        if any(has_api_key_helper) {
            "Anthropic API".into()
        } else if any(has_subscription_credentials) {
            "Subscription".into()
        } else if any(has_anthropic_api_auth) {
            "Anthropic API".into()
        } else if any(has_claude_account_auth) {
            "Subscription".into()
        } else {
            "Unknown".into()
        }
    }
}

// Subscription plan of the signed-in Claude account, from the OAuth
// credentials Claude Code stores ("subscriptionType": "max",
// "rateLimitTier": "default_claude_max_5x"), falling back to the account
// profile cached in ~/.claude.json ("organizationType": "claude_max").
pub(super) fn detect_plan() -> Option<String> {
    let read = |path: PathBuf| -> Option<Value> {
        let raw = fs::read_to_string(path).ok()?;
        serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()
    };
    let from_credentials = read(claude_dir().join(".credentials.json")).and_then(|value| {
        let oauth = value.get("claudeAiOauth")?;
        plan_label(
            oauth.get("subscriptionType")?.as_str()?,
            oauth.get("rateLimitTier").and_then(Value::as_str),
        )
    });
    from_credentials.or_else(|| {
        let value = read(home_dir().join(".claude.json"))?;
        let account = value.get("oauthAccount")?;
        plan_label(
            account.get("organizationType")?.as_str()?,
            account
                .get("organizationRateLimitTier")
                .and_then(Value::as_str),
        )
    })
}

pub(super) fn plan_label(kind: &str, tier: Option<&str>) -> Option<String> {
    let kind = kind.trim().to_ascii_lowercase();
    let tier = tier.unwrap_or_default().to_ascii_lowercase();
    let label = match kind.trim_start_matches("claude_") {
        "free" => "Claude Free",
        "pro" => "Claude Pro",
        "max" if tier.contains("20x") => "Claude Max (20x)",
        "max" if tier.contains("5x") => "Claude Max (5x)",
        "max" => "Claude Max",
        "team" => "Claude Team",
        "enterprise" => "Claude Enterprise",
        _ => return None,
    };
    Some(label.into())
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

pub(super) fn has_api_key_helper(raw: &str) -> bool {
    raw.contains("\"apiKeyHelper\"")
}

// OAuth tokens also start with "sk-ant-" ("sk-ant-oat01-…"), so only the
// API-key prefix counts as API auth.
pub(super) fn has_anthropic_api_auth(raw: &str) -> bool {
    raw.contains("sk-ant-api") || has_api_key_helper(raw)
}

pub(super) fn has_subscription_credentials(raw: &str) -> bool {
    raw.contains("\"claudeAiOauth\"")
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
        assert!(has_anthropic_api_auth(r#"{"key":"sk-ant-api03-redacted"}"#));
        // A subscription OAuth token is not an API key.
        assert!(!has_anthropic_api_auth(
            r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-redacted"}}"#
        ));
        assert!(has_subscription_credentials(
            r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-redacted"}}"#
        ));
        assert!(has_claude_account_auth(
            r#"{"oauthAccount":{"email":"user@example.com"}}"#
        ));
        assert!(has_claude_account_auth(r#"{"claudeAiOauth":{}}"#));
        assert!(!is_present(Some("  ")));
    }

    #[test]
    fn labels_subscription_plans() {
        for (kind, tier, expected) in [
            (
                "max",
                Some("default_claude_max_5x"),
                Some("Claude Max (5x)"),
            ),
            (
                "max",
                Some("default_claude_max_20x"),
                Some("Claude Max (20x)"),
            ),
            ("claude_max", None, Some("Claude Max")),
            ("pro", None, Some("Claude Pro")),
            ("free", None, Some("Claude Free")),
            ("team", Some("default_claude_team"), Some("Claude Team")),
            ("claude_enterprise", None, Some("Claude Enterprise")),
            ("unknown", None, None),
        ] {
            assert_eq!(plan_label(kind, tier).as_deref(), expected);
        }
    }
}
