use super::*;

// Sessions written within this window count as running for the private
// projects check, even when another session's log is the latest.
const RECENT_SESSION_MS: u64 = 10 * 60 * 1_000;

// Per session log: (mtime when read, time of its latest prompt typed by the user).
pub(super) type PromptCache = HashMap<PathBuf, (u64, Option<u64>)>;

pub(super) fn read_session_info(prompts: &mut PromptCache) -> Option<SessionInfo> {
    let mut candidates =
        collect_jsonl_candidates(&claude_dir().join("projects"), 24 * 60 * 60 * 1000);
    candidates.sort_by_key(|(_, mtime)| std::cmp::Reverse(*mtime));
    let now = now_ms();
    let mut recent = Vec::new();
    let mut recent_cwds = Vec::new();
    for (path, mtime) in &candidates {
        if now.saturating_sub(*mtime) > RECENT_SESSION_MS {
            break;
        }
        if let Some(cwd) = read_session_cwd(path).filter(|cwd| is_user_project_cwd(cwd)) {
            if !recent_cwds.contains(&cwd) {
                recent_cwds.push(cwd);
            }
            recent.push((path.clone(), *mtime));
        }
    }
    prompts.retain(|path, _| recent.iter().any(|(recent, _)| recent == path));
    // Several sessions write at once (agents working in the background): the
    // one in use is the one the user typed in last, not the last one written.
    let file = match recent
        .iter()
        .max_by_key(|(path, mtime)| (latest_prompt_ms(prompts, path, *mtime), *mtime))
    {
        Some((path, _)) => path.clone(),
        None => candidates
            .into_iter()
            .find(|(path, _)| is_user_session_file(path))
            .map(|(path, _)| path)?,
    };
    let started_at_ms = read_session_start_ms(&file);
    let lines = read_tail_lines(&file, 256 * 1024).unwrap_or_default();
    let model = model_from_lines(&lines);
    let (title, branch) = meta_from_lines(&lines);
    let (effort, effort_command_seen) = session_effort(&lines);
    let cwd = read_session_cwd(&file);
    Some(SessionInfo {
        file,
        started_at_ms,
        model,
        cwd,
        title,
        branch,
        effort,
        effort_command_seen,
        recent_cwds,
    })
}

fn latest_prompt_ms(prompts: &mut PromptCache, path: &Path, mtime: u64) -> Option<u64> {
    let cached = prompts.get(path).copied();
    if let Some((_, prompt)) = cached.filter(|(read_at, _)| *read_at == mtime) {
        return prompt;
    }
    // The prompt can scroll out of the tail while an agent works: keep the
    // last one seen.
    let prompt = read_tail_lines(path, 512 * 1024)
        .and_then(|lines| prompt_from_lines(&lines))
        .or(cached.and_then(|(_, prompt)| prompt));
    prompts.insert(path.to_path_buf(), (mtime, prompt));
    prompt
}

// Time of the latest message the user typed (not a tool result, command
// output or subagent turn).
pub(super) fn prompt_from_lines(lines: &[String]) -> Option<u64> {
    lines.iter().rev().find_map(|line| {
        let entry = serde_json::from_str::<Value>(line).ok()?;
        if entry.get("type").and_then(Value::as_str) != Some("user")
            || is_sidechain_entry(&entry)
            || entry.get("isMeta").and_then(Value::as_bool) == Some(true)
        {
            return None;
        }
        let human = match entry.pointer("/origin/kind").and_then(Value::as_str) {
            Some(kind) => kind == "human",
            // Older Claude Code versions have no origin field.
            None => match entry.pointer("/message/content")? {
                Value::String(text) => !text.trim_start().starts_with('<'),
                Value::Array(blocks) => blocks
                    .iter()
                    .all(|block| block.get("type").and_then(Value::as_str) == Some("text")),
                _ => false,
            },
        };
        if !human {
            return None;
        }
        parse_iso_ms(entry.get("timestamp")?.as_str()?)
    })
}

// Latest session title ("custom-title": set by /rename or by Claude Desktop)
// and git branch of the main thread. Metadata fields only, never chat text.
pub(super) fn meta_from_lines(lines: &[String]) -> (Option<String>, Option<String>) {
    let mut title = None;
    let mut branch = None;
    for line in lines.iter().rev() {
        if title.is_some() && branch.is_some() {
            break;
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if title.is_none() && entry.get("type").and_then(Value::as_str) == Some("custom-title") {
            // Line breaks become spaces instead of gluing words together.
            let raw = entry
                .get("customTitle")
                .and_then(Value::as_str)
                .map(|title| title.replace(|ch: char| ch.is_control(), " "));
            title = sanitize_field(raw.as_deref(), 100);
        }
        if branch.is_none() && !is_sidechain_entry(&entry) {
            branch = entry
                .get("gitBranch")
                .and_then(Value::as_str)
                .filter(|branch| *branch != "HEAD")
                .and_then(|branch| sanitize_field(Some(branch), 60));
        }
    }
    (title, branch)
}

// User session logs sit directly in projects/<project>/. Deeper .jsonl files
// (<session>/subagents/agent-*.jsonl, workflows) are subagent logs, not the
// user's session: they would swap the model and start time.
pub(super) fn collect_jsonl_candidates(root: &Path, max_age_ms: u64) -> Vec<(PathBuf, u64)> {
    let now = now_ms();
    let mut out = Vec::new();
    let Ok(projects) = fs::read_dir(root) else {
        return out;
    };
    for project in projects.flatten() {
        if !project
            .file_type()
            .is_ok_and(|file_type| file_type.is_dir())
        {
            continue;
        }
        let Ok(entries) = fs::read_dir(project.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry.file_type().is_ok_and(|file_type| file_type.is_file())
                || path.extension().and_then(|ext| ext.to_str()) != Some("jsonl")
            {
                continue;
            }
            // The DirEntry's metadata comes with the directory listing on
            // Windows; path.metadata() would open every file.
            let Some(mtime) = entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(system_time_ms)
            else {
                continue;
            };
            if now.saturating_sub(mtime) > max_age_ms {
                continue;
            }
            out.push((path, mtime));
        }
    }
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

// Claude Desktop runs worktree sessions in <project>\.claude\worktrees\<name>:
// they belong to <project>. Other paths are returned unchanged.
pub(super) fn project_root(cwd: &str) -> &str {
    const MARKER: &str = "/.claude/worktrees/";
    // Same byte offsets as `cwd`: '\' and '/' are one byte each and ASCII
    // lowercasing leaves every other byte as is.
    let lower = cwd.replace('\\', "/").to_ascii_lowercase();
    match lower.find(MARKER) {
        Some(index) if index > 0 => &cwd[..index],
        _ => cwd,
    }
}

pub(super) fn is_user_project_cwd(cwd: &str) -> bool {
    // Reject sessions whose cwd lives inside a hidden directory (e.g. C:\Users\x\.claude-mem\...)
    // Background subagents/observers run from these; real Claude Code sessions don't.
    project_root(cwd)
        .split(['/', '\\'])
        .filter(|seg| !seg.is_empty())
        .all(|seg| !seg.starts_with('.') || seg.chars().all(|c| c == '.'))
}

pub(super) fn read_session_start_ms(path: &Path) -> Option<u64> {
    let mut file = File::open(path).ok()?;
    let mut buf = vec![0; 8192];
    let len = file.read(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf[..len]);
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        // A first line longer than the buffer is cut off: skip it, not the file.
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
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

#[cfg(test)]
pub(super) fn read_session_tail(path: &Path) -> Option<String> {
    model_from_lines(&read_tail_lines(path, 256 * 1024)?)
}

pub(super) fn model_from_lines(lines: &[String]) -> Option<String> {
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
    fn reads_session_title_and_branch() {
        let lines = [
            json!({"type": "custom-title", "customTitle": "Old name"}),
            json!({"type": "user", "gitBranch": "feature/x", "message": {"content": "type custom-title"}}),
            json!({"type": "assistant", "gitBranch": "agent-branch", "isSidechain": true}),
            json!({"type": "custom-title", "customTitle": "Fix <login>
bug"}),
        ]
        .map(|entry| entry.to_string());
        let (title, branch) = meta_from_lines(&lines);
        assert_eq!(title.as_deref(), Some("Fix login bug"));
        assert_eq!(branch.as_deref(), Some("feature/x"));
        assert_eq!(meta_from_lines(&[]), (None, None));
    }

    #[test]
    fn worktree_sessions_belong_to_their_project() {
        let worktree = r"D:\Dev\relay-bot\.claude\worktrees\loving-shamir";
        assert_eq!(project_root(worktree), r"D:\Dev\relay-bot");
        assert_eq!(
            project_root("D:/Dev/app/.Claude/Worktrees/x/src"),
            "D:/Dev/app"
        );
        assert_eq!(project_root(r"D:\Dev\app"), r"D:\Dev\app");
        assert!(is_user_project_cwd(worktree));
        assert!(!is_user_project_cwd(r"C:\Users\x\.claude-mem\observer"));
        assert!(!is_user_project_cwd(r"C:\Users\x\.claude\projects"));
    }

    #[test]
    fn finds_the_latest_typed_prompt() {
        let lines = [
            json!({"type": "user", "timestamp": "2026-11-05T10:00:00Z", "origin": {"kind": "human"}, "message": {"content": "hi"}}),
            json!({"type": "user", "timestamp": "2026-11-05T10:01:00Z", "message": {"content": "legacy prompt"}}),
            json!({"type": "user", "timestamp": "2026-11-05T10:02:00Z", "message": {"content": [{"type": "tool_result"}]}}),
            json!({"type": "user", "timestamp": "2026-11-05T10:03:00Z", "message": {"content": "<local-command-stdout>x</local-command-stdout>"}}),
            json!({"type": "user", "timestamp": "2026-11-05T10:04:00Z", "isSidechain": true, "message": {"content": "agent"}}),
            json!({"type": "user", "timestamp": "2026-11-05T10:05:00Z", "origin": {"kind": "task-notification"}, "message": {"content": "done"}}),
        ]
        .map(|entry| entry.to_string());
        assert_eq!(
            prompt_from_lines(&lines),
            parse_iso_ms("2026-11-05T10:01:00Z")
        );
        assert_eq!(prompt_from_lines(&lines[2..]), None);
    }

    #[test]
    fn lists_only_logs_directly_in_project_folders() {
        let root = std::env::temp_dir().join(format!(
            "claude-rpc-session-candidates-{}",
            std::process::id()
        ));
        let project = root.join("project");
        let session = project.join("session");
        fs::create_dir_all(session.join("subagents")).unwrap();
        for path in [
            project.join("a.jsonl"),
            project.join("notes.txt"),
            session.join("b.jsonl"),
            session.join("subagents").join("agent.jsonl"),
        ] {
            fs::write(path, "{}\n").unwrap();
        }
        let found = collect_jsonl_candidates(&root, 60 * 60 * 1000);
        fs::remove_dir_all(&root).unwrap();
        let paths: Vec<_> = found.into_iter().map(|(path, _)| path).collect();
        assert_eq!(paths, vec![project.join("a.jsonl")]);
    }

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
