use super::*;

pub(super) fn strip_ansi(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

pub(super) fn map_desktop_mode(value: &str) -> Option<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "cowork" | "task" => Some("Cowork".into()),
        "epitaxy" => Some("Code".into()),
        "chat" => Some("Chat".into()),
        "code" => Some("Code".into()),
        _ => None,
    }
}

pub(super) fn sanitize_field(value: Option<&str>, max_len: usize) -> Option<String> {
    let cleaned = value?
        .chars()
        .filter(|ch| !ch.is_control() && !matches!(ch, '<' | '>' | '"' | '\'' | '`'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.chars().take(max_len).collect())
    }
}

pub(super) fn sanitize_discord_user(value: &str) -> Option<String> {
    sanitize_field(Some(value), 32)
}

pub(super) fn is_truthy(value: Option<&str>) -> bool {
    matches!(
        value.map(|value| value.trim().to_ascii_lowercase()),
        Some(value) if value == "1" || value == "true"
    )
}

pub(super) fn parse_iso_ms(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.len() < 19 {
        return None;
    }
    let year = value.get(0..4)?.parse::<i32>().ok()?;
    let month = value.get(5..7)?.parse::<u32>().ok()?;
    let day = value.get(8..10)?.parse::<u32>().ok()?;
    let hour = value.get(11..13)?.parse::<u32>().ok()?;
    let minute = value.get(14..16)?.parse::<u32>().ok()?;
    let second = value.get(17..19)?.parse::<u32>().ok()?;
    Some(datetime_to_unix_ms(year, month, day, hour, minute, second))
}

pub(super) fn datetime_to_unix_ms(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> u64 {
    let days = days_from_civil(year, month as i32, day as i32);
    ((days * 86_400 + (hour as i64) * 3_600 + (minute as i64) * 60 + second as i64) * 1000) as u64
}

pub(super) fn days_from_civil(year: i32, month: i32, day: i32) -> i64 {
    let year = year - if month <= 2 { 1 } else { 0 };
    let era = (if year >= 0 { year } else { year - 399 }) / 400;
    let yoe = year - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146097 + doe - 719468) as i64
}

pub(super) fn truncate(value: String, max_len: usize) -> String {
    if value.chars().count() <= max_len {
        value
    } else {
        value
            .chars()
            .take(max_len.saturating_sub(3))
            .collect::<String>()
            + "..."
    }
}

pub(super) fn min_option(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

pub(super) fn modified_ms(path: &Path) -> Option<u64> {
    let modified = path.metadata().ok()?.modified().ok()?;
    system_time_ms(modified)
}

pub(super) fn now_ms() -> u64 {
    system_time_ms(SystemTime::now()).unwrap_or(0)
}

pub(super) fn system_time_ms(value: SystemTime) -> Option<u64> {
    Some(value.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64)
}

pub(super) fn sleep_polling(stop: &Arc<AtomicBool>, interval_ms: u64) {
    let mut slept = 0;
    while slept < interval_ms && !stop.load(Ordering::SeqCst) {
        let step = (interval_ms - slept).min(100);
        thread::sleep(Duration::from_millis(step));
        slept += step;
    }
}

pub(super) fn parse_env_u64(key: &str, default: u64, min: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value >= min)
        .unwrap_or(default)
}

pub(super) fn claude_dir() -> PathBuf {
    std::env::var("CLAUDE_DIR_PATH")
        .ok()
        .map(|value| expand_home(&value))
        .unwrap_or_else(|| home_dir().join(".claude"))
}

pub(super) fn app_dir() -> PathBuf {
    std::env::var("CLAUDE_RPC_DIR")
        .ok()
        .map(|value| expand_home(&value))
        .unwrap_or_else(|| home_dir().join(".claude-rpc"))
}

pub(super) fn roaming_app_data() -> PathBuf {
    if let Some(app_data) = std::env::var_os("APPDATA") {
        return PathBuf::from(app_data);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join("Library")
            .join("Application Support");
    }
    home_dir()
}

pub(super) fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

pub(super) fn expand_home(value: &str) -> PathBuf {
    if value == "~" {
        return home_dir();
    }
    if let Some(rest) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        return home_dir().join(rest);
    }
    PathBuf::from(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_iso_timestamps_as_utc() {
        assert_eq!(parse_iso_ms("1970-01-01T00:00:01Z"), Some(1000));
    }
}
