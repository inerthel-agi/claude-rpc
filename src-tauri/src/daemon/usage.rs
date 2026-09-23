// Usage-limit buckets from Desktop and the OAuth API, plus their cache.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;

use super::{
    app_dir, claude_dir, normalize_ui_label, now_ms, truncate, write_status, StateMachine,
};
use crate::config::ClaudeConfig;

const LIMITS_CACHE_MS: u64 = 6 * 60 * 60 * 1_000;
// A per-bucket percentage is only shown as current if it was refreshed within
// this window. Kept below the shortest usage window (5h) so a stale value can
// never outlive its own reset; OAuth re-polls well within it (<=10 min idle).
const LIMITS_DISPLAY_TTL_MS: u64 = 60 * 60 * 1_000;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct UsageLimitEntry {
    pub(crate) label: String,
    pub(crate) used_percent: u8,
    pub(crate) reset: Option<String>,
    // When this bucket's percentage was last observed. Stamped per-entry in
    // merge_limit_entries so a bucket absent from a later (partial) detection is
    // not re-marked fresh, and can be expired individually past its window.
    #[serde(default)]
    pub(crate) updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LimitsCache {
    updated_at: u64,
    limits: Vec<UsageLimitEntry>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LimitVisibility {
    pub(crate) enabled: bool,
    pub(crate) show_5h: bool,
    pub(crate) show_all: bool,
}

pub(crate) fn limit_visibility(config: &ClaudeConfig) -> LimitVisibility {
    LimitVisibility {
        enabled: config.show_limits,
        show_5h: config.show_limit_5h,
        show_all: config.show_limit_all,
    }
}

#[cfg(any(windows, test))]
pub(crate) fn parse_usage_limits(names: &[String]) -> Vec<UsageLimitEntry> {
    let mut entries = Vec::new();

    for (index, name) in names.iter().enumerate() {
        let Some(used_percent) = parse_used_percent(name) else {
            continue;
        };
        let Some((label, label_index)) = find_limit_label(names, index) else {
            continue;
        };
        if entries
            .iter()
            .any(|entry: &UsageLimitEntry| entry.label == label)
        {
            continue;
        }
        entries.push(UsageLimitEntry {
            label,
            used_percent,
            reset: find_limit_reset(names, label_index, index),
            updated_at_ms: 0,
        });
    }

    sort_limit_entries(&mut entries);
    entries
}

#[cfg(any(windows, test))]
pub(crate) fn parse_used_percent(value: &str) -> Option<u8> {
    let lower = value.to_ascii_lowercase();
    if !lower.contains("used") || !lower.contains('%') {
        return None;
    }
    let before_percent = lower.split('%').next()?;
    let digits = before_percent
        .chars()
        .rev()
        .skip_while(|ch| ch.is_whitespace())
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    // Parse wide then clamp to 100: an over-limit/glitched scrape (>100, or >255
    // which would overflow u8 -> None) must not silently drop the row and skew the
    // "Limits (N)" count — mirror the OAuth path's clamp(0,100).
    digits.parse::<u32>().ok().map(|value| value.min(100) as u8)
}

#[cfg(any(windows, test))]
pub(crate) fn find_limit_label(names: &[String], usage_index: usize) -> Option<(String, usize)> {
    let start = usage_index.saturating_sub(12);
    for index in (start..usage_index).rev() {
        let label = match normalize_ui_label(&names[index]).as_str() {
            "current session" => "5h",
            "all models" => "All",
            _ => continue,
        };
        return Some((label.into(), index));
    }
    None
}

#[cfg(any(windows, test))]
pub(crate) fn find_limit_reset(
    names: &[String],
    label_index: usize,
    usage_index: usize,
) -> Option<String> {
    names
        .iter()
        .take(usage_index)
        .skip(label_index + 1)
        .find_map(|name| {
            let normalized = normalize_ui_label(name);
            normalized
                .strip_prefix("resets ")
                .map(|reset| reset.trim().to_string())
        })
}

pub(crate) fn limits_line(
    entries: &[UsageLimitEntry],
    visibility: LimitVisibility,
) -> Option<String> {
    if !visibility.enabled {
        return None;
    }

    let parts = visible_limit_labels(visibility)
        .into_iter()
        .filter_map(|label| {
            entries
                .iter()
                .find(|entry| entry.label == label)
                .map(|entry| format!("{} {}%", entry.label, entry.used_percent))
        })
        .collect::<Vec<_>>();

    if parts.is_empty() {
        return None;
    }
    let count = parts.len();
    let parts = parts.join(" | ");
    Some(truncate(format!("Limits ({count}): {parts}"), 128))
}

pub(crate) fn visible_limit_labels(visibility: LimitVisibility) -> Vec<&'static str> {
    let mut labels = Vec::new();
    if visibility.show_5h {
        labels.push("5h");
    }
    if visibility.show_all {
        labels.push("All");
    }
    labels
}

pub(crate) fn current_limits(
    machine: &mut StateMachine,
    detected_limits: &[UsageLimitEntry],
) -> Vec<UsageLimitEntry> {
    let now = now_ms();
    if machine.cached_limits.is_empty() {
        if let Some(cache) = read_limits_cache(now) {
            machine.cached_limits = cache.limits;
        }
    }

    if !detected_limits.is_empty() {
        machine.cached_limits = merge_limit_entries(&machine.cached_limits, detected_limits, now);
        write_limits_cache(now, &machine.cached_limits);
        return fresh_limit_entries(&machine.cached_limits, now);
    }

    if let Some(oauth_limits) = maybe_fetch_oauth_limits(machine, now) {
        if !oauth_limits.is_empty() {
            machine.cached_limits = merge_limit_entries(&machine.cached_limits, &oauth_limits, now);
            write_limits_cache(now, &machine.cached_limits);
            return fresh_limit_entries(&machine.cached_limits, now);
        }
    }

    fresh_limit_entries(&machine.cached_limits, now)
}

// Drop buckets not refreshed within LIMITS_DISPLAY_TTL_MS so an individual stale
// percentage (e.g. one OAuth omitted, or all sources down) is never shown as
// current, while still-fresh siblings keep displaying.
pub(crate) fn fresh_limit_entries(entries: &[UsageLimitEntry], now: u64) -> Vec<UsageLimitEntry> {
    entries
        .iter()
        .filter(|entry| now.saturating_sub(entry.updated_at_ms) <= LIMITS_DISPLAY_TTL_MS)
        .cloned()
        .collect()
}

const OAUTH_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_USAGE_BETA: &str = "oauth-2025-04-20";
const OAUTH_USAGE_IDLE_POLL_MS: u64 = 10 * 60 * 1000;
const OAUTH_USAGE_ACTIVITY_POLL_MS: u64 = 60 * 1000;
const OAUTH_USAGE_BACKOFF_MS: u64 = 5 * 60 * 1000;

pub(crate) fn maybe_fetch_oauth_limits(
    machine: &mut StateMachine,
    now: u64,
) -> Option<Vec<UsageLimitEntry>> {
    if now < machine.oauth_backoff_until_ms {
        machine.pending_activity_refresh = false;
        return None;
    }
    let min_interval = if machine.pending_activity_refresh {
        OAUTH_USAGE_ACTIVITY_POLL_MS
    } else {
        OAUTH_USAGE_IDLE_POLL_MS
    };
    if machine.oauth_last_attempt_ms != 0
        && now.saturating_sub(machine.oauth_last_attempt_ms) < min_interval
    {
        return None;
    }
    machine.oauth_last_attempt_ms = now;
    machine.pending_activity_refresh = false;
    match fetch_oauth_usage() {
        Ok(entries) => Some(entries),
        Err(OAuthFetchError::RateLimited) => {
            machine.oauth_backoff_until_ms = now + OAUTH_USAGE_BACKOFF_MS;
            None
        }
        Err(_) => None,
    }
}

pub(crate) enum OAuthFetchError {
    NoToken,
    Network,
    RateLimited,
    Parse,
}

pub(crate) fn fetch_oauth_usage() -> Result<Vec<UsageLimitEntry>, OAuthFetchError> {
    let token = read_oauth_access_token().ok_or(OAuthFetchError::NoToken)?;
    let response = ureq::get(OAUTH_USAGE_URL)
        .timeout(std::time::Duration::from_secs(8))
        .set("Authorization", &format!("Bearer {token}"))
        .set("anthropic-beta", OAUTH_USAGE_BETA)
        .set("User-Agent", "claude-rpc")
        .call();
    let body = match response {
        Ok(resp) => resp.into_string().map_err(|_| OAuthFetchError::Parse)?,
        Err(ureq::Error::Status(429, _)) => return Err(OAuthFetchError::RateLimited),
        Err(_) => return Err(OAuthFetchError::Network),
    };
    let value: Value = serde_json::from_str(&body).map_err(|_| OAuthFetchError::Parse)?;
    Ok(parse_oauth_usage_response(&value))
}

pub(crate) fn read_oauth_access_token() -> Option<String> {
    let path = claude_dir().join(".credentials.json");
    let raw = fs::read_to_string(&path).ok()?;
    let value: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()?;
    let oauth = value.get("claudeAiOauth")?;
    let token = oauth.get("accessToken").and_then(Value::as_str)?;
    if let Some(expires_at) = oauth.get("expiresAt").and_then(Value::as_u64) {
        if expires_at < now_ms() {
            return None;
        }
    }
    Some(token.to_string())
}

pub(crate) fn parse_oauth_usage_response(body: &Value) -> Vec<UsageLimitEntry> {
    let mut entries = Vec::new();
    let buckets = [("five_hour", "5h"), ("seven_day", "All")];
    for (key, label) in buckets {
        let Some(bucket) = body.get(key) else {
            continue;
        };
        let Some(percent) = extract_oauth_usage_percent(bucket) else {
            continue;
        };
        let reset = bucket
            .get("resets_at")
            .and_then(Value::as_str)
            .or_else(|| bucket.get("reset_at").and_then(Value::as_str))
            .map(String::from);
        entries.push(UsageLimitEntry {
            label: label.into(),
            used_percent: percent,
            reset,
            updated_at_ms: 0,
        });
    }
    entries
}

pub(crate) fn extract_oauth_usage_percent(bucket: &Value) -> Option<u8> {
    // `utilization` is always a 0..100 percentage in the OAuth usage API.
    if let Some(raw) = bucket.get("utilization").and_then(Value::as_f64) {
        return Some(raw.round().clamp(0.0, 100.0) as u8);
    }
    for key in ["percent_used", "used_percent", "usage", "value"] {
        let Some(raw) = bucket.get(key).and_then(Value::as_f64) else {
            continue;
        };
        // Auto-detect: ratio (0..1) gets multiplied; percentage (>1.5) used directly
        let pct = if raw <= 1.5 { raw * 100.0 } else { raw };
        return Some(pct.round().clamp(0.0, 100.0) as u8);
    }
    None
}

pub(crate) fn write_limits_cache(updated_at: u64, limits: &[UsageLimitEntry]) {
    let path = app_dir().join("limits-cache.json");
    write_status(
        &path,
        &json!({
            "updatedAt": updated_at,
            "limits": limits,
        }),
    );
}

pub(crate) fn read_limits_cache(now: u64) -> Option<LimitsCache> {
    let raw = fs::read_to_string(app_dir().join("limits-cache.json")).ok()?;
    let value: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()?;
    let updated_at = value.get("updatedAt").and_then(Value::as_u64)?;
    if now.saturating_sub(updated_at) > LIMITS_CACHE_MS {
        return None;
    }
    let mut limits =
        normalize_limit_entries(serde_json::from_value(value.get("limits")?.clone()).ok()?);
    // Cache files written before per-entry stamps carry updated_at_ms == 0; treat
    // them as having the file's age so they expire correctly rather than instantly.
    for entry in &mut limits {
        if entry.updated_at_ms == 0 {
            entry.updated_at_ms = updated_at;
        }
    }
    Some(LimitsCache { updated_at, limits })
}

pub(crate) fn merge_limit_entries(
    cached: &[UsageLimitEntry],
    detected: &[UsageLimitEntry],
    now: u64,
) -> Vec<UsageLimitEntry> {
    let mut merged = normalize_limit_entries(cached.to_vec());
    for mut entry in normalize_limit_entries(detected.to_vec()) {
        entry.updated_at_ms = now;
        if let Some(existing) = merged.iter_mut().find(|item| item.label == entry.label) {
            *existing = entry;
        } else {
            merged.push(entry);
        }
    }
    sort_limit_entries(&mut merged);
    merged
}

pub(crate) fn normalize_limit_entries(entries: Vec<UsageLimitEntry>) -> Vec<UsageLimitEntry> {
    let mut normalized = Vec::new();
    for mut entry in entries {
        let Some(label) = normalize_limit_label(&entry.label) else {
            continue;
        };
        entry.label = label.into();
        if let Some(existing) = normalized
            .iter_mut()
            .find(|item: &&mut UsageLimitEntry| item.label == entry.label)
        {
            *existing = entry;
        } else {
            normalized.push(entry);
        }
    }
    sort_limit_entries(&mut normalized);
    normalized
}

pub(crate) fn normalize_limit_label(label: &str) -> Option<&'static str> {
    match normalize_ui_label(label).as_str() {
        "5h" | "session" | "current session" => Some("5h"),
        "all" | "all models" => Some("All"),
        _ => None,
    }
}

pub(crate) fn sort_limit_entries(entries: &mut [UsageLimitEntry]) {
    entries.sort_by_key(|entry| match entry.label.as_str() {
        "5h" => 0,
        "All" => 1,
        _ => 9,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usage_limits() {
        let names = vec![
            "Plan usage limits".to_string(),
            "Current session".to_string(),
            "Resets in 4 hr 4 min".to_string(),
            "1% used".to_string(),
            "All models".to_string(),
            "Resets Thu 9:00 AM".to_string(),
            "18% used".to_string(),
            "Sonnet only".to_string(),
            "13% used".to_string(),
        ];
        let limits = parse_usage_limits(&names);
        assert_eq!(limits.len(), 2);
        assert_eq!(
            limits_line(
                &limits,
                LimitVisibility {
                    enabled: true,
                    show_5h: true,
                    show_all: true,
                }
            )
            .as_deref(),
            Some("Limits (2): 5h 1% | All 18%")
        );
        assert_eq!(
            limits_line(
                &limits,
                LimitVisibility {
                    enabled: true,
                    show_5h: false,
                    show_all: true,
                }
            )
            .as_deref(),
            Some("Limits (1): All 18%")
        );
        assert_eq!(
            limits_line(
                &[],
                LimitVisibility {
                    enabled: true,
                    show_5h: true,
                    show_all: false,
                }
            )
            .as_deref(),
            None
        );
        assert_eq!(limits[0].reset.as_deref(), Some("in 4 hr 4 min"));
    }

    #[test]
    fn merges_and_normalizes_limit_cache() {
        let cached = vec![
            UsageLimitEntry {
                label: "session".into(),
                used_percent: 2,
                reset: None,
                updated_at_ms: 1_000,
            },
            UsageLimitEntry {
                label: "all".into(),
                used_percent: 18,
                reset: None,
                updated_at_ms: 1_000,
            },
        ];
        let detected = vec![UsageLimitEntry {
            label: "5h".into(),
            used_percent: 3,
            reset: None,
            updated_at_ms: 0,
        }];

        let limits = merge_limit_entries(&cached, &detected, 2_000);
        assert!(limits.iter().all(|entry| entry.updated_at_ms != 0));
        assert_eq!(
            limits
                .iter()
                .find(|entry| entry.label == "5h")
                .unwrap()
                .updated_at_ms,
            2_000
        );
        assert_eq!(
            limits_line(
                &limits,
                LimitVisibility {
                    enabled: true,
                    show_5h: true,
                    show_all: true,
                }
            )
            .as_deref(),
            Some("Limits (2): 5h 3% | All 18%")
        );
    }
}
