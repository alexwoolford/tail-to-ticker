use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::{get_success_bytes, pad_cik, user_agent_client, Company, Result};

pub const TICKERS_URL: &str = "https://www.sec.gov/files/company_tickers_exchange.json";

/// Columnar SEC file: `{ "fields": ["cik","name","ticker","exchange"], "data": [[...], ...] }`
#[derive(Debug, Deserialize)]
struct TickersFile {
    fields: Vec<String>,
    data: Vec<Vec<Value>>,
}

pub async fn download_tickers(user_agent: &str) -> Result<Vec<Company>> {
    tracing::info!(url = TICKERS_URL, "downloading SEC ticker universe");
    let client = user_agent_client(user_agent)?;
    let body = get_success_bytes(&client, TICKERS_URL).await?;
    parse_tickers_json(&body)
}

pub fn load_tickers(path: &Path) -> Result<Vec<Company>> {
    let body = std::fs::read(path)?;
    parse_tickers_json(&body)
}

pub fn parse_tickers_json(body: &[u8]) -> Result<Vec<Company>> {
    // The official file is columnar. A few mirrors use `{ "0": {cik_str, ticker, title} }`.
    if let Ok(file) = serde_json::from_slice::<TickersFile>(body) {
        return Ok(from_columnar(file));
    }
    let v: Value = serde_json::from_slice(body)?;
    if let Some(obj) = v.as_object() {
        let mut out = Vec::new();
        for (_k, row) in obj {
            if let Some(c) = from_object_row(row) {
                out.push(c);
            }
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }
    Err(crate::Error::Msg(
        "unrecognized company_tickers JSON shape".into(),
    ))
}

fn from_columnar(file: TickersFile) -> Vec<Company> {
    let idx: HashMap<String, usize> = file
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| (f.to_ascii_lowercase(), i))
        .collect();
    let get = |row: &[Value], key: &str| -> String {
        idx.get(key)
            .and_then(|i| row.get(*i))
            .map(value_to_string)
            .unwrap_or_default()
    };
    file.data
        .into_iter()
        .filter_map(|row| {
            let ticker = get(&row, "ticker");
            if ticker.is_empty() {
                return None;
            }
            Some(Company {
                cik: pad_cik(&get(&row, "cik")),
                ticker: ticker.to_uppercase(),
                name: get(&row, "name"),
                exchange: get(&row, "exchange"),
                former_names: Vec::new(),
                street: String::new(),
                city: String::new(),
                state: String::new(),
            })
        })
        .collect()
}

fn from_object_row(row: &Value) -> Option<Company> {
    let ticker = row
        .get("ticker")
        .or_else(|| row.get("tickers"))
        .map(value_to_string)
        .filter(|s| !s.is_empty())?;
    let cik = row
        .get("cik")
        .or_else(|| row.get("cik_str"))
        .map(value_to_string)
        .unwrap_or_default();
    let name = row
        .get("name")
        .or_else(|| row.get("title"))
        .map(value_to_string)
        .unwrap_or_default();
    Some(Company {
        cik: pad_cik(&cik),
        ticker: ticker.to_uppercase(),
        name,
        exchange: row.get("exchange").map(value_to_string).unwrap_or_default(),
        former_names: Vec::new(),
        street: String::new(),
        city: String::new(),
        state: String::new(),
    })
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// One company row per CIK, preferring a common share over preferreds/warrants.
///
/// SEC `company_tickers_exchange.json` lists every class for a CIK. A HashMap
/// last-write would publish `AUB-PA` / `JPM-PM` / `FCNCP` instead of `AUB` /
/// `JPM` / `FCNCA`.
pub fn primary_listings(companies: &[Company]) -> Vec<Company> {
    let mut best: HashMap<String, Company> = HashMap::new();
    for c in companies {
        match best.get(&c.cik) {
            Some(cur) if common_share_rank(c) <= common_share_rank(cur) => {}
            _ => {
                best.insert(c.cik.clone(), c.clone());
            }
        }
    }
    best.into_values().collect()
}

fn major_exchange(exchange: &str) -> bool {
    matches!(
        exchange.trim().to_ascii_uppercase().as_str(),
        "NYSE" | "NASDAQ" | "NYSE ARCA" | "NYSE AMERICAN" | "NYSE MKT" | "AMEX"
    )
}

fn structured_preferred_or_warrant(ticker: &str) -> bool {
    let t = ticker.trim().to_ascii_uppercase();
    if let Some((_, rest)) = t.split_once('-') {
        return rest.starts_with('P') || rest.starts_with('W');
    }
    false
}

fn series_suffix_unhyphenated(ticker: &str) -> bool {
    let t = ticker.trim().to_ascii_uppercase();
    if t.contains('-') || t.contains('.') {
        return false;
    }
    t.len() >= 5 && matches!(t.chars().last(), Some('P' | 'O' | 'W'))
}

fn common_share_rank(c: &Company) -> (u8, u8, u8, i32, std::cmp::Reverse<String>) {
    let t = c.ticker.trim().to_ascii_uppercase();
    let hyphen = t.contains('-') || t.contains('.');
    (
        u8::from(!hyphen),
        u8::from(major_exchange(&c.exchange)),
        u8::from(!(structured_preferred_or_warrant(&t) || series_suffix_unhyphenated(&t))),
        -(t.len() as i32),
        std::cmp::Reverse(t),
    )
}

/// Merge HQ / former-name records onto the ticker universe (left join on CIK).
pub fn apply_addresses(companies: &mut [Company], addresses: &[crate::AddressRecord]) {
    let map: HashMap<String, &crate::AddressRecord> =
        addresses.iter().map(|a| (pad_cik(&a.cik), a)).collect();
    for c in companies {
        if let Some(a) = map.get(&c.cik) {
            if c.street.is_empty() {
                c.street = a.street.clone();
                c.city = a.city.clone();
                c.state = a.state.clone();
            }
            if c.former_names.is_empty() {
                c.former_names = a.former_names.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_columnar_tickers() {
        let json = br#"{
            "fields": ["cik","name","ticker","exchange"],
            "data": [
                [104169, "Walmart Inc.", "WMT", "NYSE"],
                [320193, "Apple Inc.", "AAPL", "Nasdaq"]
            ]
        }"#;
        let rows = parse_tickers_json(json).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].ticker, "WMT");
        assert_eq!(rows[0].cik, "0000104169");
        assert_eq!(rows[0].name, "Walmart Inc.");
    }

    fn co(cik: &str, ticker: &str, exchange: &str) -> Company {
        Company {
            cik: crate::pad_cik(cik),
            ticker: ticker.into(),
            name: "X".into(),
            exchange: exchange.into(),
            former_names: Vec::new(),
            street: String::new(),
            city: String::new(),
            state: String::new(),
        }
    }

    #[test]
    fn primary_listings_prefers_common_over_preferred() {
        let rows = primary_listings(&[
            co("883948", "AUB-PA", "NYSE"),
            co("883948", "AUB", "NYSE"),
            co("19617", "JPM-PM", "NYSE"),
            co("19617", "AMJB", "NYSE"),
            co("19617", "JPM", "NYSE"),
            co("798941", "FCNCP", "Nasdaq"),
            co("798941", "FCNCB", "OTC"),
            co("798941", "FCNCO", "Nasdaq"),
            co("798941", "FCNCA", "Nasdaq"),
            co("70858", "MER-PK", "NYSE"),
            co("70858", "BAC", "NYSE"),
        ]);
        let mut by: Vec<_> = rows.into_iter().map(|c| (c.cik, c.ticker)).collect();
        by.sort();
        assert_eq!(
            by,
            vec![
                ("0000019617".into(), "JPM".into()),
                ("0000070858".into(), "BAC".into()),
                ("0000798941".into(), "FCNCA".into()),
                ("0000883948".into(), "AUB".into()),
            ]
        );
    }
}
