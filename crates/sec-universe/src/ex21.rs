use std::fs::File;
use std::path::Path;

use arrow::array::{Array, DictionaryArray, Int32Array, Int64Array, LargeStringArray, StringArray};
use arrow::datatypes::{Int32Type, Int64Type};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

use crate::{pad_cik, user_agent_client, Result, Subsidiary};

/// Nightly PUDL parquet of parent/subsidiary rows (CC-BY-4.0).
pub const EX21_URLS: &[&str] = &[
    "https://s3.us-west-2.amazonaws.com/pudl.catalyst.coop/nightly/out_sec10k__parents_and_subsidiaries.parquet",
    "https://s3.us-west-2.amazonaws.com/pudl.catalyst.coop/stable/out_sec10k__parents_and_subsidiaries.parquet",
    "https://s3.us-west-2.amazonaws.com/pudl.catalyst.coop/nightly/parquet/out_sec10k__parents_and_subsidiaries.parquet",
];

pub async fn download_ex21_parquet(user_agent: &str, dest: &Path) -> Result<()> {
    let client = user_agent_client(user_agent)?;
    let mut last_err = None;
    for url in EX21_URLS {
        tracing::info!(url, "trying PUDL Exhibit 21 parquet");
        match client.get(*url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let bytes = resp.bytes().await?;
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(dest, bytes)?;
                return Ok(());
            }
            Ok(resp) => {
                last_err = Some(format!("{url} -> {}", resp.status()));
            }
            Err(e) => last_err = Some(e.to_string()),
        }
    }
    Err(crate::Error::Msg(format!(
        "could not download PUDL Exhibit 21 parquet: {}",
        last_err.unwrap_or_else(|| "unknown".into())
    )))
}

pub fn load_ex21(path: &Path) -> Result<Vec<Subsidiary>> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "csv" => load_ex21_csv(path),
        "json" | "jsonl" => load_ex21_jsonl(path),
        "parquet" | "pq" => load_ex21_parquet(path),
        _ => {
            // Try parquet then csv.
            if let Ok(v) = load_ex21_parquet(path) {
                return Ok(v);
            }
            load_ex21_csv(path)
        }
    }
}

fn load_ex21_csv(path: &Path) -> Result<Vec<Subsidiary>> {
    let mut rdr = csv::Reader::from_path(path)?;
    let mut out = Vec::new();
    for rec in rdr.deserialize() {
        let row: Ex21Csv = rec?;
        let name = row
            .subsidiary_name
            .or(row.subsidiary_company_name)
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        out.push(Subsidiary {
            parent_cik: pad_cik(
                &row.parent_cik
                    .or(row.parent_company_central_index_key)
                    .unwrap_or_default(),
            ),
            parent_name: row
                .parent_name
                .or(row.parent_company_name)
                .unwrap_or_default(),
            subsidiary_name: name,
            parent_street: row
                .parent_street
                .or(row.parent_company_business_street_address)
                .unwrap_or_default(),
            parent_city: row
                .parent_city
                .or(row.parent_company_business_city)
                .unwrap_or_default(),
            parent_state: row
                .parent_state
                .or(row.parent_company_business_state)
                .unwrap_or_default(),
        });
    }
    Ok(out)
}

#[derive(Debug, serde::Deserialize)]
struct Ex21Csv {
    #[serde(default)]
    parent_cik: Option<String>,
    #[serde(default)]
    parent_company_central_index_key: Option<String>,
    #[serde(default)]
    parent_name: Option<String>,
    #[serde(default)]
    parent_company_name: Option<String>,
    #[serde(default)]
    subsidiary_name: Option<String>,
    #[serde(default)]
    subsidiary_company_name: Option<String>,
    #[serde(default)]
    parent_street: Option<String>,
    #[serde(default)]
    parent_company_business_street_address: Option<String>,
    #[serde(default)]
    parent_city: Option<String>,
    #[serde(default)]
    parent_company_business_city: Option<String>,
    #[serde(default)]
    parent_state: Option<String>,
    #[serde(default)]
    parent_company_business_state: Option<String>,
}

fn load_ex21_jsonl(path: &Path) -> Result<Vec<Subsidiary>> {
    let text = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            let rows: Vec<Ex21Csv> = serde_json::from_str(&text)?;
            return Ok(rows
                .into_iter()
                .filter_map(|row| {
                    let name = row.subsidiary_name.or(row.subsidiary_company_name)?;
                    Some(Subsidiary {
                        parent_cik: pad_cik(
                            &row.parent_cik
                                .or(row.parent_company_central_index_key)
                                .unwrap_or_default(),
                        ),
                        parent_name: row
                            .parent_name
                            .or(row.parent_company_name)
                            .unwrap_or_default(),
                        subsidiary_name: name,
                        parent_street: row
                            .parent_street
                            .or(row.parent_company_business_street_address)
                            .unwrap_or_default(),
                        parent_city: row
                            .parent_city
                            .or(row.parent_company_business_city)
                            .unwrap_or_default(),
                        parent_state: row
                            .parent_state
                            .or(row.parent_company_business_state)
                            .unwrap_or_default(),
                    })
                })
                .collect());
        }
        let row: Ex21Csv = serde_json::from_str(line)?;
        let name = row
            .subsidiary_name
            .or(row.subsidiary_company_name)
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        out.push(Subsidiary {
            parent_cik: pad_cik(
                &row.parent_cik
                    .or(row.parent_company_central_index_key)
                    .unwrap_or_default(),
            ),
            parent_name: row
                .parent_name
                .or(row.parent_company_name)
                .unwrap_or_default(),
            subsidiary_name: name,
            parent_street: row
                .parent_street
                .or(row.parent_company_business_street_address)
                .unwrap_or_default(),
            parent_city: row
                .parent_city
                .or(row.parent_company_business_city)
                .unwrap_or_default(),
            parent_state: row
                .parent_state
                .or(row.parent_company_business_state)
                .unwrap_or_default(),
        });
    }
    Ok(out)
}

fn load_ex21_parquet(path: &Path) -> Result<Vec<Subsidiary>> {
    let file = File::open(path)?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|e| crate::Error::Parquet(e.to_string()))?;
    let reader = builder
        .build()
        .map_err(|e| crate::Error::Parquet(e.to_string()))?;
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch.map_err(|e| crate::Error::Parquet(e.to_string()))?;
        let n = batch.num_rows();
        let parent_cik = col_strings(
            &batch,
            &[
                "parent_company_central_index_key",
                "parent_cik",
                "central_index_key",
            ],
        );
        let parent_name = col_strings(&batch, &["parent_company_name", "parent_name"]);
        let sub_name = col_strings(&batch, &["subsidiary_company_name", "subsidiary_name"]);
        let street = col_strings(
            &batch,
            &["parent_company_business_street_address", "parent_street"],
        );
        let city = col_strings(&batch, &["parent_company_business_city", "parent_city"]);
        let state = col_strings(&batch, &["parent_company_business_state", "parent_state"]);
        for i in 0..n {
            let name = sub_name.get(i).cloned().unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            out.push(Subsidiary {
                parent_cik: pad_cik(&parent_cik.get(i).cloned().unwrap_or_default()),
                parent_name: parent_name.get(i).cloned().unwrap_or_default(),
                subsidiary_name: name,
                parent_street: street.get(i).cloned().unwrap_or_default(),
                parent_city: city.get(i).cloned().unwrap_or_default(),
                parent_state: state.get(i).cloned().unwrap_or_default(),
            });
        }
    }
    Ok(out)
}

fn col_strings(batch: &arrow::record_batch::RecordBatch, names: &[&str]) -> Vec<String> {
    for name in names {
        if let Some(col) = batch.column_by_name(name) {
            return array_to_strings(col.as_ref());
        }
        // PUDL sometimes uses fully-qualified names.
        for (i, field) in batch.schema().fields().iter().enumerate() {
            if field.name().ends_with(name) {
                return array_to_strings(batch.column(i).as_ref());
            }
        }
    }
    vec![String::new(); batch.num_rows()]
}

fn array_to_strings(arr: &dyn Array) -> Vec<String> {
    if let Some(a) = arr.as_any().downcast_ref::<StringArray>() {
        return (0..a.len())
            .map(|i| {
                if a.is_null(i) {
                    String::new()
                } else {
                    a.value(i).to_string()
                }
            })
            .collect();
    }
    if let Some(a) = arr.as_any().downcast_ref::<LargeStringArray>() {
        return (0..a.len())
            .map(|i| {
                if a.is_null(i) {
                    String::new()
                } else {
                    a.value(i).to_string()
                }
            })
            .collect();
    }
    if let Some(a) = arr.as_any().downcast_ref::<Int64Array>() {
        return (0..a.len())
            .map(|i| {
                if a.is_null(i) {
                    String::new()
                } else {
                    a.value(i).to_string()
                }
            })
            .collect();
    }
    if let Some(a) = arr.as_any().downcast_ref::<Int32Array>() {
        return (0..a.len())
            .map(|i| {
                if a.is_null(i) {
                    String::new()
                } else {
                    a.value(i).to_string()
                }
            })
            .collect();
    }
    if let Some(a) = arr.as_any().downcast_ref::<DictionaryArray<Int32Type>>() {
        return dict_to_strings(a.values().as_ref(), a.keys());
    }
    if let Some(a) = arr.as_any().downcast_ref::<DictionaryArray<Int64Type>>() {
        let keys: Vec<Option<i64>> = (0..a.len())
            .map(|i| {
                if a.is_null(i) {
                    None
                } else {
                    Some(a.keys().value(i))
                }
            })
            .collect();
        let values = array_to_strings(a.values().as_ref());
        return keys
            .into_iter()
            .map(|k| {
                k.and_then(|i| values.get(i as usize).cloned())
                    .unwrap_or_default()
            })
            .collect();
    }
    vec![String::new(); arr.len()]
}

fn dict_to_strings(values: &dyn Array, keys: &Int32Array) -> Vec<String> {
    let vals = array_to_strings(values);
    (0..keys.len())
        .map(|i| {
            if keys.is_null(i) {
                String::new()
            } else {
                vals.get(keys.value(i) as usize)
                    .cloned()
                    .unwrap_or_default()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_ex21_csv() {
        let dir = std::env::temp_dir();
        let path = dir.join("ex21_test.csv");
        let mut f = File::create(&path).unwrap();
        writeln!(
            f,
            "parent_cik,parent_name,subsidiary_name,parent_street,parent_city,parent_state"
        )
        .unwrap();
        writeln!(
            f,
            "104169,Walmart Inc.,WALMART AVIATION LLC,702 SW 8TH ST,BENTONVILLE,AR"
        )
        .unwrap();
        let rows = load_ex21_csv(&path).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].parent_cik, "0000104169");
        assert_eq!(rows[0].subsidiary_name, "WALMART AVIATION LLC");
    }
}
