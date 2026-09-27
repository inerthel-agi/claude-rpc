// Usage-limit buckets from Desktop and the OAuth API, plus their cache.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::sync::mpsc::{self, Receiver, TryRecvError};

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
    pub(crate) show_fable: bool,
}

pub(crate) fn limit_visibility(config: &ClaudeConfig) -> LimitVisibility {
    LimitVisibility {
        enabled: config.show_limits,
        show_5h: config.show_limit_5h,
        show_all: config.show_limit_all,
        show_fable: config.show_limit_fable,
    }
}

#[cfg(any(windows, test))]
pub(crate) fn parse_usage_limits(names: &[String]) -> Vec<UsageLimitEntry> {
    let mut entries = Vec::new();

    for (index, name) in names.iter().enumerate() {
        if let Some(entry) = parse_usage_button(name) {
            if !entries
                .iter()
                .any(|existing: &UsageLimitEntry| existing.label == entry.label)
            {
                entries.push(entry);
            }
            continue;
        }
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

// The Code tab composer's usage button is always on screen, even with the
// popover closed: "Usage: Context 146.8k / 1M (15%), 6% of 5-hour limit,
// Resets in 4 hr 33 min". Only the 5-hour bucket is exposed there.
#[cfg(any(windows, test))]
pub(crate) fn parse_usage_button(value: &str) -> Option<UsageLimitEntry> {
    let lower = value.to_ascii_lowercase();
    if !lower.starts_with("usage:") {
        return None;
    }
    let segments = lower.split(',').map(str::trim).collect::<Vec<_>>();
    let index = segments
        .iter()
        .position(|segment| segment.ends_with("% of 5-hour limit"))?;
    let used_percent = segments[index]
        .split('%')
        .next()?
        .trim()
        .parse::<u32>()
        .ok()?
        .min(100) as u8;
    let reset = segments
        .get(index + 1)
        .and_then(|segment| segment.strip_prefix("resets "))
        .map(|reset| reset.trim().to_string());
    Some(UsageLimitEntry {
        label: "5h".into(),
        used_percent,
        reset,
        updated_at_ms: 0,
    })
}

#[cfg(any(windows, test))]
pub(crate) fn parse_used_percent(value: &str) -> Option<u8> {
    let lower = value.to_ascii_lowercase();
    // The current usage popover shows a bare "6%" under each bucket label; the
    // older layout said "6% used".
    let bare = lower.trim();
    if bare.len() > 1
        && bare.ends_with('%')
        && bare[..bare.len() - 1].chars().all(|ch| ch.is_ascii_digit())
    {
        return bare[..bare.len() - 1]
            .parse::<u32>()
            .ok()
            .map(|value| value.min(100) as u8);
    }
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
    // limits line — mirror the OAuth path's clamp(0,100).
    digits.parse::<u32>().ok().map(|value| value.min(100) as u8)
}

#[cfg(any(windows, test))]
pub(crate) fn find_limit_label(names: &[String], usage_index: usize) -> Option<(String, usize)> {
    let start = usage_index.saturating_sub(12);
    for index in (start..usage_index).rev() {
        let norm = normalize_ui_label(&names[index]);
        let label = match norm.as_str() {
            "current session" | "5-hour limit" => "5h",
            "all models" => "All",
            // Current popover: "Weekly · all models" and the per-model
            // "Weekly · Fable" bucket; other per-model buckets are ignored.
            _ if norm.starts_with("weekly") && norm.ends_with("all models") => "All",
            _ if norm.starts_with("weekly") && norm.ends_with("fable") => "Fable",
            _ if norm.starts_with("weekly") => return None,
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
    let parts = parts.join(" | ");
    Some(truncate(format!("Limits: {parts}"), 128))
}

pub(crate) fn visible_limit_labels(visibility: LimitVisibility) -> Vec<&'static str> {
    let mut labels = Vec::new();
    if visibility.show_5h {
        labels.push("5h");
    }
    if visibility.show_all {
        labels.push("All");
    }
    if visibility.show_fable {
        labels.push("Fable");
    }
    labels
}

pub(crate) fn current_limits(
    machine: &mut StateMachine,
    detected_limits: &[UsageLimitEntry],
) -> Vec<UsageLimitEntry> {
    let now = now_ms();
    // Load the cache once per run, not on every tick while nothing is known.
    if !machine.limits_cache_loaded {
        machine.limits_cache_loaded = true;
        if let Some(cache) = read_limits_cache(now) {
            machine.cached_limits = cache.limits;
        }
    }

    // OAuth keeps polling (at its own cadence) even while Desktop is scraped:
    // with the usage popover closed Desktop only exposes the 5-hour bucket, and
    // skipping OAuth would let the weekly bucket expire from the display.
    let oauth = maybe_fetch_oauth_limits(machine, now);
    if let Some(usage) = &oauth {
        // A successful fetch without extra_usage means credits are not available.
        machine.credits = usage.credits.clone().map(|credits| UsageCredits {
            updated_at_ms: now,
            ..credits
        });
    }
    let oauth_limits = oauth.map(|usage| usage.limits).unwrap_or_default();
    let before = machine.cached_limits.clone();
    if !oauth_limits.is_empty() {
        machine.cached_limits = merge_limit_entries(&machine.cached_limits, &oauth_limits, now);
    }
    // Desktop is merged last: it is read every scan, so it is the freshest value.
    if !detected_limits.is_empty() {
        machine.cached_limits = merge_limit_entries(&machine.cached_limits, detected_limits, now);
    }
    // Desktop limits are merged on every 250 ms scan: write the cache when a
    // value changes, else only every few minutes to keep its timestamps fresh.
    let refreshed = !oauth_limits.is_empty() || !detected_limits.is_empty();
    if refreshed
        && (!same_limit_values(&before, &machine.cached_limits)
            || now.saturating_sub(machine.limits_cache_written_at) >= LIMITS_CACHE_REWRITE_MS)
    {
        write_limits_cache(now, &machine.cached_limits);
        machine.limits_cache_written_at = now;
    }

    let fresh = fresh_limit_entries(&machine.cached_limits, now);
    record_usage_history(machine, &fresh, now);
    fresh
}

const LIMITS_CACHE_REWRITE_MS: u64 = 5 * 60 * 1_000;

fn same_limit_values(a: &[UsageLimitEntry], b: &[UsageLimitEntry]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.label == b.label && a.used_percent == b.used_percent && a.reset == b.reset
        })
}

const HISTORY_SAMPLE_MS: u64 = 5 * 60 * 1_000;
const HISTORY_WINDOW_MS: u64 = 24 * 60 * 60 * 1_000;

// Keeps the last 24 h of the 5-hour bucket (one sample per 5 minutes) in
// usage-history.json, so the tray chart survives restarts. Local only.
pub(crate) fn record_usage_history(
    machine: &mut StateMachine,
    limits: &[UsageLimitEntry],
    now: u64,
) {
    let path = app_dir().join("usage-history.json");
    if !machine.history_loaded {
        machine.history_loaded = true;
        machine.history_5h = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Vec<[u64; 2]>>(&raw).ok())
            .unwrap_or_default();
    }
    let Some(entry) = limits.iter().find(|entry| entry.label == "5h") else {
        return;
    };
    // After the clock was set back, samples dated in the future would block
    // new ones until the clock caught up: drop them.
    machine.history_5h.retain(|sample| sample[0] <= now);
    let last = machine
        .history_5h
        .last()
        .map(|sample| sample[0])
        .unwrap_or(0);
    if now.saturating_sub(last) < HISTORY_SAMPLE_MS {
        return;
    }
    machine
        .history_5h
        .push([now, u64::from(entry.used_percent)]);
    machine
        .history_5h
        .retain(|sample| now.saturating_sub(sample[0]) <= HISTORY_WINDOW_MS);
    write_status(&path, &json!(machine.history_5h));
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

pub(crate) fn maybe_fetch_oauth_limits(machine: &mut StateMachine, now: u64) -> Option<OAuthUsage> {
    // The HTTP call runs on its own thread (up to 8 s): the daemon loop only
    // collects the result, so presence, status and Quit never wait on it.
    let polled = machine.oauth_inflight.as_ref().map(Receiver::try_recv);
    match polled {
        Some(Ok(result)) => {
            machine.oauth_inflight = None;
            return match result {
                Ok(usage) => Some(usage),
                Err(OAuthFetchError::RateLimited) => {
                    machine.oauth_backoff_until_ms = now + OAUTH_USAGE_BACKOFF_MS;
                    None
                }
                Err(_) => None,
            };
        }
        Some(Err(TryRecvError::Empty)) => return None,
        Some(Err(TryRecvError::Disconnected)) => machine.oauth_inflight = None,
        None => {}
    }
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
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(fetch_oauth_usage());
    });
    machine.oauth_inflight = Some(receiver);
    None
}

// Extra usage ("usage credits") spent in the billing period, from the OAuth
// usage endpoint. Amounts are in minor units of `currency` (cents for USD);
// the currency is the account's billing currency, never guessed from location.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageCredits {
    pub(crate) enabled: bool,
    pub(crate) used: Option<u64>,
    pub(crate) limit: Option<u64>,
    // None when the API gives no valid code: shown as a plain amount.
    pub(crate) currency: Option<String>,
    #[serde(skip)]
    pub(crate) updated_at_ms: u64,
}

pub(crate) struct OAuthUsage {
    pub(crate) limits: Vec<UsageLimitEntry>,
    pub(crate) credits: Option<UsageCredits>,
}

// `extra_usage` is absent or null when the plan has no extra usage.
pub(crate) fn parse_extra_usage(body: &Value) -> Option<UsageCredits> {
    let extra = body.get("extra_usage")?.as_object()?;
    let amount = |key: &str| {
        extra
            .get(key)
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|value| value.round() as u64)
    };
    // An ISO 4217 code; anything else would break the UI's currency formatting.
    let currency = extra
        .get("currency")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|code| code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic()))
        .map(str::to_ascii_uppercase);
    Some(UsageCredits {
        enabled: extra
            .get("is_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        used: amount("used_credits"),
        limit: amount("monthly_limit"),
        currency,
        updated_at_ms: 0,
    })
}

// Credits are shown only while fresh, like the limit buckets.
pub(crate) fn fresh_credits(machine: &StateMachine, now: u64) -> Option<UsageCredits> {
    machine
        .credits
        .clone()
        .filter(|credits| now.saturating_sub(credits.updated_at_ms) <= LIMITS_DISPLAY_TTL_MS)
}

pub(crate) enum OAuthFetchError {
    NoToken,
    Network,
    RateLimited,
    Parse,
}

pub(crate) fn fetch_oauth_usage() -> Result<OAuthUsage, OAuthFetchError> {
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
    Ok(OAuthUsage {
        limits: parse_oauth_usage_response(&value),
        credits: parse_extra_usage(&value),
    })
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
    // Per-model weekly caps only appear in the `limits` array, e.g.
    // {"kind":"weekly_scoped","percent":0,"scope":{"model":{"display_name":"Fable"}}}.
    if let Some(limits) = body.get("limits").and_then(Value::as_array) {
        for limit in limits {
            let is_fable = limit.get("kind").and_then(Value::as_str) == Some("weekly_scoped")
                && limit
                    .pointer("/scope/model/display_name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.eq_ignore_ascii_case("fable"));
            let Some(percent) = limit.get("percent").and_then(Value::as_f64) else {
                continue;
            };
            if is_fable {
                entries.push(UsageLimitEntry {
                    label: "Fable".into(),
                    used_percent: percent.round().clamp(0.0, 100.0) as u8,
                    reset: limit
                        .get("resets_at")
                        .and_then(Value::as_str)
                        .map(String::from),
                    updated_at_ms: 0,
                });
                break;
            }
        }
    }
    entries
}

pub(crate) fn extract_oauth_usage_percent(bucket: &Value) -> Option<u8> {
    // `utilization` is always a 0..100 percentage in the OAuth usage API.
    if let Some(raw) = bucket.get("utilization").and_then(Value::as_f64) {
        return Some(raw.round().clamp(0.0, 100.0) as u8);
    }
    // Keys named "percent" are percentages, so 1 means 1%, not 100%.
    for key in ["percent_used", "used_percent", "percent"] {
        if let Some(raw) = bucket.get(key).and_then(Value::as_f64) {
            return Some(raw.round().clamp(0.0, 100.0) as u8);
        }
    }
    // Generic keys may carry a 0..1 ratio; only a value strictly below 1 is
    // read as one, so an exact 1 is read as 1%.
    for key in ["usage", "value"] {
        if let Some(raw) = bucket.get(key).and_then(Value::as_f64) {
            let pct = if raw < 1.0 { raw * 100.0 } else { raw };
            return Some(pct.round().clamp(0.0, 100.0) as u8);
        }
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
        "fable" => Some("Fable"),
        _ => None,
    }
}

pub(crate) fn sort_limit_entries(entries: &mut [UsageLimitEntry]) {
    entries.sort_by_key(|entry| match entry.label.as_str() {
        "5h" => 0,
        "All" => 1,
        "Fable" => 2,
        _ => 9,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expires_stale_limit_buckets() {
        let entry = |label: &str, updated_at_ms| UsageLimitEntry {
            label: label.into(),
            used_percent: 10,
            reset: None,
            updated_at_ms,
        };
        let now = 10 * LIMITS_DISPLAY_TTL_MS;
        let fresh = fresh_limit_entries(
            &[
                entry("5h", now - 1_000),
                entry("All", now - LIMITS_DISPLAY_TTL_MS - 1),
            ],
            now,
        );
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].label, "5h");
    }

    #[test]
    fn reads_fallback_usage_percent_keys() {
        // A percentage key of 1 is 1%, never 100%.
        assert_eq!(
            extract_oauth_usage_percent(&json!({"percent_used": 1})),
            Some(1)
        );
        assert_eq!(
            extract_oauth_usage_percent(&json!({"percent": 42.4})),
            Some(42)
        );
        // Generic keys accept a 0..1 ratio.
        assert_eq!(
            extract_oauth_usage_percent(&json!({"usage": 0.25})),
            Some(25)
        );
        assert_eq!(extract_oauth_usage_percent(&json!({"value": 1})), Some(1));
        assert_eq!(
            extract_oauth_usage_percent(&json!({"utilization": 150.0})),
            Some(100)
        );
        assert_eq!(extract_oauth_usage_percent(&json!({})), None);
    }

    #[test]
    fn parses_oauth_fable_weekly_limit() {
        let body = json!({
            "five_hour": {"utilization": 11.0, "resets_at": "2026-09-25T20:19:59+00:00"},
            "seven_day": {"utilization": 1.0, "resets_at": "2026-09-27T01:59:59+00:00"},
            "limits": [
                {"kind": "session", "percent": 11, "scope": null},
                {"kind": "weekly_all", "percent": 1, "scope": null},
                {
                    "kind": "weekly_scoped",
                    "percent": 4,
                    "resets_at": "2026-09-27T01:59:59+00:00",
                    "scope": {"model": {"id": null, "display_name": "Fable"}, "surface": null}
                }
            ]
        });
        let limits = parse_oauth_usage_response(&body);
        let labels = limits
            .iter()
            .map(|entry| (entry.label.as_str(), entry.used_percent))
            .collect::<Vec<_>>();
        assert_eq!(labels, [("5h", 11), ("All", 1), ("Fable", 4)]);
    }

    #[test]
    fn parses_current_desktop_usage_layout() {
        // Popover open: bare percentages under "5-hour limit", "Weekly · all
        // models" and the per-model "Weekly · Fable" bucket.
        let names = [
            "Usage: Context 146.8k / 1M (15%), 6% of 5-hour limit, Resets in 4 hr 33 min",
            "Plan usage limits",
            "Max (5x)",
            "5-hour limit",
            "6%",
            "5-hour limit",
            "Weekly \u{b7} all models",
            "Resets Sun 4:00 AM",
            "3%",
            "Weekly \u{b7} all models",
            "Weekly \u{b7} Fable",
            "Resets Sun 4:00 AM",
            "9%",
        ]
        .map(String::from);
        let limits = parse_usage_limits(&names);
        assert_eq!(limits.len(), 3);
        assert_eq!(
            (limits[2].label.as_str(), limits[2].used_percent),
            ("Fable", 9)
        );
        assert_eq!(
            (limits[0].label.as_str(), limits[0].used_percent),
            ("5h", 6)
        );
        assert_eq!(limits[0].reset.as_deref(), Some("in 4 hr 33 min"));
        assert_eq!(
            (limits[1].label.as_str(), limits[1].used_percent),
            ("All", 3)
        );

        // Popover closed: only the composer button is on screen.
        let limits = parse_usage_limits(&[
            "Usage: Context 117.9k / 1M (12%), 2% of 5-hour limit, Resets in 4 hr 44 min".into(),
        ]);
        assert_eq!(limits.len(), 1);
        assert_eq!(
            (limits[0].label.as_str(), limits[0].used_percent),
            ("5h", 2)
        );
    }

    #[test]
    fn parses_extra_usage_credits() {
        let body = json!({
            "extra_usage": {
                "is_enabled": true,
                "monthly_limit": 4000,
                "used_credits": 1250.0,
                "utilization": 31.25,
                "currency": "eur"
            }
        });
        assert_eq!(
            parse_extra_usage(&body),
            Some(UsageCredits {
                enabled: true,
                used: Some(1250),
                limit: Some(4000),
                currency: Some("EUR".into()),
                updated_at_ms: 0,
            })
        );

        // Disabled, unlimited, no currency: no amounts, currency unknown.
        let body = json!({"extra_usage": {"is_enabled": false, "monthly_limit": null,
            "used_credits": null, "currency": null}});
        let credits = parse_extra_usage(&body).unwrap();
        assert!(!credits.enabled);
        assert_eq!((credits.used, credits.limit), (None, None));
        assert_eq!(credits.currency, None);

        // Invalid currency codes and negative amounts are not trusted.
        let body = json!({"extra_usage": {"used_credits": -5, "currency": "€UR"}});
        let credits = parse_extra_usage(&body).unwrap();
        assert_eq!(credits.used, None);
        assert_eq!(credits.currency, None);

        assert_eq!(parse_extra_usage(&json!({"extra_usage": null})), None);
        assert_eq!(parse_extra_usage(&json!({})), None);
    }

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
                    show_fable: false,
                }
            )
            .as_deref(),
            Some("Limits: 5h 1% | All 18%")
        );
        assert_eq!(
            limits_line(
                &limits,
                LimitVisibility {
                    enabled: true,
                    show_5h: false,
                    show_all: true,
                    show_fable: false,
                }
            )
            .as_deref(),
            Some("Limits: All 18%")
        );
        assert_eq!(
            limits_line(
                &[],
                LimitVisibility {
                    enabled: true,
                    show_5h: true,
                    show_all: false,
                    show_fable: false,
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
                    show_fable: false,
                }
            )
            .as_deref(),
            Some("Limits: 5h 3% | All 18%")
        );
    }
}
