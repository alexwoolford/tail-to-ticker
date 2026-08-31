//! Download and parse the FAA Releasable Aircraft Database.
//!
//! The nightly zip is public-domain US government data:
//! <https://registry.faa.gov/database/ReleasableAircraft.zip>

mod download;
mod filter;
mod parse;

pub use download::{download_registry, download_registry_to, FAA_ZIP_URL};
pub use filter::{corporate_reason, is_corporate_aviation};
pub use parse::{parse_acftref, parse_master, parse_registry_zip};

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
    #[error("{0}")]
    Msg(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// One joined MASTER + ACFTREF row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aircraft {
    /// Canonical N-number with leading N, uppercase (e.g. `N123AB`).
    pub n_number: String,
    pub serial: String,
    pub mfr_mdl_code: String,
    pub year_mfr: String,
    pub type_registrant: String,
    pub registrant_name: String,
    pub street: String,
    pub street2: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub country: String,
    pub last_action_date: String,
    pub cert_issue_date: String,
    pub type_aircraft: String,
    pub type_engine: String,
    pub status_code: String,
    pub fractional_owner: bool,
    pub icao24: String,
    pub make: String,
    pub model: String,
    pub no_seats: String,
}

impl Aircraft {
    pub fn is_valid_registration(&self) -> bool {
        matches!(self.status_code.trim(), "V" | "v")
    }

    pub fn is_individual(&self) -> bool {
        self.type_registrant.trim() == "1"
    }
}

pub fn canonical_n_number(raw: &str) -> String {
    let s = raw.trim().to_uppercase().replace(['-', ' ', '.'], "");
    if s.is_empty() {
        return s;
    }
    if s.starts_with('N') {
        s
    } else {
        format!("N{s}")
    }
}

pub fn canonical_icao24(raw: &str) -> String {
    raw.trim().to_lowercase()
}
