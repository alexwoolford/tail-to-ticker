use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct OverrideFile {
    #[serde(default)]
    pub mappings: Vec<OverrideEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OverrideEntry {
    pub n_number: String,
    pub ticker: String,
    #[serde(default)]
    pub cik: String,
    #[serde(default)]
    pub company_name: String,
    #[serde(default)]
    pub citation: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GoldFile {
    #[serde(default)]
    pub companies: Vec<GoldCompany>,
    #[serde(default)]
    pub tails: Vec<OverrideEntry>,
    /// N-numbers that must not be published as `must_not_ticker` (namesake FPs).
    #[serde(default)]
    pub unpublished_tails: Vec<UnpublishedTail>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UnpublishedTail {
    pub n_number: String,
    pub must_not_ticker: String,
    #[serde(default)]
    pub citation: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GoldCompany {
    pub ticker: String,
    #[serde(default)]
    pub cik: String,
    #[serde(default)]
    pub company_name: String,
    /// FAA registrant names (as-filed) that should resolve to this ticker.
    #[serde(default)]
    pub registrant_names: Vec<String>,
}

pub fn load_overrides(path: &Path) -> anyhow::Result<HashMap<String, OverrideEntry>> {
    let text = std::fs::read_to_string(path)?;
    let file: OverrideFile = serde_yaml::from_str(&text)?;
    let mut map = HashMap::new();
    for mut e in file.mappings {
        e.n_number = faa_ingest::canonical_n_number(&e.n_number);
        e.ticker = e.ticker.to_uppercase();
        map.insert(e.n_number.clone(), e);
    }
    Ok(map)
}

pub fn load_gold(path: &Path) -> anyhow::Result<GoldFile> {
    let text = std::fs::read_to_string(path)?;
    let mut gold: GoldFile = serde_yaml::from_str(&text)?;
    for t in &mut gold.tails {
        t.n_number = faa_ingest::canonical_n_number(&t.n_number);
        t.ticker = t.ticker.to_uppercase();
    }
    for t in &mut gold.unpublished_tails {
        t.n_number = faa_ingest::canonical_n_number(&t.n_number);
        t.must_not_ticker = t.must_not_ticker.to_uppercase();
    }
    Ok(gold)
}

pub fn unpublished_by_n(gold: &GoldFile) -> HashMap<String, String> {
    gold.unpublished_tails
        .iter()
        .map(|t| (t.n_number.clone(), t.must_not_ticker.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gold_file_has_at_least_fifty_companies() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overrides/gold.yaml");
        let gold = load_gold(&path).unwrap();
        assert!(gold.companies.len() >= 50, "got {}", gold.companies.len());
        assert_eq!(
            gold.unpublished_tails.len(),
            19,
            "unpublished_tails should be the 18 audit namesake FPs plus N881RC/COO"
        );
    }

    #[test]
    fn overrides_have_citations() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overrides/mappings.yaml");
        let ov = load_overrides(&path).unwrap();
        assert!(!ov.is_empty(), "cited gold-miss / at-risk TPs");
        for e in ov.values() {
            assert!(!e.n_number.is_empty());
            assert!(!e.ticker.is_empty());
            assert!(
                !e.citation.trim().is_empty(),
                "{} missing citation",
                e.n_number
            );
        }
    }
}
