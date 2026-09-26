// Shared settings model for the tray app (main.rs) and the daemon. Single
// source of truth for ClaudeConfig so the two sides can never drift apart
// (a field missing on one side used to silently wipe saved settings).

use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RpcButton {
    pub(crate) label: String,
    pub(crate) url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClaudeConfig {
    #[serde(default)]
    pub(crate) dnd: bool,
    #[serde(default = "default_show_limits")]
    pub(crate) show_limits: bool,
    #[serde(default = "default_show_limits")]
    pub(crate) show_limit_5h: bool,
    #[serde(default = "default_show_limits")]
    pub(crate) show_limit_all: bool,
    #[serde(default = "default_show_limits")]
    pub(crate) show_limit_fable: bool,
    #[serde(default = "default_show_limits")]
    pub(crate) show_provider: bool,
    #[serde(default = "default_show_limits")]
    pub(crate) show_plan: bool,
    #[serde(default = "default_show_limits")]
    pub(crate) show_effort: bool,
    #[serde(default = "default_rpc_mode")]
    pub(crate) rpc_mode: String,
    #[serde(default = "default_buttons")]
    pub(crate) buttons: Vec<RpcButton>,
    // Temporary pause (tray "Pause activity"): hide the activity until this
    // Unix time in ms. 0 = not paused. `dnd` is the permanent switch.
    #[serde(default)]
    pub(crate) pause_until_ms: u64,
    #[serde(default)]
    pub(crate) show_sessions: bool,
    // Small Discord image shows the model family instead of the terminal icon.
    #[serde(default)]
    pub(crate) model_icon: bool,
    // Custom Discord lines; empty = built-in text. See presence::render_template.
    #[serde(default)]
    pub(crate) details_template: String,
    #[serde(default)]
    pub(crate) state_template: String,
    // Folder names (or path fragments) whose sessions are never published.
    #[serde(default)]
    pub(crate) private_projects: Vec<String>,
    #[serde(default)]
    pub(crate) hide_in_chat: bool,
    // Windows notifications for the 5-hour session limit.
    #[serde(default = "default_show_limits")]
    pub(crate) alert_80: bool,
    #[serde(default = "default_show_limits")]
    pub(crate) alert_95: bool,
    #[serde(default = "default_show_limits")]
    pub(crate) alert_reset: bool,
    // "auto" (follow Windows), "en" or "fr".
    #[serde(default = "default_language")]
    pub(crate) language: String,
}

impl ClaudeConfig {
    // Nothing is published while DND is on or a temporary pause is running.
    pub(crate) fn is_paused(&self, now_ms: u64) -> bool {
        self.dnd || now_ms < self.pause_until_ms
    }
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            dnd: false,
            show_limits: default_show_limits(),
            show_limit_5h: default_show_limits(),
            show_limit_all: default_show_limits(),
            show_limit_fable: default_show_limits(),
            show_provider: default_show_limits(),
            show_plan: default_show_limits(),
            show_effort: default_show_limits(),
            rpc_mode: default_rpc_mode(),
            buttons: default_buttons(),
            pause_until_ms: 0,
            show_sessions: false,
            model_icon: false,
            details_template: String::new(),
            state_template: String::new(),
            private_projects: Vec::new(),
            hide_in_chat: false,
            alert_80: default_show_limits(),
            alert_95: default_show_limits(),
            alert_reset: default_show_limits(),
            language: default_language(),
        }
    }
}

fn default_language() -> String {
    "auto".into()
}

fn default_show_limits() -> bool {
    true
}

fn default_rpc_mode() -> String {
    "playing".into()
}

fn default_buttons() -> Vec<RpcButton> {
    vec![
        RpcButton {
            label: "Claude".into(),
            url: "https://claude.ai".into(),
        },
        RpcButton {
            label: "GitHub Repo".into(),
            url: "https://github.com/inerthel-agi/claude-rpc".into(),
        },
    ]
}

pub(crate) fn read_config(path: &Path) -> ClaudeConfig {
    load_config(path).unwrap_or_default()
}

// Strict load: a missing file is the default config, but an unreadable or
// unparsable one is an error, so a caller can keep its last good config instead
// of silently falling back to defaults (DND off, everything shown).
pub(crate) fn load_config(path: &Path) -> Result<ClaudeConfig, String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ClaudeConfig::default())
        }
        Err(err) => return Err(err.to_string()),
    };
    serde_json::from_str::<ClaudeConfig>(raw.trim_start_matches('\u{feff}'))
        .map(normalize_config)
        .map_err(|err| err.to_string())
}

// Write to a sibling temp file, then rename over the target: the daemon polls
// config.json and must never observe a half-written file.
pub(crate) fn write_config_atomic(path: &Path, config: &ClaudeConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let json = serde_json::to_string_pretty(config).map_err(|err| err.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|err| err.to_string())?;
    fs::rename(&tmp, path).map_err(|err| err.to_string())
}

pub(crate) fn normalize_config(mut config: ClaudeConfig) -> ClaudeConfig {
    config.rpc_mode = normalize_mode(&config.rpc_mode);
    config.buttons = config
        .buttons
        .into_iter()
        .filter_map(|button| {
            let label = clean_label(&button.label)?;
            let url = clean_url(&button.url)?;
            Some(RpcButton { label, url })
        })
        .take(2)
        .collect();
    config.details_template = clean_template(&config.details_template);
    config.state_template = clean_template(&config.state_template);
    config.private_projects = config
        .private_projects
        .iter()
        .map(|entry| entry.trim().to_string())
        .filter(|entry| !entry.is_empty())
        .take(50)
        .collect();
    if !matches!(config.language.as_str(), "auto" | "en" | "fr") {
        config.language = default_language();
    }
    config
}

fn clean_template(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control())
        .take(128)
        .collect::<String>()
        .trim()
        .to_string()
}

pub(crate) fn normalize_mode(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "watching" | "tv" => "watching",
        "listening" | "listen" => "listening",
        "competing" | "compete" => "competing",
        _ => "playing",
    }
    .into()
}

fn clean_label(value: &str) -> Option<String> {
    let cleaned = value
        .chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.chars().take(32).collect())
    }
}

fn clean_url(value: &str) -> Option<String> {
    let value = value.trim();
    if value.starts_with("http://") || value.starts_with("https://") {
        Some(value.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!("claude-rpc-test-{}-{name}", std::process::id()))
            .join("config.json")
    }

    #[test]
    fn normalizes_buttons_and_mode() {
        let config = normalize_config(ClaudeConfig {
            rpc_mode: " TV ".into(),
            buttons: vec![
                RpcButton {
                    label: "  My\u{7}   Site  ".into(),
                    url: "https://example.com".into(),
                },
                RpcButton {
                    label: "Bad".into(),
                    url: "javascript:alert(1)".into(),
                },
                RpcButton {
                    label: "x".repeat(40),
                    url: "http://a.test".into(),
                },
                RpcButton {
                    label: "Third".into(),
                    url: "https://c.test".into(),
                },
            ],
            ..ClaudeConfig::default()
        });
        assert_eq!(config.rpc_mode, "watching");
        assert_eq!(config.buttons.len(), 2);
        assert_eq!(config.buttons[0].label, "My Site");
        assert_eq!(config.buttons[1].label.chars().count(), 32);
    }

    #[test]
    fn loads_and_writes_config_files() {
        let path = temp_config_path("roundtrip");
        let _ = fs::remove_file(&path);
        // Missing file: defaults.
        assert!(load_config(&path).unwrap().show_limits);

        let config = ClaudeConfig {
            dnd: true,
            ..ClaudeConfig::default()
        };
        write_config_atomic(&path, &config).unwrap();
        assert!(load_config(&path).unwrap().dnd);
        assert!(!path.with_extension("json.tmp").exists());

        // A truncated file is an error for load_config (the daemon keeps its
        // last good copy) but still defaults for read_config.
        fs::write(&path, "{\"dnd\": tr").unwrap();
        assert!(load_config(&path).is_err());
        assert!(!read_config(&path).dnd);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
