//! Text and numbers shown in the settings window and the tray panel, derived
//! from the daemon's status file (status.txt, JSON) and the saved settings.

use crate::{config::ClaudeConfig, Update};
use serde_json::Value;

const DAY_MS: i64 = 86_400_000;

pub(crate) fn tr(french: bool, en: &str, fr: &str) -> String {
    if french { fr } else { en }.to_string()
}

fn text<'a>(status: &'a Value, key: &str) -> &'a str {
    status.get(key).and_then(Value::as_str).unwrap_or("")
}

/// "Discord: Connected (inerthel)" with prefix "Discord:" -> "inerthel".
fn inner<'a>(line: &'a str, prefix: &str) -> &'a str {
    let Some(rest) = line.strip_prefix(prefix) else {
        return "";
    };
    let rest = rest.trim();
    match rest.find('(') {
        Some(open) if rest.ends_with(')') => &rest[open + 1..rest.len() - 1],
        _ => "",
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// Whether a Claude client runs, and its short name ("Desktop · Code + CLI").
pub(crate) fn client(status: &Value, french: bool) -> (bool, String) {
    let line = text(status, "claudeLine");
    let name = line.strip_prefix("Claude:").unwrap_or(line).trim();
    if name.is_empty() || name == "Off" {
        return (false, tr(french, "Off", "Inactif"));
    }
    if !name.starts_with("Desktop") {
        return (true, "Claude Code".into());
    }
    let mut label = "Desktop".to_string();
    let mode = inner(line, "Claude: Desktop");
    if !mode.is_empty() {
        label = format!("{label} · {mode}");
    }
    if status.get("cliAlongside").and_then(Value::as_bool) == Some(true) {
        label.push_str(" + CLI");
    }
    (true, label)
}

/// "Claude Opus 5.5 | High | Claude Max" -> ("Opus 5.5", "High").
fn model(status: &Value) -> (String, String) {
    const EFFORTS: [&str; 7] = [
        "low",
        "medium",
        "high",
        "extra high",
        "xhigh",
        "max",
        "ultracode",
    ];
    let mut parts = text(status, "modelLine").split(" | ");
    let name = parts.next().unwrap_or("");
    let name = if name == "Auto-detect" {
        ""
    } else {
        name.strip_prefix("Claude ").unwrap_or(name)
    };
    let effort = parts
        .find(|part| EFFORTS.contains(&part.to_lowercase().as_str()))
        .unwrap_or("");
    (name.to_string(), effort.to_string())
}

/// "Provider: Subscription (Claude Max)" -> "Claude Max"; empty when unknown.
pub(crate) fn plan(status: &Value) -> String {
    let line = text(status, "providerLine");
    let inside = inner(line, "Provider:");
    let plan = if inside.is_empty() {
        let rest = line.strip_prefix("Provider:").unwrap_or(line).trim();
        rest.split(" (").next().unwrap_or("")
    } else {
        inside
    };
    if plan == "Unknown" { "" } else { plan }.to_string()
}

/// Claude Ultra has no 5-hour limit, only the weekly ones.
pub(crate) fn has_five_hour_limit(status: &Value) -> bool {
    plan(status) != "Claude Ultra"
}

pub(crate) fn discord_user(status: &Value) -> String {
    let line = text(status, "discordLine");
    if !line.starts_with("Discord: Connected") {
        return String::new();
    }
    match inner(line, "Discord:") {
        "" => "Discord".into(),
        user => user.into(),
    }
}

// ---------- Time ----------

/// Local time minus UTC. Read once: a DST change is picked up at the next start.
pub(crate) fn local_offset_ms() -> i64 {
    use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
    static OFFSET: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *OFFSET.get_or_init(|| {
        let (local, utc) = unsafe { (GetLocalTime(), GetSystemTime()) };
        let minutes = |hour: u16, minute: u16| hour as i64 * 60 + minute as i64;
        let day_shift =
            match (local.wYear, local.wMonth, local.wDay).cmp(&(utc.wYear, utc.wMonth, utc.wDay)) {
                std::cmp::Ordering::Greater => 1440,
                std::cmp::Ordering::Less => -1440,
                std::cmp::Ordering::Equal => 0,
            };
        let offset =
            minutes(local.wHour, local.wMinute) - minutes(utc.wHour, utc.wMinute) + day_shift;
        // The two clocks are read a moment apart; real offsets are multiples of 15 minutes.
        (offset as f64 / 15.0).round() as i64 * 15 * 60_000
    })
}

/// "21:14" in local time.
pub(crate) fn local_clock(ms: u64, offset_ms: i64) -> String {
    let minutes = (ms as i64 + offset_ms).div_euclid(60_000);
    format!(
        "{:02}:{:02}",
        minutes.div_euclid(60).rem_euclid(24),
        minutes.rem_euclid(60)
    )
}

fn local_day(ms: u64, offset_ms: i64) -> i64 {
    (ms as i64 + offset_ms).div_euclid(DAY_MS)
}

/// "21:14" today, "Sun 21:14" on another day.
fn clock_or_day(ms: u64, now: u64, offset_ms: i64, french: bool) -> String {
    let clock = local_clock(ms, offset_ms);
    let day = local_day(ms, offset_ms);
    if day == local_day(now, offset_ms) {
        return clock;
    }
    // 1970-01-01 was a Thursday.
    let names = if french {
        ["jeu.", "ven.", "sam.", "dim.", "lun.", "mar.", "mer."]
    } else {
        ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"]
    };
    format!("{} {clock}", names[day.rem_euclid(7) as usize])
}

/// Next local midnight, for the tray's "Tomorrow" pause.
pub(crate) fn next_midnight(now: u64, offset_ms: i64) -> u64 {
    ((local_day(now, offset_ms) + 1) * DAY_MS - offset_ms) as u64
}

/// "3h 12m" / "3 h 12", "2d 4h" / "2 j 4 h", "40m" / "40 min".
fn duration(minutes: u64, french: bool) -> String {
    let (days, hours, mins) = (minutes / 1440, minutes % 1440 / 60, minutes % 60);
    match (french, days, hours) {
        (true, 1.., _) => format!("{days} j {hours} h"),
        (true, 0, 1..) => format!("{hours} h {mins:02}"),
        (true, 0, 0) => format!("{mins} min"),
        (false, 1.., _) => format!("{days}d {hours}h"),
        (false, 0, 1..) => format!("{hours}h {mins}m"),
        (false, 0, 0) => format!("{mins}m"),
    }
}

/// "2026-11-23T22:00:00.123+01:00" -> Unix milliseconds.
fn parse_iso_ms(value: &str) -> Option<u64> {
    let number = |range: std::ops::Range<usize>| value.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    // Days since 1970-01-01 (Howard Hinnant's days-from-civil).
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    let zone = value
        .get(19..)?
        .trim_start_matches(|ch: char| ch == '.' || ch.is_ascii_digit());
    let offset_minutes = match zone.as_bytes().first() {
        None | Some(b'Z') => 0,
        Some(sign @ (b'+' | b'-')) => {
            let hours: i64 = zone.get(1..3)?.parse().ok()?;
            let minutes: i64 = zone.get(4..6).and_then(|m| m.parse().ok()).unwrap_or(0);
            (hours * 60 + minutes) * if *sign == b'-' { -1 } else { 1 }
        }
        _ => return None,
    };
    let seconds = days * 86_400 + hour * 3600 + minute * 60 + second - offset_minutes * 60;
    u64::try_from(seconds * 1000).ok()
}

/// Minutes in Claude Desktop's reset text: "in 4 hr 31 min", "in 2 hours", "in 12 min".
fn relative_minutes(reset: &str) -> Option<u64> {
    let rest = reset.trim().to_lowercase();
    let rest = rest.strip_prefix("in ")?;
    let (mut total, mut number, mut found) = (0u64, None::<u64>, false);
    for word in rest.split_whitespace() {
        let digits: String = word.chars().take_while(char::is_ascii_digit).collect();
        let unit = &word[digits.len()..];
        if !digits.is_empty() {
            number = digits.parse().ok();
        }
        let scale = match unit {
            _ if unit.starts_with('h') => 60,
            _ if unit.starts_with("min") => 1,
            _ => continue,
        };
        total += number.take()? * scale;
        found = true;
    }
    found.then_some(total)
}

/// ("resets in 3h 12m", "Sun 04:00"). OAuth resets are ISO dates; Claude
/// Desktop gives free text, which has no fixed end to show as a clock time.
fn reset_parts(reset: &str, now: u64, offset_ms: i64, french: bool) -> Option<(String, String)> {
    if reset.is_empty() {
        return None;
    }
    let resets_in = |minutes: u64| {
        let duration = duration(minutes, french);
        if french {
            format!("remise à zéro dans {duration}")
        } else {
            format!("resets in {duration}")
        }
    };
    if let Some(minutes) = relative_minutes(reset) {
        return Some((resets_in(minutes), String::new()));
    }
    match parse_iso_ms(reset) {
        Some(at) => Some((
            resets_in((at.saturating_sub(now) + 30_000) / 60_000),
            clock_or_day(at, now, offset_ms, french),
        )),
        None if french => Some((format!("remise à zéro {reset}"), String::new())),
        None => Some((format!("resets {reset}"), String::new())),
    }
}

// ---------- Limits ----------

/// One usage gauge; mirrors the `Limit` struct in app.slint.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LimitView {
    pub(crate) name: String,
    pub(crate) figure: String,
    pub(crate) percent: f32,
    /// 0 fine, 1 from 80 %, 2 from 95 %, 3 not reported.
    pub(crate) level: i32,
    pub(crate) reset_in: String,
    pub(crate) reset_at: String,
    pub(crate) chart: bool,
}

struct RawLimit<'a> {
    label: &'a str,
    percent: u64,
    reset: &'a str,
}

fn raw_limits(status: &Value) -> Vec<RawLimit<'_>> {
    status
        .get("limits")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    Some(RawLimit {
                        label: entry.get("label")?.as_str()?,
                        percent: entry
                            .get("usedPercent")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0)
                            .clamp(0.0, 100.0)
                            .round() as u64,
                        reset: entry.get("reset").and_then(Value::as_str).unwrap_or(""),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn limit_name(label: &str, french: bool) -> String {
    match label {
        "5h" => tr(french, "5-hour session", "Session de 5 h"),
        "All" => tr(french, "Weekly", "Semaine"),
        "Fable" => tr(french, "Fable weekly", "Fable (semaine)"),
        other => other.to_string(),
    }
}

fn level(percent: u64) -> i32 {
    match percent {
        95.. => 2,
        80.. => 1,
        _ => 0,
    }
}

fn limit_view(limit: &RawLimit, now: u64, offset_ms: i64, french: bool) -> LimitView {
    let (reset_in, reset_at) = reset_parts(limit.reset, now, offset_ms, french).unwrap_or_default();
    LimitView {
        name: limit_name(limit.label, french),
        figure: format!("{}%", limit.percent),
        percent: limit.percent as f32 / 100.0,
        level: level(limit.percent),
        reset_in: capitalize(&reset_in),
        reset_at,
        chart: limit.label == "5h",
    }
}

/// The Fable weekly limit only comes from the account usage request or the
/// Claude Desktop usage panel: its row stays, and says why it has no value.
fn missing_fable(note: String, french: bool) -> LimitView {
    LimitView {
        name: limit_name("Fable", french),
        figure: "—".into(),
        level: 3,
        reset_in: note,
        ..LimitView::default()
    }
}

/// The 24-hour chart of the 5-hour session: line and area as SVG path
/// commands drawn in a `width` x `height` box, and the peak.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Spark {
    pub(crate) line: String,
    pub(crate) area: String,
    pub(crate) peak: String,
}

fn spark(status: &Value, now: u64, width: f32, height: f32, french: bool) -> Option<Spark> {
    // The daemon samples the 5-hour bucket every 5 minutes: [time ms, percent].
    let points: Vec<(f64, f64)> = status
        .get("history5h")?
        .as_array()?
        .iter()
        .filter_map(|point| Some((point.get(0)?.as_f64()?, point.get(1)?.as_f64()?)))
        .filter(|(time, _)| now as f64 - time <= DAY_MS as f64)
        .collect();
    if points.len() < 2 || width < 2.0 {
        return None;
    }
    let (width, height) = (width as f64, height as f64);
    let start = points[0].0;
    let span = (points[points.len() - 1].0 - start).max(1.0);
    let line = points
        .iter()
        .enumerate()
        .map(|(index, (time, percent))| {
            let x = (time - start) / span * width;
            let y = height - 2.0 - percent.clamp(0.0, 100.0) / 100.0 * (height - 4.0);
            format!("{}{x:.1} {y:.1}", if index == 0 { "M" } else { " L" })
        })
        .collect::<String>();
    let peak = points
        .iter()
        .map(|(_, percent)| *percent)
        .fold(0.0, f64::max);
    Some(Spark {
        area: format!("{line} L{width:.1} {height:.1} L0 {height:.1} Z"),
        line,
        peak: if french {
            format!("Pic {peak:.0} %")
        } else {
            format!("Peak {peak:.0}%")
        },
    })
}

// ---------- Credits ----------

/// "$12.34" / "12,34 €"; amounts come in minor units of the account's currency.
fn money(minor: f64, currency: &str, french: bool) -> String {
    // shortcut: only yen and won are treated as having no minor unit; add
    // other zero-decimal currencies here if Claude ever bills in them.
    let decimals = if matches!(currency, "JPY" | "KRW") {
        0
    } else {
        2
    };
    let mut amount = format!("{:.*}", decimals, minor / 10f64.powi(decimals as i32));
    if french {
        amount = amount.replace('.', ",");
    }
    let symbol = match currency {
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        other => other,
    };
    match (symbol == currency, french) {
        // An unknown currency keeps its code, never a guessed symbol.
        (true, _) | (_, true) => format!("{amount} {symbol}").trim().to_string(),
        (false, false) => format!("{symbol}{amount}"),
    }
}

struct Credits {
    enabled: bool,
    used: String,
    limit: String,
    percent: Option<f32>,
    currency: String,
}

fn credits(status: &Value, french: bool) -> Option<Credits> {
    let raw = status.get("credits")?.as_object()?;
    let amount = |key: &str| {
        raw.get(key)
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value >= 0.0)
    };
    let currency = raw.get("currency").and_then(Value::as_str).unwrap_or("");
    let (used, limit) = (amount("used"), amount("limit"));
    Some(Credits {
        enabled: raw.get("enabled").and_then(Value::as_bool) != Some(false),
        used: used
            .map(|value| money(value, currency, french))
            .unwrap_or_default(),
        limit: limit
            .map(|value| money(value, currency, french))
            .unwrap_or_default(),
        percent: match (used, limit) {
            (Some(used), Some(limit)) if limit > 0.0 => Some((used / limit).min(1.0) as f32),
            _ => None,
        },
        currency: currency.to_string(),
    })
}

/// The "Usage credits" card of the Usage page.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CreditsView {
    pub(crate) value: String,
    pub(crate) of: String,
    pub(crate) percent: f32,
    pub(crate) level: i32,
    pub(crate) note: String,
}

fn credits_view(status: &Value, french: bool) -> Option<CreditsView> {
    let credits = credits(status, french)?;
    if !credits.enabled {
        return Some(CreditsView {
            value: tr(french, "Off", "Désactivés"),
            of: String::new(),
            percent: -1.0,
            level: 0,
            note: tr(
                french,
                "Turn them on in your Claude account to keep working past your plan limits.",
                "Active-les dans ton compte Claude pour continuer au-delà des limites de ton forfait.",
            ),
        });
    }
    let capped = !credits.limit.is_empty();
    let mut note = match (capped, french) {
        (true, false) => "Spent automatically past your plan limits, up to this cap.",
        (true, true) => {
            "Utilisés automatiquement au-delà des limites du forfait, jusqu’à ce plafond."
        }
        (false, false) => "Spent automatically past your plan limits.",
        (false, true) => "Utilisés automatiquement au-delà des limites du forfait.",
    }
    .to_string();
    if !credits.currency.is_empty() {
        note = if french {
            format!("{note} Facturés en {}.", credits.currency)
        } else {
            format!("{note} Billed in {}.", credits.currency)
        };
    }
    Some(CreditsView {
        value: if credits.used.is_empty() {
            "—".into()
        } else {
            credits.used
        },
        of: match (capped, french) {
            (true, false) => format!("of {} this month", credits.limit),
            (true, true) => format!("sur {} ce mois-ci", credits.limit),
            (false, false) => "spent this month".into(),
            (false, true) => "dépensés ce mois-ci".into(),
        },
        percent: credits.percent.unwrap_or(-1.0),
        level: level((credits.percent.unwrap_or(0.0) * 100.0) as u64),
        note,
    })
}

// ---------- Settings window ----------

/// Mirror of config.rs `clean_url`: only http(s) buttons reach Discord.
fn is_web_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// What is wrong with a profile button as typed, or an empty string.
pub(crate) fn button_error(label: &str, url: &str, french: bool) -> String {
    let (label, url) = (label.trim(), url.trim());
    if !url.is_empty() && !is_web_url(url) {
        tr(
            french,
            "The URL must start with https://",
            "L’adresse doit commencer par https://",
        )
    } else if label.is_empty() != url.is_empty() {
        tr(
            french,
            "Add both a label and a URL.",
            "Ajoute un libellé et une adresse.",
        )
    } else {
        String::new()
    }
}

pub(crate) fn broken_config(french: bool) -> String {
    tr(
        french,
        "config.json cannot be read. Fix or delete it; nothing is published meanwhile.",
        "config.json est illisible. Corrige-le ou supprime-le ; rien n’est publié en attendant.",
    )
}

/// The Discord card of the Presence page.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Preview {
    /// Nothing reaches Discord right now; `primary` says why.
    pub(crate) off: bool,
    pub(crate) activity: String,
    pub(crate) primary: String,
    pub(crate) secondary: String,
    pub(crate) tertiary: String,
    pub(crate) buttons: Vec<String>,
}

fn preview(
    status: &Value,
    config: &ClaudeConfig,
    broken: bool,
    now: u64,
    offset_ms: i64,
    french: bool,
) -> Preview {
    let header = text(status, "previewHeader");
    let hidden = if broken {
        broken_config(french)
    } else if config.dnd {
        tr(french, "Do Not Disturb is on", "Ne pas déranger est activé")
    } else if config.pause_until_ms > now {
        let time = local_clock(config.pause_until_ms, offset_ms);
        if french {
            format!("En pause jusqu’à {time}")
        } else {
            format!("Paused until {time}")
        }
    } else {
        match text(status, "hiddenReason") {
            "private" => tr(french, "Private project", "Projet privé"),
            "chat" => tr(french, "Chat tab is hidden", "Onglet Chat masqué"),
            _ if header.is_empty() => tr(french, "Claude is not running", "Claude n’est pas lancé"),
            _ => String::new(),
        }
    };
    if !hidden.is_empty() {
        return Preview {
            off: true,
            activity: tr(french, "Not shown", "Non affiché"),
            primary: hidden,
            secondary: tr(
                french,
                "Nothing is shown on your profile right now.",
                "Rien n’est affiché sur ton profil pour l’instant.",
            ),
            tertiary: String::new(),
            buttons: Vec::new(),
        };
    }
    Preview {
        off: false,
        activity: header.to_string(),
        primary: text(status, "previewPrimary").to_string(),
        secondary: text(status, "previewSecondary").to_string(),
        tertiary: text(status, "previewTertiary").to_string(),
        // The mode the daemon actually rendered, not the unsaved form.
        buttons: config
            .buttons
            .iter()
            .filter(|button| header.starts_with("Watching") && is_web_url(button.url.trim()))
            .take(2)
            .map(|button| button.label.clone())
            .collect(),
    }
}

/// "12:34 elapsed" under the Discord card, or empty.
pub(crate) fn elapsed(status: &Value, now: u64, french: bool) -> String {
    let Some(started) = status.get("startedAtMs").and_then(Value::as_u64) else {
        return String::new();
    };
    let seconds = now.saturating_sub(started) / 1000;
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    let clock = if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    };
    if french {
        format!("{clock} écoulées")
    } else {
        format!("{clock} elapsed")
    }
}

/// Everything in the settings window that follows the status file, except the
/// elapsed timer, so the window is only repainted when something changed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StatusView {
    pub(crate) client_on: bool,
    pub(crate) client_label: String,
    pub(crate) discord_user: String,
    pub(crate) plan: String,
    pub(crate) has_5h: bool,
    pub(crate) limits: Vec<LimitView>,
    pub(crate) spark: Option<Spark>,
    pub(crate) credits: Option<CreditsView>,
    pub(crate) preview: Preview,
}

impl StatusView {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        status: &Value,
        config: &ClaudeConfig,
        broken: bool,
        chart_width: f32,
        now: u64,
        offset_ms: i64,
        french: bool,
    ) -> Self {
        let (client_on, label) = client(status, french);
        let raw = raw_limits(status);
        let mut limits: Vec<LimitView> = raw
            .iter()
            .map(|limit| limit_view(limit, now, offset_ms, french))
            .collect();
        if !raw.is_empty() && !raw.iter().any(|limit| limit.label == "Fable") {
            limits.push(missing_fable(
                tr(
                    french,
                    "Not reported right now. Sign in again in Claude Code (/login), or open the usage panel in Claude Desktop.",
                    "Non transmis pour l’instant. Reconnecte-toi dans Claude Code (/login), ou ouvre le panneau d’usage de Claude Desktop.",
                ),
                french,
            ));
        }
        Self {
            client_on,
            client_label: if client_on {
                label
            } else {
                tr(french, "Not running", "Pas lancé")
            },
            discord_user: discord_user(status),
            plan: plan(status),
            has_5h: has_five_hour_limit(status),
            limits,
            spark: spark(status, now, chart_width, 64.0, french),
            credits: credits_view(status, french),
            preview: preview(status, config, broken, now, offset_ms, french),
        }
    }
}

/// The line under the version on the General page.
pub(crate) fn update_status(update: &Update, french: bool) -> String {
    match (&update.latest, &update.error) {
        (Some(version), _) if french => format!("Mise à jour disponible : v{version}"),
        (Some(version), _) => format!("Update available: v{version}"),
        _ if update.checking => tr(french, "Checking…", "Vérification…"),
        (None, Some(error)) if french => format!("Échec de la vérification : {error}"),
        (None, Some(error)) => format!("Check failed: {error}"),
        (None, None) if update.checked_ms > 0 => tr(french, "You are up to date.", "Tu es à jour."),
        (None, None) => String::new(),
    }
}

// ---------- Tray panel ----------

/// What the tray panel shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PanelView {
    pub(crate) model_line: String,
    pub(crate) session_line: String,
    pub(crate) client_on: bool,
    pub(crate) client_label: String,
    /// The 5-hour session leads (the weekly limit on Claude Ultra).
    pub(crate) hero: Option<LimitView>,
    pub(crate) plan: String,
    pub(crate) alert: String,
    pub(crate) alert_level: i32,
    pub(crate) spark: Option<(String, String, String)>,
    pub(crate) limits: Vec<LimitView>,
    pub(crate) credits: String,
    pub(crate) credits_percent: f32,
    pub(crate) credits_level: i32,
    pub(crate) paused: bool,
    pub(crate) pause_state: String,
    pub(crate) mode: String,
    pub(crate) update_label: String,
    pub(crate) update_ready: bool,
    /// Discord line as text before the user name, the name in bold, and the rest.
    pub(crate) discord: [String; 3],
}

/// How long the result of a manual update check stays on the tray item.
const CHECK_NOTICE_MS: u64 = 4_000;

pub(crate) fn panel_view(
    status: &Value,
    config: &ClaudeConfig,
    update: &Update,
    chart_width: f32,
    now: u64,
    offset_ms: i64,
    french: bool,
) -> PanelView {
    let (client_on, client_label) = client(status, french);
    let (model, effort) = model(status);
    let model_line = if client_on {
        [if model.is_empty() { "Claude" } else { &model }, &effort]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    } else {
        tr(french, "Waiting for Claude", "En attente de Claude")
    };
    let session_line = if client_on {
        [text(status, "sessionTitle"), text(status, "gitBranch")]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    } else {
        String::new()
    };

    let raw = raw_limits(status);
    let main = raw.iter().find(|limit| limit.label == "5h").or(raw.first());
    let five_hour = main.is_some_and(|limit| limit.label == "5h");
    // Same limit as the Windows alerts: the 5-hour session, else the weekly one.
    let alert_level = match main {
        Some(limit) if five_hour || limit.label == "All" => level(limit.percent),
        _ => 0,
    };
    let alert = match (alert_level, five_hour, french) {
        (0, _, _) => String::new(),
        (level, true, false) => format!(
            "Above {}% of your 5-hour session",
            if level == 2 { 95 } else { 80 }
        ),
        (level, true, true) => format!(
            "Plus de {} % de ta session de 5 h",
            if level == 2 { 95 } else { 80 }
        ),
        (level, false, false) => format!(
            "Above {}% of your weekly limit",
            if level == 2 { 95 } else { 80 }
        ),
        (level, false, true) => format!(
            "Plus de {} % de ta limite hebdomadaire",
            if level == 2 { 95 } else { 80 }
        ),
    };
    let mut limits: Vec<LimitView> = raw
        .iter()
        .filter(|limit| main.is_none_or(|main| !std::ptr::eq(*limit, main)))
        .map(|limit| {
            let mut view = limit_view(limit, now, offset_ms, french);
            // Compact rows: one line, the clock time when there is one.
            if view.reset_at.is_empty() {
                view.reset_at = std::mem::take(&mut view.reset_in);
            }
            view
        })
        .collect();
    if main.is_some() && !raw.iter().any(|limit| limit.label == "Fable") {
        let mut fable = missing_fable(String::new(), french);
        fable.reset_at = tr(french, "Not reported", "Non transmis");
        limits.push(fable);
    }

    let credits = credits(status, french).filter(|c| c.enabled && !c.used.is_empty());
    let credits_percent = credits.as_ref().and_then(|c| c.percent);

    let paused = config.is_paused(now);
    let pause_state = if config.dnd {
        tr(french, "Until you resume", "Jusqu’à la reprise")
    } else if paused {
        let time = clock_or_day(config.pause_until_ms, now, offset_ms, french);
        if french {
            format!("Jusqu’à {time}")
        } else {
            format!("Until {time}")
        }
    } else {
        String::new()
    };

    let recent = now.saturating_sub(update.checked_ms) < CHECK_NOTICE_MS;
    let update_label = match (&update.latest, &update.error) {
        (Some(version), _) if french => format!("Mise à jour disponible · v{version}"),
        (Some(version), _) => format!("Update available · v{version}"),
        _ if update.checking => tr(french, "Checking…", "Vérification…"),
        (None, Some(_)) if recent => tr(french, "Check failed", "Échec de la vérification"),
        (None, None) if recent => tr(french, "Up to date", "À jour"),
        _ => tr(french, "Check for updates", "Rechercher une mise à jour"),
    };

    let user = discord_user(status);
    let discord = if user.is_empty() {
        [
            tr(french, "Discord not connected", "Discord non connecté"),
            String::new(),
            String::new(),
        ]
    } else {
        // Why nothing reaches Discord right now, or empty when the activity is shown.
        let reason = if config.dnd {
            tr(french, "Do Not Disturb", "ne pas déranger")
        } else if paused {
            tr(french, "paused", "en pause")
        } else {
            match text(status, "hiddenReason") {
                "private" => tr(french, "private project", "projet privé"),
                "chat" => tr(french, "Chat tab", "onglet Chat"),
                _ if !client_on => tr(french, "Claude not running", "Claude pas lancé"),
                _ => String::new(),
            }
        };
        let (before, extra) = if reason.is_empty() {
            let since = status
                .get("startedAtMs")
                .and_then(Value::as_u64)
                .map(|started| duration(now.saturating_sub(started) / 60_000, french))
                .unwrap_or_default();
            (tr(french, "Showing on ", "Affiché sur "), since)
        } else {
            (tr(french, "Hidden on ", "Masqué sur "), reason)
        };
        let after = if extra.is_empty() {
            extra
        } else {
            format!(" · {extra}")
        };
        [before, user, after]
    };

    PanelView {
        model_line,
        session_line,
        client_on,
        client_label,
        hero: main.map(|limit| limit_view(limit, now, offset_ms, french)),
        // "Claude Max (5x)" -> "Max 5x".
        plan: plan(status)
            .trim_start_matches("Claude ")
            .replace(['(', ')'], ""),
        alert,
        alert_level,
        spark: spark(status, now, chart_width, 34.0, french)
            .filter(|_| five_hour)
            .map(|spark| (spark.line, spark.area, spark.peak)),
        limits,
        credits: credits
            .map(|c| {
                if c.limit.is_empty() {
                    c.used
                } else {
                    format!("{} / {}", c.used, c.limit)
                }
            })
            .unwrap_or_default(),
        credits_percent: credits_percent.unwrap_or(-1.0),
        credits_level: level((credits_percent.unwrap_or(0.0) * 100.0) as u64),
        paused,
        pause_state,
        mode: config.rpc_mode.clone(),
        update_label,
        update_ready: update.latest.is_some(),
        discord,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // 2026-11-23 12:00:00 UTC.
    const NOON: u64 = 1_795_435_200_000;

    fn status() -> Value {
        json!({
            "claudeLine": "Claude: Desktop (Code)",
            "modelLine": "Claude Opus 5.5 | Extra high | Claude Max",
            "providerLine": "Provider: Subscription (Claude Max)",
            "discordLine": "Discord: Connected (inerthel)",
            "cliAlongside": true,
            "limits": [
                { "label": "5h", "usedPercent": 83, "reset": "2026-11-23T15:12:00Z" },
                { "label": "All", "usedPercent": 44, "reset": "in 2 hours 5 min" },
            ],
            "history5h": [[NOON - 7_200_000, 10], [NOON - 3_600_000, 60], [NOON, 83]],
            "previewHeader": "Watching Clawd",
            "previewPrimary": "Claude Desktop (Code)",
            "previewSecondary": "Claude Opus 5.5",
            "previewTertiary": "Powered by Anthropic",
            "startedAtMs": NOON - 3_725_000,
            "sessionTitle": "Slint migration",
            "gitBranch": "main",
        })
    }

    #[test]
    fn reads_the_status_lines() {
        let status = status();
        assert_eq!(
            client(&status, false),
            (true, "Desktop · Code + CLI".into())
        );
        assert_eq!(
            client(&json!({ "claudeLine": "Claude: CLI (Code) [2]" }), false),
            (true, "Claude Code".into())
        );
        assert_eq!(client(&Value::Null, true), (false, "Inactif".into()));
        assert_eq!(model(&status), ("Opus 5.5".into(), "Extra high".into()));
        assert_eq!(plan(&status), "Claude Max");
        assert_eq!(plan(&json!({ "providerLine": "Provider: Unknown" })), "");
        assert_eq!(
            plan(&json!({ "providerLine": "Provider: Bedrock" })),
            "Bedrock"
        );
        assert!(has_five_hour_limit(&status));
        assert_eq!(discord_user(&status), "inerthel");
        assert_eq!(discord_user(&Value::Null), "");
    }

    #[test]
    fn formats_resets_in_both_languages() {
        assert_eq!(parse_iso_ms("2026-11-23T12:00:00Z"), Some(NOON));
        assert_eq!(parse_iso_ms("2026-11-23T13:00:00.250+01:00"), Some(NOON));
        assert_eq!(parse_iso_ms("soon"), None);
        assert_eq!(
            reset_parts("2026-11-23T15:12:00Z", NOON, 3_600_000, false),
            Some(("resets in 3h 12m".into(), "16:12".into()))
        );
        // Another local day gets its weekday.
        assert_eq!(
            reset_parts("2026-11-29T03:00:00Z", NOON, 0, true),
            Some(("remise à zéro dans 5 j 15 h".into(), "dim. 03:00".into()))
        );
        // Free text has no clock time; unknown text is kept as is.
        assert_eq!(
            reset_parts("in 4 hr 31 min", NOON, 0, false),
            Some(("resets in 4h 31m".into(), String::new()))
        );
        assert_eq!(
            reset_parts("Sun 3:00 AM", NOON, 0, false),
            Some(("resets Sun 3:00 AM".into(), String::new()))
        );
        assert_eq!(reset_parts("", NOON, 0, false), None);
        assert_eq!(duration(45, true), "45 min");
        assert_eq!(duration(125, true), "2 h 05");
        assert_eq!(next_midnight(NOON, 3_600_000), NOON + 11 * 3_600_000);
    }

    #[test]
    fn settings_view_follows_status_and_form() {
        let config = ClaudeConfig::default();
        let view = StatusView::new(&status(), &config, false, 500.0, NOON, 0, false);
        assert_eq!(view.client_label, "Desktop · Code + CLI");
        assert_eq!(view.limits.len(), 3);
        assert_eq!(view.limits[0].figure, "83%");
        assert_eq!(view.limits[0].level, 1);
        assert!(view.limits[0].chart);
        assert_eq!(view.limits[1].reset_in, "Resets in 2h 5m");
        // Fable keeps its row while Claude does not report it.
        assert_eq!(
            (view.limits[2].figure.as_str(), view.limits[2].level),
            ("—", 3)
        );
        let spark = view.spark.unwrap();
        assert!(spark.line.starts_with("M0.0 56.0 L250.0 26.0 L500.0 12.2"));
        assert!(spark.area.ends_with("L500.0 64.0 L0 64.0 Z"));
        assert_eq!(spark.peak, "Peak 83%");
        // Buttons show in Watching mode only, and only complete web links.
        assert!(!view.preview.off);
        assert_eq!(view.preview.buttons, ["Claude", "GitHub Repo"]);
        assert_eq!(elapsed(&status(), NOON, false), "01:02:05 elapsed");

        let paused = ClaudeConfig {
            pause_until_ms: NOON + 60_000,
            ..ClaudeConfig::default()
        };
        let view = StatusView::new(&status(), &paused, false, 500.0, NOON, 0, true);
        assert!(view.preview.off);
        assert_eq!(view.preview.primary, "En pause jusqu’à 12:01");
        let off = StatusView::new(&Value::Null, &config, false, 500.0, NOON, 0, false);
        assert_eq!(off.client_label, "Not running");
        assert_eq!(off.preview.primary, "Claude is not running");
        assert!(off.limits.is_empty() && off.spark.is_none() && off.credits.is_none());
    }

    #[test]
    fn panel_view_leads_with_the_five_hour_session() {
        let update = Update::default();
        let config = ClaudeConfig::default();
        let view = panel_view(&status(), &config, &update, 288.0, NOON, 0, false);
        assert_eq!(view.model_line, "Opus 5.5 · Extra high");
        assert_eq!(view.session_line, "Slint migration · main");
        assert_eq!(view.plan, "Max");
        assert_eq!(view.hero.as_ref().unwrap().name, "5-hour session");
        assert_eq!(view.alert, "Above 80% of your 5-hour session");
        let rows: Vec<_> = view.limits.iter().map(|l| l.reset_at.as_str()).collect();
        assert_eq!(rows, ["Resets in 2h 5m", "Not reported"]);
        assert_eq!(
            view.discord,
            ["Showing on ", "inerthel", " · 1h 2m"].map(String::from)
        );
        assert_eq!(view.update_label, "Check for updates");

        let dnd = ClaudeConfig {
            dnd: true,
            ..ClaudeConfig::default()
        };
        let found = Update {
            latest: Some("4.2.0".into()),
            ..Update::default()
        };
        let view = panel_view(&status(), &dnd, &found, 288.0, NOON, 0, true);
        assert!(view.paused && view.update_ready);
        assert_eq!(view.pause_state, "Jusqu’à la reprise");
        assert_eq!(view.discord[0], "Masqué sur ");
        assert_eq!(view.discord[2], " · ne pas déranger");
        assert_eq!(view.update_label, "Mise à jour disponible · v4.2.0");
    }

    #[test]
    fn formats_credits_and_button_errors() {
        assert_eq!(money(1234.0, "USD", false), "$12.34");
        assert_eq!(money(1234.0, "EUR", true), "12,34 €");
        assert_eq!(money(1234.0, "CAD", false), "12.34 CAD");
        assert_eq!(money(1234.0, "", false), "12.34");
        assert_eq!(money(1234.0, "JPY", false), "1234 JPY");
        let status = json!({ "credits": { "enabled": true, "used": 4000, "limit": 5000, "currency": "USD" } });
        let view = credits_view(&status, false).unwrap();
        assert_eq!(view.value, "$40.00");
        assert_eq!(view.of, "of $50.00 this month");
        assert_eq!((view.percent, view.level), (0.8, 1));
        let off = json!({ "credits": { "enabled": false } });
        assert_eq!(credits_view(&off, false).unwrap().value, "Off");
        assert!(credits_view(&json!({ "credits": null }), false).is_none());

        assert_eq!(button_error("Site", "https://a.test", false), "");
        assert_eq!(button_error("", "", false), "");
        assert!(button_error("Site", "a.test", false).contains("https://"));
        assert!(button_error("Site", "", true).contains("libellé"));
    }
}
