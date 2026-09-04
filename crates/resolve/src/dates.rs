//! UTC calendar days (`YYYY-MM-DD`) on mapping rows; one write instant on `refresh_run`.

use chrono::{DateTime, Utc};

pub fn is_utc_date(s: &str) -> bool {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(|d| d.format("%Y-%m-%d").to_string() == s)
        .unwrap_or(false)
}

pub fn require_utc_date(s: &str, what: &str) -> anyhow::Result<()> {
    if is_utc_date(s) {
        Ok(())
    } else {
        anyhow::bail!("{what} must be UTC calendar day YYYY-MM-DD, got {s:?}")
    }
}

/// Instant TEXT matching journal `utc_iso`: `YYYY-MM-DDTHH:MM:SSZ`.
pub fn utc_iso(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub fn is_utc_instant(s: &str) -> bool {
    let Some(body) = s.strip_suffix('Z') else {
        return false;
    };
    chrono::NaiveDateTime::parse_from_str(body, "%Y-%m-%dT%H:%M:%S")
        .map(|dt| format!("{}Z", dt.format("%Y-%m-%dT%H:%M:%S")) == s)
        .unwrap_or(false)
}

pub fn require_utc_instant(s: &str, what: &str) -> anyhow::Result<()> {
    if is_utc_instant(s) {
        Ok(())
    } else {
        anyhow::bail!("{what} must be UTC instant YYYY-MM-DDTHH:MM:SSZ, got {s:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn accepts_calendar_day() {
        assert!(is_utc_date("2026-09-01"));
    }

    #[test]
    fn rejects_instant_slash_and_empty() {
        assert!(!is_utc_date("2026-09-01T00:00:00Z"));
        assert!(!is_utc_date("2026/09/01"));
        assert!(!is_utc_date(""));
    }

    #[test]
    fn utc_iso_is_zulu_second_resolution() {
        let dt = Utc.with_ymd_and_hms(2026, 9, 1, 21, 19, 58).unwrap();
        assert_eq!(utc_iso(dt), "2026-09-01T21:19:58Z");
        assert!(is_utc_instant("2026-09-01T21:19:58Z"));
        assert!(!is_utc_instant("2026-09-01"));
        assert!(!is_utc_instant("2026-09-01T21:19:58+00:00"));
        assert!(!is_utc_instant("2026-09-01T21:19:58.000Z"));
        assert!(!is_utc_instant(""));
    }
}
