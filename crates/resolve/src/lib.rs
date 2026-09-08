//! Entity resolution: FAA registrant → listed ticker, with provenance.

mod aviation;
mod classify;
mod dates;
mod eval;
mod matchers;
mod normalize;
mod overrides;
mod store;

pub use aviation::{
    annotate_aviation_issuer, load_aviation_issuers, published_row_floor, AviationIssuers,
};

pub use classify::{classify_registrant, Class};
pub use dates::{is_utc_date, is_utc_instant, require_utc_date, require_utc_instant, utc_iso};
pub use eval::{evaluate_gold, EvalReport};
pub use matchers::{resolve_all, ResolveOutput};
pub use normalize::{name_match_corroborated, normalize_address, normalize_name};
pub use overrides::{
    apply_issuer_aliases, load_gold, load_issuer_aliases, load_overrides, unpublished_by_n,
    GoldCompany, GoldFile, IssuerAlias, OverrideEntry, UnpublishedTail,
};
pub use store::{apply_scd2, lookup, open_db, FeedDb};

use serde::{Deserialize, Serialize};

pub const MANUAL_OVERRIDE: &str = "manual_override";
pub const EDGAR_NNUMBER: &str = "edgar_nnumber";
pub const EXACT_LEGAL_NAME: &str = "exact_legal_name";
pub const EX21_SUBSIDIARY: &str = "ex21_subsidiary";
pub const ADDRESS_CLUSTER: &str = "address_cluster";

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Mapping {
    pub n_number: String,
    pub icao24: String,
    pub serial: String,
    pub make: String,
    pub model: String,
    pub ticker: String,
    pub cik: String,
    pub company_name: String,
    pub registrant_name: String,
    pub match_method: String,
    pub as_of_date: String,
    pub source_url: String,
    /// Count of published rows sharing this ticker (0 until fleet annotation).
    #[serde(default)]
    pub fleet_size: u32,
    /// Registrant is the listed aviation business (OEM / operator / lessor), not a flight department.
    #[serde(default)]
    pub aviation_issuer: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unresolved {
    pub n_number: String,
    pub icao24: String,
    pub make: String,
    pub model: String,
    pub registrant_name: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conflict {
    pub n_number: String,
    pub registrant_name: String,
    pub tickers: Vec<String>,
    pub method: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangelogEntry {
    pub as_of_date: String,
    pub n_number: String,
    pub change: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgarHit {
    pub cik: String,
    pub ticker: Option<String>,
    pub accession: String,
    pub form: String,
    pub n_number: String,
    pub snippet: String,
    pub filing_url: String,
    pub keyword_hit: bool,
}

pub fn load_edgar_jsonl(path: &std::path::Path) -> anyhow::Result<Vec<EdgarHit>> {
    let text = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        out.push(serde_json::from_str(line)?);
    }
    Ok(out)
}
