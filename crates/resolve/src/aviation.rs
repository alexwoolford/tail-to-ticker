//! Deterministic `aviation_issuer` flag from a reviewable YAML list.

use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;

use crate::Mapping;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AviationIssuers {
    #[serde(default)]
    pub tickers: Vec<String>,
    #[serde(default)]
    pub needles: Vec<String>,
}

impl AviationIssuers {
    fn ticker_set(&self) -> HashSet<String> {
        self.tickers
            .iter()
            .map(|t| t.trim().to_ascii_uppercase())
            .collect()
    }

    pub fn matches(&self, ticker: &str, registrant: &str) -> bool {
        let t = ticker.trim().to_ascii_uppercase();
        if self.ticker_set().contains(&t) {
            return true;
        }
        let reg = registrant.to_ascii_uppercase();
        self.needles.iter().any(|n| {
            let n = n.trim().to_ascii_uppercase();
            !n.is_empty() && reg.contains(&n)
        })
    }
}

pub fn load_aviation_issuers(path: &Path) -> anyhow::Result<AviationIssuers> {
    let text = std::fs::read_to_string(path)?;
    let mut file: AviationIssuers = serde_yaml::from_str(&text)?;
    for t in &mut file.tickers {
        *t = t.trim().to_ascii_uppercase();
    }
    Ok(file)
}

pub fn annotate_aviation_issuer(rows: &mut [Mapping], issuers: &AviationIssuers) {
    for m in rows.iter_mut() {
        m.aviation_issuer = issuers.matches(&m.ticker, &m.registrant_name);
    }
}

/// If `previous` published rows is large enough, the new count must be at least this.
pub fn published_row_floor(previous: usize) -> Option<usize> {
    if previous < 50 {
        None
    } else {
        Some((previous / 2).max(50))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(ticker: &str, registrant: &str) -> Mapping {
        Mapping {
            ticker: ticker.into(),
            registrant_name: registrant.into(),
            ..Mapping::default()
        }
    }

    #[test]
    fn textron_is_issuer_walmart_is_not() {
        let issuers = AviationIssuers {
            tickers: vec!["TXT".into()],
            needles: vec!["BELL TEXTRON".into()],
        };
        let mut rows = vec![
            row("TXT", "TEXTRON AVIATION INC"),
            row("WMT", "WALMART INC"),
            row("WMT", "WALMART AVIATION LLC"),
        ];
        annotate_aviation_issuer(&mut rows, &issuers);
        assert!(rows[0].aviation_issuer);
        assert!(!rows[1].aviation_issuer);
        assert!(!rows[2].aviation_issuer);
    }

    #[test]
    fn needle_matches_without_ticker_list() {
        let issuers = AviationIssuers {
            tickers: vec![],
            needles: vec!["BRIDGER AIR".into()],
        };
        assert!(issuers.matches("BAER", "BRIDGER AIR TANKER 1 LLC"));
        assert!(!issuers.matches("WMT", "WALMART INC"));
    }

    #[test]
    fn published_row_floor_half_with_min_fifty() {
        assert_eq!(published_row_floor(0), None);
        assert_eq!(published_row_floor(49), None);
        assert_eq!(published_row_floor(50), Some(50));
        assert_eq!(published_row_floor(80), Some(50));
        assert_eq!(published_row_floor(200), Some(100));
        assert_eq!(published_row_floor(603), Some(301));
    }

    #[test]
    fn repo_yaml_marks_textron_not_walmart() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overrides/aviation_issuers.yaml");
        let issuers = load_aviation_issuers(&path).unwrap();
        let mut rows = vec![
            row("TXT", "TEXTRON AVIATION INC"),
            row("WMT", "WALMART INC"),
        ];
        annotate_aviation_issuer(&mut rows, &issuers);
        assert!(rows[0].aviation_issuer);
        assert!(!rows[1].aviation_issuer);
    }
}
