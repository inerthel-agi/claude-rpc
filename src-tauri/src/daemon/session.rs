use super::*;

pub(super) fn read_session_info() -> Option<SessionInfo> {
    let file = find_latest_jsonl_file(&claude_dir().join("projects"), 24 * 60 * 60 * 1000)?;
    let started_at_ms = read_session_start_ms(&file);
    let model = read_session_tail(&file);
    let cwd = read_session_cwd(&file);
    Some(SessionInfo {
        file,
        started_at_ms,
        model,
        cwd,
    })
}

pub(super) fn find_latest_jsonl_file(root: &Path, max_age_ms: u64) -> Option<PathBuf> {
    let mut candidates = collect_jsonl_candidates(root, max_age_ms);
    candidates.sort_by_key(|(_, mtime)| std::cmp::Reverse(*mtime));
    candidates
        .into_iter()
        .find(|(path, _)| is_user_session_file(path))
        .map(|(path, _)| path)
}

pub(super) fn collect_jsonl_candidates(root: &Path, max_age_ms: u64) -> Vec<(PathBuf, u64)> {
    fn walk(dir: &Path, depth: usize, now: u64, max_age_ms: u64, out: &mut Vec<(PathBuf, u64)>) {
        if depth > 3 {
            return;
        }
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                walk(&path, depth + 1, now, max_age_ms, out);
                continue;
            }
            if !file_type.is_file()
                || path.extension().and_then(|ext| ext.to_str()) != Some("jsonl")
            {
                continue;
            }
            let Some(mtime) = modified_ms(&path) else {
                continue;
            };
            if now.saturating_sub(mtime) > max_age_ms {
                continue;
            }
            out.push((path, mtime));
        }
    }

    let mut out = Vec::new();
    walk(root, 0, now_ms(), max_age_ms, &mut out);
    out
}

pub(super) fn is_user_session_file(path: &Path) -> bool {
    match read_session_cwd(path) {
        Some(cwd) => is_user_project_cwd(&cwd),
        // No cwd readable yet (very fresh file) — assume valid; tail-based detection will refine
        None => true,
    }
}

pub(super) fn read_session_cwd(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut buf = vec![0; 32 * 1024];
    let len = file.read(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf[..len]);
    for line in text.lines().take(20) {
        let entry: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if let Some(cwd) = entry.get("cwd").and_then(Value::as_str) {
            return Some(cwd.to_string());
        }
    }
    None
}

pub(super) fn is_user_project_cwd(cwd: &str) -> bool {
    // Reject sessions whose cwd lives inside a hidden directory (e.g. C:\Users\x\.claude-mem\...)
    // Background subagents/observers run from these; real Claude Code sessions don't.
    cwd.split(['/', '\\'])
        .filter(|seg| !seg.is_empty())
        .all(|seg| !seg.starts_with('.') || seg.chars().all(|c| c == '.'))
}

pub(super) fn read_session_start_ms(path: &Path) -> Option<u64> {
    let mut file = File::open(path).ok()?;
    let mut buf = vec![0; 8192];
    let len = file.read(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf[..len]);
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let entry: Value = serde_json::from_str(line).ok()?;
        if let Some(ts) = entry
            .get("timestamp")
            .or_else(|| {
                entry
                    .get("snapshot")
                    .and_then(|snapshot| snapshot.get("timestamp"))
            })
            .and_then(Value::as_str)
        {
            return parse_iso_ms(ts);
        }
    }
    None
}

pub(super) fn read_session_tail(path: &Path) -> Option<String> {
    let lines = read_tail_lines(path, 256 * 1024)?;
    // Pass 1: latest "/model" command anywhere — user-set model wins
    for line in lines.iter().rev() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if is_sidechain_entry(&entry) {
            continue;
        }
        if let Some(model) = read_command_model(&entry) {
            return Some(model);
        }
    }
    // Pass 2: latest assistant message.model from the main thread (skip sidechains)
    for line in lines.iter().rev() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if is_sidechain_entry(&entry) {
            continue;
        }
        if entry.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        if let Some(model) = entry
            .get("message")
            .and_then(|message| message.get("model"))
            .and_then(Value::as_str)
            .and_then(format_model_name)
        {
            return Some(model);
        }
    }
    None
}

pub(super) fn is_sidechain_entry(entry: &Value) -> bool {
    entry
        .get("isSidechain")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(super) fn read_tail_lines(path: &Path, max_bytes: u64) -> Option<Vec<String>> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    let len = size.min(max_bytes);
    let offset = size.saturating_sub(len);
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = vec![0; len as usize];
    file.read_exact(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if offset > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    Some(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_text_never_becomes_the_published_model() {
        let path = std::env::temp_dir().join(format!(
            "claude-rpc-model-privacy-{}.jsonl",
            std::process::id()
        ));
        let metadata = json!({"type": "assistant", "message": {
            "role": "assistant", "model": "claude-sonnet-5", "content": []
        }});
        let wrapped =
            "<local-command-stdout>Set model to CONFIDENTIAL-CANARY</local-command-stdout>";
        for entry in [
            json!({"type": "user", "message": {"role": "user", "content": "Please set model to CONFIDENTIAL-CANARY"}}),
            json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": wrapped}]}}),
            json!({"type": "user", "message": {"role": "user", "content": format!("Quoted example: {wrapped}")}}),
            json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "text": wrapped}]}}),
        ] {
            fs::write(&path, format!("{metadata}\n{entry}\n")).unwrap();
            let result = DetectionResult {
                client: ClientType::Code,
                model: read_session_tail(&path),
                ..DetectionResult::default()
            };
            let activity = build_activity(&result, &ClaudeConfig::default()).unwrap();
            fs::remove_file(&path).unwrap();
            assert_eq!(result.model.as_deref(), Some("Claude Sonnet 5"));
            assert!(!activity.to_string().contains("CONFIDENTIAL-CANARY"));
        }

        // Real command output keeps priority and supports custom model IDs.
        for content in [
            json!("<local-command-stdout>Set model to custom-model-v2</local-command-stdout>"),
            json!([{"type": "text", "text": "<local-command-stdout>Set model to custom-model-v2</local-command-stdout>"}]),
        ] {
            let command = json!({"type": "user", "message": {"role": "user", "content": content}});
            fs::write(&path, format!("{command}\n{metadata}\n")).unwrap();
            let model = read_session_tail(&path);
            fs::remove_file(&path).unwrap();
            assert_eq!(model.as_deref(), Some("custom-model-v2"));
        }
    }
}
