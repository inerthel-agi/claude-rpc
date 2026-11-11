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
    resolve_session_effort(machine, session).or_else(read_settings_effort)
}

// The effort of the session in use (see `effort_from_lines`), cached per session.
// Desktop Code sessions use it alone: the settings.json default does not apply to them.
pub(super) fn resolve_session_effort(
    machine: &mut StateMachine,
    session: Option<&SessionInfo>,
) -> Option<String> {
    let session_file = session.map(|session| session.file.clone());
    if machine.cached_code_effort_session != session_file {
        machine.cached_code_effort = None;
        machine.cached_code_effort_session = session_file;
    }
    if let Some(effort) = read_session_effort_override(session) {
        // Ultracode runs at xhigh: once its `/effort` line leaves the read tail,
        // the turns alone say Extra high, which is not a change.
        let ultracode_continues = machine.cached_code_effort.as_deref() == Some("Ultracode")
            && effort == "Extra high"
            && !session.is_some_and(|session| session.effort_command_seen);
        if !ultracode_continues {
            machine.cached_code_effort = Some(effort);
        }
    }
    machine.cached_code_effort.clone()
}

// Desktop Code tab whose window shows no effort: the session effort is the only
// source left. Chat and Cowork tabs never take it (their chat is not the log).
// With a terminal CLI running, the latest session may be the CLI's, not the Desktop
// tab's, so the session effort is not shown.
pub(super) fn desktop_model_with_session_effort(
    model: Option<String>,
    window_effort: Option<&str>,
    mode: Option<&str>,
    session_effort: Option<String>,
    cli_running: bool,
) -> Option<String> {
    let label_has_effort = model
        .as_deref()
        .and_then(|model| extract_effort_label(&model.to_ascii_lowercase()))
        .is_some();
    if mode != Some("Code")
        || window_effort.is_some()
        || label_has_effort
        || cli_running
        || session_effort.is_none()
    {
        return model;
    }
    append_code_effort(model, session_effort)
}

pub(super) fn read_settings_effort() -> Option<String> {
    let raw = fs::read_to_string(claude_dir().join("settings.json")).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    effort_label(value.get("effortLevel").and_then(Value::as_str)?)
}

pub(super) fn read_session_effort_override(session: Option<&SessionInfo>) -> Option<String> {
    session?.effort.clone()
}

// Effort of the session in use, from the main-thread log tail: the latest `/effort`
// output (O) against the latest turn's effort (P, Claude Code's "perTurnEffort").
// The newer one wins. "Ultracode" runs at xhigh, so an Ultracode override followed
// by an Extra high turn stays Ultracode. A later turn at another level ends it.
#[cfg(test)]
pub(super) fn effort_from_lines(lines: &[String]) -> Option<String> {
    session_effort(lines).0
}

// `effort_from_lines`, plus whether the tail holds an `/effort` command.
pub(super) fn session_effort(lines: &[String]) -> (Option<String>, bool) {
    let mut command: Option<(usize, String)> = None;
    let mut turn: Option<(usize, String)> = None;
    // A turn after the command at a level other than xhigh ends an Ultracode override.
    let mut other_turn_after_command = false;
    for (index, line) in lines.iter().enumerate().rev() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if is_sidechain_entry(&entry) {
            continue;
        }
        if command.is_none() {
            command = read_command_effort(&entry).map(|effort| (index, effort));
        }
        if let Some(effort) = read_turn_effort(&entry) {
            other_turn_after_command |= command.is_none() && effort != "Extra high";
            turn.get_or_insert((index, effort));
        }
        if command.is_some() && turn.is_some() {
            break;
        }
    }
    let command_seen = command.is_some();
    let Some((turn_index, turn_effort)) = turn else {
        return (command.map(|(_, effort)| effort), command_seen);
    };
    let effort = match command {
        Some((command_index, effort)) if command_index > turn_index => effort,
        // shortcut: a level picked in the Desktop UI is not logged, so Ultracode stays
        // until a turn at another level is logged, upgrade if the UI writes a log line.
        Some((_, effort))
            if effort == "Ultracode"
                && turn_effort == "Extra high"
                && !other_turn_after_command =>
        {
            effort
        }
        _ => turn_effort,
    };
    (Some(effort), command_seen)
}

// Effort of a main-thread assistant turn: top-level "perTurnEffort", else the
// top-level "effort" of the same entry.
pub(super) fn read_turn_effort(entry: &Value) -> Option<String> {
    if entry.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    entry
        .get("perTurnEffort")
        .and_then(Value::as_str)
        .and_then(effort_label)
        .or_else(|| {
            entry
                .get("effort")
                .and_then(Value::as_str)
                .and_then(effort_label)
        })
}

pub(super) fn read_command_effort(entry: &Value) -> Option<String> {
    read_local_command(entry, parse_command_effort_text)
}

fn read_local_command(entry: &Value, parse: fn(&str) -> Option<String>) -> Option<String> {
    if entry.get("type").and_then(Value::as_str) != Some("user")
        || entry.pointer("/message/role").and_then(Value::as_str) != Some("user")
    {
        return None;
    }
    let content = entry.get("message")?.get("content")?;
    if let Some(text) = content.as_str() {
        return parse(text);
    }
    content.as_array()?.iter().find_map(|item| {
        if item.get("type").and_then(Value::as_str) != Some("text") {
            return None;
        }
        item.get("text").and_then(Value::as_str).and_then(parse)
    })
}

fn local_command_stdout(text: &str) -> Option<&str> {
    text.trim()
        .strip_prefix("<local-command-stdout>")?
        .strip_suffix("</local-command-stdout>")
        .map(str::trim)
}

pub(super) fn parse_command_effort_text(text: &str) -> Option<String> {
    let cleaned = strip_ansi(local_command_stdout(text)?);
    let marker = "set effort level to ";
    let lower = cleaned.to_ascii_lowercase();
    lower.strip_prefix(marker)?;
    let rest = cleaned[marker.len()..].lines().next().unwrap_or_default();
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
        let version = extract_model_version(value, family)
            .map(|version| format!(" {version}"))
            .unwrap_or_default();
        let context = if lower.contains("1m") { " (1M)" } else { "" };
        format!("Claude {family}{version}{context}")
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
    read_local_command(entry, parse_command_model_text)
}

pub(super) fn parse_command_model_text(text: &str) -> Option<String> {
    // Conversation text is private, even when it quotes a /model command.
    let cleaned = strip_ansi(local_command_stdout(text)?);
    let marker = "set model to ";
    let lower = cleaned.to_ascii_lowercase();
    lower.strip_prefix(marker)?;
    let raw = cleaned[marker.len()..]
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
    // family. Without a version the family is shown alone: a guessed one goes stale.
    for (family, display) in [
        ("fable", "Fable"),
        ("opus", "Opus"),
        ("sonnet", "Sonnet"),
        ("haiku", "Haiku"),
    ] {
        if !id.contains(family) {
            continue;
        }
        let version = model_version_from_id(&parts, family)
            .or_else(|| extract_model_version(model_id, display))
            .map(|version| format!(" {version}"))
            .unwrap_or_default();
        return Some(format!("Claude {display}{version}{context}"));
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
            ("Fable", "Claude Fable"),
            ("Opus", "Claude Opus"),
        ] {
            assert_eq!(format_model_name(id).as_deref(), Some(expected));
        }
        assert_eq!(
            parse_desktop_model_name("Opus").map(|model| model.model),
            Some("Claude Opus".into())
        );
        // A project named "claude-…" in the Desktop sidebar is not a model.
        // Usage rows are not models, and version 0 does not exist.
        assert!(parse_desktop_model_name("Weekly - Fable 0%").is_none());
        assert_eq!(
            parse_desktop_model_name("Fable 0").map(|model| model.model),
            Some("Claude Fable".into())
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
        // A family without a version is shown alone, never with a guessed one.
        assert_eq!(
            parse_desktop_model_name("Opus")
                .map(|candidate| candidate.model)
                .as_deref(),
            Some("Claude Opus")
        );
        assert_eq!(
            format_model_name("claude-opusplan").as_deref(),
            Some("Opus Plan / Sonnet")
        );
    }

    #[test]
    fn keeps_the_context_window_without_a_version() {
        assert_eq!(
            format_model_name("opus[1m]").as_deref(),
            Some("Claude Opus (1M)")
        );
        assert_eq!(
            parse_desktop_model_name("Opus 1M")
                .map(|candidate| candidate.model)
                .as_deref(),
            Some("Claude Opus (1M)")
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

    #[test]
    fn reads_latest_main_thread_effort_from_lines() {
        let command = |level: &str, sidechain: bool| {
            json!({"type": "user", "isSidechain": sidechain, "message": {"role": "user",
                "content": format!("<local-command-stdout>Set effort level to {level}</local-command-stdout>")}})
            .to_string()
        };
        let lines = vec![
            command("low", false),
            command("high", false),
            json!({"type": "assistant", "message": {"role": "assistant", "content": []}})
                .to_string(),
            "not json".to_string(),
            command("max", true),
        ];
        assert_eq!(effort_from_lines(&lines).as_deref(), Some("High"));
        assert_eq!(effort_from_lines(&lines[2..]), None);
        assert_eq!(effort_from_lines(&[]), None);
    }

    fn assistant_turn(extra: Value) -> String {
        let mut entry =
            json!({"type": "assistant", "message": {"role": "assistant", "content": []}});
        for (key, value) in extra.as_object().expect("object") {
            entry[key] = value.clone();
        }
        entry.to_string()
    }

    fn effort_command(level: &str) -> String {
        json!({"type": "user", "message": {"role": "user",
            "content": format!("<local-command-stdout>Set effort level to {level}</local-command-stdout>")}})
        .to_string()
    }

    #[test]
    fn reads_the_effort_of_each_turn() {
        let turn = |effort: &str| assistant_turn(json!({"perTurnEffort": effort}));
        assert_eq!(effort_from_lines(&[turn("high")]).as_deref(), Some("High"));
        assert_eq!(
            effort_from_lines(&[assistant_turn(json!({"effort": "xhigh"}))]).as_deref(),
            Some("Extra high")
        );
        assert_eq!(effort_from_lines(&[turn("max")]).as_deref(), Some("Max"));
        assert_eq!(effort_from_lines(&[turn("low")]).as_deref(), Some("Low"));
        // The latest turn wins.
        assert_eq!(
            effort_from_lines(&[turn("high"), turn("xhigh")]).as_deref(),
            Some("Extra high")
        );
        // A turn without the field does not hide an earlier one.
        assert_eq!(
            effort_from_lines(&[turn("high"), assistant_turn(json!({}))]).as_deref(),
            Some("High")
        );
        // An unknown perTurnEffort falls back to the top-level effort.
        assert_eq!(
            effort_from_lines(&[assistant_turn(
                json!({"perTurnEffort": "auto", "effort": "medium"})
            )])
            .as_deref(),
            Some("Medium")
        );
        // Sidechain turns and other "effort" keys are not the turn effort.
        let sidechain = json!({"type": "assistant", "isSidechain": true, "perTurnEffort": "max"});
        assert_eq!(
            effort_from_lines(&[turn("low"), sidechain.to_string()]).as_deref(),
            Some("Low")
        );
        let other_keys = [
            json!({"type": "assistant", "wireToolInputs": {"toolu_1": {"effort": "max"}}}),
            json!({"type": "api-request", "params": {"output_config": {"effort": "max"}}}),
            json!({"type": "user", "effort": "max"}),
        ];
        for entry in other_keys {
            assert_eq!(effort_from_lines(&[entry.to_string()]), None);
        }
    }

    #[test]
    fn desktop_code_tab_takes_the_session_effort_only_without_a_window_effort() {
        let opus = || Some("Claude Opus 5.5".to_string());
        let session = || Some("Extra high".to_string());
        // Code tab, no effort on screen: the session effort is shown.
        assert_eq!(
            desktop_model_with_session_effort(opus(), None, Some("Code"), session(), false)
                .as_deref(),
            Some("Claude Opus 5.5 | Extra high")
        );
        // The window's own effort is kept.
        assert_eq!(
            desktop_model_with_session_effort(
                Some("Claude Opus 5.5 | High".into()),
                Some("High"),
                Some("Code"),
                session(),
                false
            )
            .as_deref(),
            Some("Claude Opus 5.5 | High")
        );
        // A model label that already names an effort is kept.
        assert_eq!(
            desktop_model_with_session_effort(
                Some("Claude Opus 5.5 | Low".into()),
                None,
                Some("Code"),
                session(),
                false
            )
            .as_deref(),
            Some("Claude Opus 5.5 | Low")
        );
        // Chat and Cowork tabs never take the session effort.
        for mode in [Some("Chat"), Some("Cowork"), None] {
            assert_eq!(
                desktop_model_with_session_effort(opus(), None, mode, session(), false).as_deref(),
                Some("Claude Opus 5.5")
            );
        }
        // A terminal CLI is running: the session may be the CLI's, so no session effort.
        assert_eq!(
            desktop_model_with_session_effort(opus(), None, Some("Code"), session(), true)
                .as_deref(),
            Some("Claude Opus 5.5")
        );
        // No session effort: unchanged.
        assert_eq!(
            desktop_model_with_session_effort(opus(), None, Some("Code"), None, false).as_deref(),
            Some("Claude Opus 5.5")
        );
        assert_eq!(
            desktop_model_with_session_effort(None, None, Some("Code"), session(), false),
            None
        );
    }

    #[test]
    fn newer_effort_command_or_turn_wins() {
        let turn = |effort: &str| assistant_turn(json!({"perTurnEffort": effort}));
        assert_eq!(
            effort_from_lines(&[turn("high"), effort_command("low")]).as_deref(),
            Some("Low")
        );
        assert_eq!(
            effort_from_lines(&[effort_command("low"), turn("high")]).as_deref(),
            Some("High")
        );
    }

    #[test]
    fn ultracode_runs_at_extra_high_in_the_turns() {
        let turn = |effort: &str| assistant_turn(json!({"perTurnEffort": effort}));
        assert_eq!(
            effort_from_lines(&[effort_command("ultracode"), turn("xhigh"), turn("xhigh")])
                .as_deref(),
            Some("Ultracode")
        );
        // A later turn at another level means the override is gone.
        assert_eq!(
            effort_from_lines(&[effort_command("ultracode"), turn("xhigh"), turn("high")])
                .as_deref(),
            Some("High")
        );
        // Even when xhigh turns follow it: the High turn already ended Ultracode.
        assert_eq!(
            effort_from_lines(&[effort_command("ultracode"), turn("high"), turn("xhigh")])
                .as_deref(),
            Some("Extra high")
        );
    }

    #[test]
    fn ultracode_survives_its_command_leaving_the_tail() {
        let session = |effort: &str, effort_command_seen| SessionInfo {
            file: std::path::PathBuf::from("session.jsonl"),
            started_at_ms: None,
            model: None,
            cwd: None,
            title: None,
            branch: None,
            effort: Some(effort.into()),
            effort_command_seen,
            recent_cwds: Vec::new(),
        };
        let mut machine = StateMachine::default();
        let mut resolve =
            |effort, seen| resolve_session_effort(&mut machine, Some(&session(effort, seen)));
        assert_eq!(resolve("Ultracode", true).as_deref(), Some("Ultracode"));
        // `/effort ultracode` scrolled out: only the xhigh turns are left.
        assert_eq!(resolve("Extra high", false).as_deref(), Some("Ultracode"));
        // An explicit `/effort xhigh` in the tail is a real change.
        assert_eq!(resolve("Extra high", true).as_deref(), Some("Extra high"));
        resolve("Ultracode", true);
        // A turn at another level ends it.
        assert_eq!(resolve("High", false).as_deref(), Some("High"));
    }
}
