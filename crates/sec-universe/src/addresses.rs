use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;

use crate::{pad_cik, AddressRecord, Result};

/// Load a JSON array of `{cik, street, city, state, former_names}` produced by
/// the harvest sidecar or a bulk submissions extract.
pub fn load_addresses_json(path: &Path) -> Result<Vec<AddressRecord>> {
    let mut buf = String::new();
    File::open(path)?.read_to_string(&mut buf)?;
    let rows: Vec<AddressJson> = serde_json::from_str(&buf)?;
    Ok(rows
        .into_iter()
        .map(|r| AddressRecord {
            cik: pad_cik(&r.cik),
            street: r.street.unwrap_or_default(),
            city: r.city.unwrap_or_default(),
            state: r.state.unwrap_or_default(),
            former_names: r.former_names.unwrap_or_default(),
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct AddressJson {
    cik: String,
    #[serde(default)]
    street: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    former_names: Option<Vec<String>>,
}
