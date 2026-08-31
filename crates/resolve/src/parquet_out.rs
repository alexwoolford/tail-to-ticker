use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use arrow::array::{BooleanArray, StringArray, UInt32Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::arrow_writer::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;

use crate::Mapping;

pub fn write_mappings_parquet(path: &Path, rows: &[Mapping]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let schema = Arc::new(Schema::new(vec![
        Field::new("n_number", DataType::Utf8, false),
        Field::new("icao24", DataType::Utf8, false),
        Field::new("serial", DataType::Utf8, false),
        Field::new("make", DataType::Utf8, false),
        Field::new("model", DataType::Utf8, false),
        Field::new("ticker", DataType::Utf8, false),
        Field::new("cik", DataType::Utf8, false),
        Field::new("company_name", DataType::Utf8, false),
        Field::new("registrant_name", DataType::Utf8, false),
        Field::new("match_method", DataType::Utf8, false),
        Field::new("as_of_date", DataType::Utf8, false),
        Field::new("source_url", DataType::Utf8, false),
        Field::new("fleet_size", DataType::UInt32, false),
        Field::new("aviation_issuer", DataType::Boolean, false),
    ]));

    let n_number: Vec<&str> = rows.iter().map(|r| r.n_number.as_str()).collect();
    let icao24: Vec<&str> = rows.iter().map(|r| r.icao24.as_str()).collect();
    let serial: Vec<&str> = rows.iter().map(|r| r.serial.as_str()).collect();
    let make: Vec<&str> = rows.iter().map(|r| r.make.as_str()).collect();
    let model: Vec<&str> = rows.iter().map(|r| r.model.as_str()).collect();
    let ticker: Vec<&str> = rows.iter().map(|r| r.ticker.as_str()).collect();
    let cik: Vec<&str> = rows.iter().map(|r| r.cik.as_str()).collect();
    let company_name: Vec<&str> = rows.iter().map(|r| r.company_name.as_str()).collect();
    let registrant_name: Vec<&str> = rows.iter().map(|r| r.registrant_name.as_str()).collect();
    let match_method: Vec<&str> = rows.iter().map(|r| r.match_method.as_str()).collect();
    let as_of: Vec<&str> = rows.iter().map(|r| r.as_of_date.as_str()).collect();
    let source: Vec<&str> = rows.iter().map(|r| r.source_url.as_str()).collect();
    let fleet_size: Vec<u32> = rows.iter().map(|r| r.fleet_size).collect();
    let aviation_issuer: Vec<bool> = rows.iter().map(|r| r.aviation_issuer).collect();

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(StringArray::from(n_number)),
            Arc::new(StringArray::from(icao24)),
            Arc::new(StringArray::from(serial)),
            Arc::new(StringArray::from(make)),
            Arc::new(StringArray::from(model)),
            Arc::new(StringArray::from(ticker)),
            Arc::new(StringArray::from(cik)),
            Arc::new(StringArray::from(company_name)),
            Arc::new(StringArray::from(registrant_name)),
            Arc::new(StringArray::from(match_method)),
            Arc::new(StringArray::from(as_of)),
            Arc::new(StringArray::from(source)),
            Arc::new(UInt32Array::from(fleet_size)),
            Arc::new(BooleanArray::from(aviation_issuer)),
        ],
    )?;

    let file = File::create(path)?;
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(file, schema, Some(props))?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}

pub fn write_mappings_csv(path: &Path, rows: &[Mapping]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut w = csv::Writer::from_path(path)?;
    for r in rows {
        w.serialize(r)?;
    }
    w.flush()?;
    Ok(())
}
