//! SEC ticker universe, optional HQ addresses, and Exhibit 21 subsidiaries.
//!
//! Identifier file: <https://www.sec.gov/files/company_tickers_exchange.json>
//! Exhibit 21: PUDL `out_sec10k__parents_and_subsidiaries` (CC-BY-4.0, cite Catalyst Cooperative).

mod addresses;
mod ex21;
mod tickers;

pub use addresses::load_addresses_json;
pub use ex21::{download_ex21_parquet, load_ex21};
pub use tickers::{
    apply_addresses, download_tickers, load_tickers, parse_tickers_json, primary_listings,
    TICKERS_URL,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),
    #[error("parquet error: {0}")]
    Parquet(String),
    #[error("{0}")]
    Msg(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Company {
    pub cik: String,
    pub ticker: String,
    pub name: String,
    pub exchange: String,
    pub former_names: Vec<String>,
    pub street: String,
    pub city: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subsidiary {
    pub parent_cik: String,
    pub parent_name: String,
    pub subsidiary_name: String,
    pub parent_street: String,
    pub parent_city: String,
    pub parent_state: String,
}

pub fn pad_cik(cik: &str) -> String {
    let digits: String = cik.chars().filter(|c| c.is_ascii_digit()).collect();
    format!("{digits:0>10}")
}

/// HTTP client that honors `HTTPS_PROXY` / `NO_PROXY` (reqwest `system-proxy`).
/// Used for SEC (Akamai). Do not point `HTTPS_PROXY` at a proxy that 403s
/// CONNECT to `*.gov` (the existing IPRoyal product does).
pub fn user_agent_client(user_agent: &str) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(user_agent)
        .gzip(true)
        .default_headers(http_get::default_headers())
        .build()?)
}

/// Same headers, never proxied. PUDL Exhibit 21 is S3 — keep it off residential
/// bandwidth even when `HTTPS_PROXY` is set for FAA/SEC.
pub fn user_agent_client_direct(user_agent: &str) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(user_agent)
        .gzip(true)
        .default_headers(http_get::default_headers())
        .no_proxy()
        .build()?)
}

pub(crate) async fn get_success_bytes(client: &reqwest::Client, url: &str) -> Result<bytes::Bytes> {
    http_get::get_success_bytes(client, url)
        .await
        .map_err(Error::Msg)
}

impl Error {
    pub fn is_http_forbidden(&self) -> bool {
        match self {
            Error::Http(e) => e.status().map(|s| s.as_u16()) == Some(403),
            Error::Msg(s) => s.contains("HTTP 403"),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressRecord {
    pub cik: String,
    pub street: String,
    pub city: String,
    pub state: String,
    pub former_names: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_detects_status_in_msg() {
        let err = Error::Msg(
            "HTTP 403 Forbidden for https://www.sec.gov/files/company_tickers_exchange.json (Server: AkamaiGHost)"
                .into(),
        );
        assert!(err.is_http_forbidden());
        let err = Error::Msg("HTTP 503 for url (Server: AkamaiGHost)".into());
        assert!(!err.is_http_forbidden());
    }
}
