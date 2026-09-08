use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::normalize::normalize_name;
use sec_universe::{pad_cik, Company};

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

#[derive(Debug, Clone, Deserialize)]
pub struct IssuerAliasFile {
    #[serde(default)]
    pub aliases: Vec<IssuerAlias>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IssuerAlias {
    pub cik: String,
    pub name: String,
    #[serde(default)]
    pub ticker: String,
    #[serde(default)]
    pub citation: String,
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

pub fn load_issuer_aliases(path: &Path) -> anyhow::Result<Vec<IssuerAlias>> {
    let text = std::fs::read_to_string(path)?;
    let file: IssuerAliasFile = serde_yaml::from_str(&text)?;
    let mut out = Vec::new();
    for mut a in file.aliases {
        a.cik = pad_cik(&a.cik);
        a.name = a.name.trim().to_string();
        a.ticker = a.ticker.trim().to_uppercase();
        if a.name.is_empty() {
            anyhow::bail!("issuer alias for CIK {} has empty name", a.cik);
        }
        if a.citation.trim().is_empty() {
            anyhow::bail!("issuer alias {} / {} missing citation", a.cik, a.name);
        }
        out.push(a);
    }
    Ok(out)
}

/// Index aliases as former names on the matching listed CIK, or ticker if the
/// ticker file has a different CIK for that symbol (XOM cache vs gold).
pub fn apply_issuer_aliases(companies: &mut [Company], aliases: &[IssuerAlias]) -> usize {
    let mut applied = 0usize;
    for a in aliases {
        let key = normalize_name(&a.name);
        if key.is_empty() {
            continue;
        }
        let ticker = a.ticker.trim().to_uppercase();
        for c in companies.iter_mut() {
            let cik_hit = c.cik == a.cik;
            let ticker_hit = !ticker.is_empty() && c.ticker.trim().to_uppercase() == ticker;
            if !cik_hit && !ticker_hit {
                continue;
            }
            let already = c
                .former_names
                .iter()
                .any(|n| normalize_name(n) == key)
                || normalize_name(&c.name) == key;
            if already {
                continue;
            }
            c.former_names.push(a.name.clone());
            applied += 1;
        }
    }
    applied
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

    #[test]
    fn issuer_aliases_have_citations() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overrides/issuer_aliases.yaml");
        let aliases = load_issuer_aliases(&path).unwrap();
        assert!(!aliases.is_empty());
        for a in &aliases {
            assert!(!a.cik.is_empty());
            assert!(!a.name.is_empty());
            assert!(!a.citation.trim().is_empty());
        }
    }

    #[test]
    fn apply_alias_by_ticker_when_cik_missing() {
        let mut companies = vec![Company {
            cik: "0002115436".into(),
            ticker: "XOM".into(),
            name: "ExxonMobil Holdings Corp".into(),
            exchange: "NYSE".into(),
            former_names: Vec::new(),
            street: String::new(),
            city: String::new(),
            state: String::new(),
        }];
        let aliases = load_issuer_aliases(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overrides/issuer_aliases.yaml"),
        )
        .unwrap();
        let n = apply_issuer_aliases(&mut companies, &aliases);
        assert!(n >= 1);
        assert!(companies[0]
            .former_names
            .iter()
            .any(|n| n.eq_ignore_ascii_case("EXXON MOBIL CORP")));
    }
}
