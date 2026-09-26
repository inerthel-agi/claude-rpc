// Tray icon tooltip and 5-hour usage notifications, derived from the daemon's
// status.txt by the status watcher in main.rs.

use serde_json::Value;

use crate::config::ClaudeConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Alert {
    Above80,
    Above95,
    Reset,
}

// Remembers which threshold was already announced for the current 5-hour
// window, so each notification fires once per window.
#[derive(Debug, Default)]
pub(crate) struct AlertState {
    announced: u8,
    last_percent: Option<u8>,
}

impl AlertState {
    pub(crate) fn update(&mut self, percent: u8, config: &ClaudeConfig) -> Vec<Alert> {
        let mut alerts = Vec::new();
        // A drop of 15 points or more after real use means the window reset.
        if let Some(last) = self.last_percent {
            if last >= 20 && percent.saturating_add(15) <= last {
                if config.alert_reset {
                    alerts.push(Alert::Reset);
                }
                self.announced = 0;
            }
        }
        if percent >= 95 && self.announced < 95 {
            if config.alert_95 {
                alerts.push(Alert::Above95);
            }
            self.announced = 95;
        } else if percent >= 80 && self.announced < 80 {
            if config.alert_80 {
                alerts.push(Alert::Above80);
            }
            self.announced = 80;
        }
        self.last_percent = Some(percent);
        alerts
    }
}

pub(crate) fn alert_text(alert: Alert, percent: u8, french: bool) -> (String, String) {
    match (alert, french) {
        (Alert::Reset, false) => (
            "5-hour session reset".into(),
            "Your Claude session limit is back to 0%.".into(),
        ),
        (Alert::Reset, true) => (
            "Session de 5 heures remise à zéro".into(),
            "Ta limite de session Claude est revenue à 0 %.".into(),
        ),
        (_, false) => (
            format!("5-hour session at {percent}%"),
            "Claude usage is getting close to the 5-hour limit.".into(),
        ),
        (_, true) => (
            format!("Session de 5 heures à {percent} %"),
            "Ton usage de Claude approche de la limite de 5 heures.".into(),
        ),
    }
}

pub(crate) fn five_hour_percent(status: &Value) -> Option<u8> {
    status
        .get("limits")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("label").and_then(Value::as_str) == Some("5h"))?
        .get("usedPercent")?
        .as_u64()
        .map(|value| value.min(100) as u8)
}

// Windows caps tray tooltips at 127 characters.
pub(crate) fn tray_tooltip(status: &Value, french: bool) -> String {
    let text = |key: &str| status.get(key).and_then(Value::as_str).unwrap_or("");
    let mut lines = vec!["Claude RPC".to_string()];
    if text("claudeLine") == "Claude: Off" || text("claudeLine").is_empty() {
        lines.push(
            if french {
                "Claude n'est pas lancé"
            } else {
                "Claude is not running"
            }
            .into(),
        );
    } else {
        let model = text("modelLine").replace(" | ", " · ");
        if !model.is_empty() && model != "Auto-detect" {
            lines.push(model.trim_start_matches("Claude ").to_string());
        }
    }
    let limits = status
        .get("limits")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let label = match entry.get("label")?.as_str()? {
                        "All" if french => "Semaine",
                        "All" => "Weekly",
                        other => other,
                    };
                    Some(format!("{label} {}%", entry.get("usedPercent")?.as_u64()?))
                })
                .collect::<Vec<_>>()
                .join(" · ")
        })
        .unwrap_or_default();
    if !limits.is_empty() {
        lines.push(limits);
    }
    lines.join("\n").chars().take(127).collect()
}

// "auto" follows the Windows display language.
pub(crate) fn is_french(language: &str) -> bool {
    match language {
        "fr" => true,
        "en" => false,
        _ => windows_locale().is_some_and(|locale| locale.to_ascii_lowercase().starts_with("fr")),
    }
}

#[cfg(windows)]
fn windows_locale() -> Option<String> {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buffer = [0u16; 85];
    let len = unsafe { GetUserDefaultLocaleName(&mut buffer) };
    (len > 1).then(|| String::from_utf16_lossy(&buffer[..len as usize - 1]))
}

#[cfg(not(windows))]
fn windows_locale() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn announces_each_threshold_once_per_window() {
        let config = ClaudeConfig::default();
        let mut state = AlertState::default();
        assert!(state.update(40, &config).is_empty());
        assert_eq!(state.update(81, &config), [Alert::Above80]);
        assert!(state.update(85, &config).is_empty());
        assert_eq!(state.update(96, &config), [Alert::Above95]);
        assert!(state.update(99, &config).is_empty());
        // The window resets: one reset notice, then thresholds re-arm.
        assert_eq!(state.update(2, &config), [Alert::Reset]);
        assert_eq!(state.update(83, &config), [Alert::Above80]);

        let quiet = ClaudeConfig {
            alert_80: false,
            ..ClaudeConfig::default()
        };
        let mut state = AlertState::default();
        assert!(state.update(82, &quiet).is_empty());
    }

    #[test]
    fn builds_tray_tooltip() {
        let status = json!({
            "claudeLine": "Claude: Desktop (Code)",
            "modelLine": "Claude Opus 5.5 | High",
            "limits": [
                {"label": "5h", "usedPercent": 66},
                {"label": "All", "usedPercent": 24}
            ]
        });
        assert_eq!(
            tray_tooltip(&status, false),
            "Claude RPC\nOpus 5.5 · High\n5h 66% · Weekly 24%"
        );
        assert_eq!(five_hour_percent(&status), Some(66));
        let off = json!({"claudeLine": "Claude: Off", "limits": []});
        assert_eq!(
            tray_tooltip(&off, true),
            "Claude RPC\nClaude n'est pas lancé"
        );
    }
}
