use super::*;

#[cfg(any(windows, test))]
#[derive(Debug, Clone)]
pub(super) struct DesktopModelCandidate {
    pub(super) model: String,
    pub(super) adaptive: bool,
    pub(super) extended: bool,
    pub(super) effort: Option<String>,
}

// Hold the last model detected for a session so the RPC doesn't fall back to
// "Auto-detect" when the assistant entry scrolls past the tail-read window
// (e.g. after large image attachments) or before the first reply lands.
pub(super) fn resolve_code_model(
    machine: &mut StateMachine,
    session: Option<&SessionInfo>,
) -> Option<String> {
    let session_file = session.map(|session| session.file.clone());
    if machine.cached_code_model_session != session_file {
        machine.cached_code_model = None;
        machine.cached_code_model_session = session_file;
    }
    if let Some(model) = detect_code_model(session) {
        machine.cached_code_model = Some(model.clone());
        return Some(model);
    }
    machine.cached_code_model.clone()
}

pub(super) fn detect_code_model(session: Option<&SessionInfo>) -> Option<String> {
    session
        .and_then(|session| session.model.clone())
        .or_else(read_settings_model)
        .or_else(|| {
            std::env::var("CLAUDE_MODEL")
                .ok()
                .and_then(|v| format_model_name(&v))
        })
        .or_else(|| {
            std::env::var("ANTHROPIC_MODEL")
                .ok()
                .and_then(|v| format_model_name(&v))
        })
        .or_else(|| {
            session
                .and_then(|session| session.cwd.as_deref())
                .and_then(read_recent_project_model)
        })
}

// Last-resort guess for a brand-new session with no assistant reply yet:
// the model that did the most work in this project's previous session,
// recorded in ~/.claude.json under projects.<cwd>.lastModelUsage.
pub(super) fn read_recent_project_model(cwd: &str) -> Option<String> {
    let raw = fs::read_to_string(home_dir().join(".claude.json")).ok()?;
    let value: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()?;
    let projects = value.get("projects")?.as_object()?;
    let target = cwd.replace('\\', "/");
    let project = projects
        .iter()
        .find(|(key, _)| key.replace('\\', "/").eq_ignore_ascii_case(&target))
        .map(|(_, value)| value)?;
    let usage = project.get("lastModelUsage")?.as_object()?;
    usage
        .iter()
        .filter(|(id, _)| !id.to_ascii_lowercase().contains("haiku"))
        .max_by_key(|(_, value)| {
            value
                .get("outputTokens")
                .and_then(Value::as_u64)
                .unwrap_or(0)
        })
        .and_then(|(id, _)| format_model_name(id))
}

pub(super) fn read_settings_model() -> Option<String> {
    let raw = fs::read_to_string(claude_dir().join("settings.json")).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    value
        .get("model")
        .and_then(Value::as_str)
        .and_then(format_model_name)
}

pub(super) fn append_code_effort(model: Option<String>, effort: Option<String>) -> Option<String> {
    let model = model?;
    let effort = effort?;
    if model
        .to_ascii_lowercase()
        .contains(&effort.to_ascii_lowercase())
    {
        Some(model)
    } else {
        Some(format!("{model} | {effort}"))
    }
}

// A `/effort` "this session only" override (recorded in the session log) wins over
// the persisted settings.json effortLevel. It is the ONLY signal for the
// "ultracode" tier, which leaves effortLevel at "xhigh" (it is xhigh + workflow
// orchestration) and would otherwise display as "Extra high". Cached per-session
// so it survives the override scrolling past the tail-read window.
pub(super) fn resolve_code_effort(
    machine: &mut StateMachine,
    session: Option<&SessionInfo>,
) -> Option<String> {
    let session_file = session.map(|session| session.file.clone());
    if machine.cached_code_effort_session != session_file {
        machine.cached_code_effort = None;
        machine.cached_code_effort_session = session_file;
    }
    if let Some(effort) = read_session_effort_override(session) {
        machine.cached_code_effort = Some(effort.clone());
        return Some(effort);
    }
    machine
        .cached_code_effort
        .clone()
        .or_else(read_settings_effort)
}

pub(super) fn read_settings_effort() -> Option<String> {
    let raw = fs::read_to_string(claude_dir().join("settings.json")).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    effort_label(value.get("effortLevel").and_then(Value::as_str)?)
}

pub(super) fn read_session_effort_override(session: Option<&SessionInfo>) -> Option<String> {
    let lines = read_tail_lines(&session?.file, 256 * 1024)?;
    for line in lines.iter().rev() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if is_sidechain_entry(&entry) {
            continue;
        }
        if let Some(effort) = read_command_effort(&entry) {
            return Some(effort);
        }
    }
    None
}

pub(super) fn read_command_effort(entry: &Value) -> Option<String> {
    let content = entry.get("message")?.get("content")?;
    if let Some(text) = content.as_str() {
        return parse_command_effort_text(text);
    }
    content.as_array()?.iter().find_map(|item| {
        item.get("text")
            .and_then(Value::as_str)
            .and_then(parse_command_effort_text)
    })
}

pub(super) fn parse_command_effort_text(text: &str) -> Option<String> {
    // Only trust actual /effort command output, not assistant prose that may
    // mention the phrase.
    if !text.contains("command-stdout") {
        return None;
    }
    let cleaned = strip_ansi(text)
        .replace("<local-command-stdout>", " ")
        .replace("</local-command-stdout>", " ");
    let marker = "set effort level to ";
    let lower = cleaned.to_ascii_lowercase();
    let start = lower.find(marker)? + marker.len();
    let rest = cleaned[start..].lines().next().unwrap_or_default();
    let word = rest.split(['(', ':']).next().unwrap_or(rest).trim();
    effort_label(word)
}

pub(super) fn effort_label(raw: &str) -> Option<String> {
    Some(
        match raw.trim().to_ascii_lowercase().as_str() {
            "low" => "Low",
            "medium" => "Medium",
            "high" => "High",
            "xhigh" | "extrahigh" | "extra high" => "Extra high",
            "max" => "Max",
            "ultracode" => "Ultracode",
            _ => return None,
        }
        .into(),
    )
}

#[cfg(any(windows, test))]
pub(super) fn parse_desktop_model_name(raw: &str) -> Option<DesktopModelCandidate> {
    let value = raw.trim();
    // Model labels never carry a percentage; usage rows do ("Weekly · Fable 0%").
    if value.is_empty() || value.contains('%') {
        return None;
    }

    let lower = value.to_ascii_lowercase();
    // A "claude-…" label only counts as a model id when it names a family:
    // the Desktop sidebar lists projects such as "claude-rpc", which would
    // otherwise be displayed verbatim as the model.
    let names_family = ["fable", "opus", "sonnet", "haiku"]
        .iter()
        .any(|family| lower.contains(family));
    let model = if names_family && lower.contains("claude-") {
        format_model_name(value)?
    } else if lower.contains("opus plan") {
        "Opus Plan / Sonnet".into()
    } else {
        let family = if lower.contains("opus") {
            "Opus"
        } else if lower.contains("sonnet") {
            "Sonnet"
        } else if lower.contains("haiku") {
            "Haiku"
        } else if lower.contains("fable") {
            "Fable"
        } else {
            return None;
        };
        let version =
            extract_model_version(value, family).unwrap_or_else(|| default_model_version(family));
        let context = if lower.contains("1m") { " (1M)" } else { "" };
        format!("Claude {family} {version}{context}")
    };

    Some(DesktopModelCandidate {
        model,
        adaptive: lower.contains("adaptive thinking") || lower.contains(" adaptive"),
        extended: lower.contains("extended thinking") || lower.contains(" extended"),
        effort: extract_effort_label(&lower),
    })
}

pub(super) fn format_desktop_model(info: &DesktopInfo) -> Option<String> {
    let mut model = info.model.clone()?;
    append_unique_label(&mut model, info.adaptive, "Adaptive");
    append_unique_label(&mut model, info.extended, "Extended");
    if let Some(effort) = info.effort.as_deref() {
        append_unique_label(&mut model, true, effort);
    }
    Some(truncate(model, 128))
}

pub(super) fn append_unique_label(model: &mut String, enabled: bool, label: &str) {
    if enabled
        && !model
            .to_ascii_lowercase()
            .contains(&label.to_ascii_lowercase())
    {
        model.push_str(" | ");
        model.push_str(label);
    }
}

pub(super) fn normalize_ui_label(value: &str) -> String {
    value
        .replace('\u{2019}', "'")
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn extract_model_version(value: &str, family: &str) -> Option<String> {
    let tokens = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>();
    let parts = tokens.split_whitespace().collect::<Vec<_>>();

    for (index, part) in parts.iter().enumerate() {
        if !part.eq_ignore_ascii_case(family) {
            continue;
        }
        for candidate in parts.iter().skip(index + 1).take(3) {
            // "1m" is the context window, not a version.
            if candidate.eq_ignore_ascii_case("1m") {
                continue;
            }
            let normalized = candidate
                .chars()
                .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
                .collect::<String>();
            if !normalized.chars().any(|ch| ch.is_ascii_digit()) {
                continue;
            }
            // A bare number is a major-only version ("Opus 5"); anything longer
            // than two digits is a snapshot date, not a version.
            if !normalized.contains('.') && normalized.len() > 2 {
                continue;
            }
            // No model family has a major version 0.
            if normalized.starts_with('0') {
                continue;
            }
            return Some(normalized.trim_end_matches('.').to_string());
        }
    }

    None
}

#[cfg(any(windows, test))]
pub(super) fn default_model_version(family: &str) -> String {
    match family {
        "Haiku" => "4.5",
        "Fable" => "5.1",
        "Opus" => "5.5",
        _ => "5",
    }
    .into()
}

pub(super) fn extract_effort_label(lower: &str) -> Option<String> {
    if lower.contains("ultracode") {
        Some("Ultracode".into())
    } else if lower.contains("extra high") || lower.contains("xhigh") {
        Some("Extra high".into())
    } else if lower.contains("medium") {
        Some("Medium".into())
    } else if lower.contains("high") {
        Some("High".into())
    } else if lower.contains("low") {
        Some("Low".into())
    } else if lower.contains("max") {
        Some("Max".into())
    } else {
        None
    }
}

pub(super) fn read_command_model(entry: &Value) -> Option<String> {
    let content = entry.get("message")?.get("content")?;
    if let Some(text) = content.as_str() {
        return parse_command_model_text(text);
    }
    content.as_array()?.iter().find_map(|item| {
        item.get("text")
            .and_then(Value::as_str)
            .and_then(parse_command_model_text)
    })
}

pub(super) fn parse_command_model_text(text: &str) -> Option<String> {
    let cleaned = strip_ansi(text)
        .replace("<local-command-stdout>", " ")
        .replace("</local-command-stdout>", " ")
        .replace("<command-name>", " ")
        .replace("</command-name>", " ")
        .replace("<command-message>", " ")
        .replace("</command-message>", " ")
        .replace("<command-args>", " ")
        .replace("</command-args>", " ");
    let marker = "set model to ";
    let lower = cleaned.to_ascii_lowercase();
    let start = lower.find(marker)? + marker.len();
    let raw = cleaned[start..]
        .lines()
        .next()
        .unwrap_or_default()
        .replace("(default)", "")
        .trim()
        .to_string();
    format_model_name(&raw)
}

pub(super) fn format_model_name(model_id: &str) -> Option<String> {
    let id = model_id.trim().to_ascii_lowercase();
    if id.is_empty() || id == "<synthetic>" || id == "synthetic" {
        return None;
    }
    let context = if id.contains("1m") || id.contains("1m]") || id.contains("1m context") {
        " (1M)"
    } else {
        ""
    };
    let parts = id.split('-').collect::<Vec<_>>();
    if id.contains("opusplan") {
        // The paired Sonnet version follows whatever Claude Code ships, so the
        // label stays version-less instead of drifting.
        return Some("Opus Plan / Sonnet".into());
    }
    // Fable is checked before Opus/Sonnet so a compound id resolves to its own
    // family; the fallback version is the current release of each family.
    for (family, display, fallback) in [
        ("fable", "Fable", "5.1"),
        ("opus", "Opus", "5.5"),
        ("sonnet", "Sonnet", "5"),
        ("haiku", "Haiku", "4.5"),
    ] {
        if !id.contains(family) {
            continue;
        }
        let version = model_version_from_id(&parts, family)
            .or_else(|| extract_model_version(model_id, display))
            .unwrap_or_else(|| fallback.into());
        return Some(format!("Claude {display} {version}{context}"));
    }
    sanitize_field(Some(model_id), 64)
}

// Version read from the dash segments of a lowercased model id:
// "claude-opus-4-8" -> "4.8", "claude-opus-5" -> "5", and the "[1m]" suffix or a
// trailing snapshot date ("claude-opus-5-20260101") are ignored — a version
// segment is at most two digits.
pub(super) fn model_version_from_id(parts: &[&str], family: &str) -> Option<String> {
    let index = parts.iter().position(|part| *part == family)?;
    let major = leading_digits(parts.get(index + 1)?);
    if major.is_empty() || major.len() > 2 {
        return None;
    }
    let minor = parts
        .get(index + 2)
        .map(|part| leading_digits(part))
        .unwrap_or_default();
    if minor.is_empty() || minor.len() > 2 {
        Some(major)
    } else {
        Some(format!("{major}.{minor}"))
    }
}

pub(super) fn leading_digits(value: &str) -> String {
    value.chars().take_while(|ch| ch.is_ascii_digit()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_current_documented_models() {
        for (id, expected) in [
            ("claude-fable-5-1", "Claude Fable 5.1"),
            ("anthropic.claude-opus-5-5", "Claude Opus 5.5"),
            ("claude-sonnet-5", "Claude Sonnet 5"),
            ("claude-haiku-4-5-20251001", "Claude Haiku 4.5"),
            ("Fable", "Claude Fable 5.1"),
            ("Opus", "Claude Opus 5.5"),
        ] {
            assert_eq!(format_model_name(id).as_deref(), Some(expected));
        }
        assert_eq!(
            parse_desktop_model_name("Opus").map(|model| model.model),
            Some("Claude Opus 5.5".into())
        );
        // A project named "claude-…" in the Desktop sidebar is not a model.
        // Usage rows are not models, and version 0 does not exist.
        assert!(parse_desktop_model_name("Weekly - Fable 0%").is_none());
        assert_eq!(
            parse_desktop_model_name("Fable 0").map(|model| model.model),
            Some("Claude Fable 5.1".into())
        );
        assert!(parse_desktop_model_name("claude-rpc").is_none());
        assert_eq!(
            parse_desktop_model_name("claude-opus-5-5").map(|model| model.model),
            Some("Claude Opus 5.5".into())
        );
    }

    #[test]
    fn formats_known_models() {
        assert_eq!(
            format_model_name("claude-opus-4-6").as_deref(),
            Some("Claude Opus 4.6")
        );
        assert_eq!(
            format_model_name("Opus 4.7 (1M context) (default)").as_deref(),
            Some("Claude Opus 4.7 (1M)")
        );
        assert_eq!(
            format_model_name("claude-sonnet-4-6[1m]").as_deref(),
            Some("Claude Sonnet 4.6 (1M)")
        );
    }

    #[test]
    fn formats_generation_five_models() {
        // Major-only ids ("-5") and the 4.8 point release.
        assert_eq!(
            format_model_name("claude-opus-5").as_deref(),
            Some("Claude Opus 5")
        );
        assert_eq!(
            format_model_name("claude-sonnet-5").as_deref(),
            Some("Claude Sonnet 5")
        );
        assert_eq!(
            format_model_name("claude-opus-4-8").as_deref(),
            Some("Claude Opus 4.8")
        );
        assert_eq!(
            format_model_name("claude-opus-4-8-20260210").as_deref(),
            Some("Claude Opus 4.8")
        );
        // A snapshot date must not be read as the minor version.
        assert_eq!(
            format_model_name("claude-opus-5-20260601").as_deref(),
            Some("Claude Opus 5")
        );
        assert_eq!(
            format_model_name("anthropic.claude-sonnet-5[1m]").as_deref(),
            Some("Claude Sonnet 5 (1M)")
        );
        // Desktop UI labels (no dashes) resolve through the text scrape.
        assert_eq!(
            format_model_name("Opus 5 (1M context) (default)").as_deref(),
            Some("Claude Opus 5 (1M)")
        );
        assert_eq!(
            parse_desktop_model_name("Claude Sonnet 5 Adaptive")
                .map(|candidate| candidate.model)
                .as_deref(),
            Some("Claude Sonnet 5")
        );
        // Family without a version falls back to the current release.
        assert_eq!(
            parse_desktop_model_name("Opus")
                .map(|candidate| candidate.model)
                .as_deref(),
            Some("Claude Opus 5.5")
        );
        assert_eq!(
            format_model_name("claude-opusplan").as_deref(),
            Some("Opus Plan / Sonnet")
        );
    }

    #[test]
    fn parses_model_command_output() {
        assert_eq!(
            parse_command_model_text(
                "<local-command-stdout>Set model to \u{1b}[1mOpus 4.7 (1M context) (default)\u{1b}[22m</local-command-stdout>"
            )
            .as_deref(),
            Some("Claude Opus 4.7 (1M)")
        );
        assert_eq!(
            parse_command_model_text(
                "<local-command-stdout>Set model to \u{1b}[1mSonnet 4.6\u{1b}[22m</local-command-stdout>"
            )
            .as_deref(),
            Some("Claude Sonnet 4.6")
        );
        assert_eq!(format_model_name("<synthetic>"), None);
    }
    #[test]
    fn parses_desktop_model_labels() {
        let candidate = parse_desktop_model_name("Claude Opus 4.7 1M Adaptive Extended Extra high")
            .expect("desktop model");
        assert_eq!(candidate.model, "Claude Opus 4.7 (1M)");
        assert!(candidate.adaptive);
        assert!(candidate.extended);
        assert_eq!(candidate.effort.as_deref(), Some("Extra high"));

        let label = format_desktop_model(&DesktopInfo {
            model: Some(candidate.model),
            adaptive: candidate.adaptive,
            extended: candidate.extended,
            effort: candidate.effort,
            ..DesktopInfo::default()
        });
        assert_eq!(
            label.as_deref(),
            Some("Claude Opus 4.7 (1M) | Adaptive | Extended | Extra high")
        );
    }
    #[test]
    fn formats_fable_model() {
        assert_eq!(
            format_model_name("claude-fable-5").as_deref(),
            Some("Claude Fable 5")
        );
        assert_eq!(
            format_model_name("anthropic.claude-fable-5").as_deref(),
            Some("Claude Fable 5")
        );
        assert_eq!(
            format_model_name("claude-fable-5[1m]").as_deref(),
            Some("Claude Fable 5 (1M)")
        );
        // Mythos is intentionally left unformatted (enterprise / invite-only).
        assert_eq!(
            format_model_name("claude-mythos-5").as_deref(),
            Some("claude-mythos-5")
        );
    }

    #[test]
    fn parses_effort_command_output() {
        assert_eq!(
            parse_command_effort_text(
                "<local-command-stdout>Set effort level to ultracode (this session only): xhigh + dynamic workflow orchestration</local-command-stdout>"
            )
            .as_deref(),
            Some("Ultracode")
        );
        assert_eq!(
            parse_command_effort_text(
                "<local-command-stdout>Set effort level to high</local-command-stdout>"
            )
            .as_deref(),
            Some("High")
        );
        // Prose without the command-output tag must not match (e.g. chat text).
        assert_eq!(parse_command_effort_text("set effort level to max"), None);
        assert_eq!(effort_label("xhigh").as_deref(), Some("Extra high"));
        assert_eq!(effort_label("nonsense"), None);
    }
}
