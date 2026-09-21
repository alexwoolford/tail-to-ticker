//! Read FCC ULS aircraft-radio licensee names from the fcc-uls-aircraft work sqlite.
//!
//! This crate does not GET `l_aircr.zip`. Missing file → empty vec (refresh skips
//! the trustee pierce). Retracted rows (`deleted_at IS NOT NULL`) are ignored.
//! Open is read-only (`mode=ro&immutable=1`) so `tails` does not write WAL/shm.

use std::path::Path;

use faa_ingest::canonical_n_number;
use rusqlite::Connection;

/// Host work sqlite from fcc-uls-aircraft (no published `current/` copy).
pub const DEFAULT_FCC_DB: &str = "/var/lib/fcc-uls-aircraft/fcc-uls-aircraft.sqlite";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FccLicensee {
    pub n_number: String,
    pub licensee_name: String,
    pub uls_id: String,
}

pub fn load_fcc_licensees(path: &Path) -> anyhow::Result<Vec<FccLicensee>> {
    // Read-only: this job must not create WAL/shm under fcc-uls-aircraft (tails
    // cannot write that directory). immutable=1 skips the shm file.
    let uri = format!("file:{}?mode=ro&immutable=1", path.display());
    let conn = Connection::open_with_flags(
        &uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| anyhow::anyhow!("open FCC sqlite {}: {e}", path.display()))?;
    let mut stmt = conn
        .prepare(
            "SELECT uls_id, n_number, licensee_name
             FROM licenses
             WHERE deleted_at IS NULL
               AND n_number IS NOT NULL AND n_number != ''
               AND licensee_name IS NOT NULL AND licensee_name != ''",
        )
        .map_err(|e| anyhow::anyhow!("prepare FCC licenses: {e}"))?;
    let mapped = stmt
        .query_map([], |row| {
            Ok(FccLicensee {
                uls_id: row.get::<_, String>(0)?,
                n_number: canonical_n_number(&row.get::<_, String>(1)?),
                licensee_name: row.get(2)?,
            })
        })
        .map_err(|e| anyhow::anyhow!("query FCC licenses: {e}"))?;
    let mut out = Vec::new();
    for row in mapped {
        let row = row.map_err(|e| anyhow::anyhow!("FCC license row: {e}"))?;
        if row.n_number.is_empty() {
            continue;
        }
        out.push(row);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn skips_retracted_and_blank_n() {
        let dir = std::env::temp_dir().join(format!(
            "fcc-lic-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fcc.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE licenses (
                uls_id TEXT PRIMARY KEY,
                n_number TEXT,
                licensee_name TEXT,
                deleted_at INTEGER
             );
             INSERT INTO licenses VALUES ('1','425MP','Marathon Petroleum Company LP',NULL);
             INSERT INTO licenses VALUES ('2','N426MP','Marathon Petroleum Company LP',123);
             INSERT INTO licenses VALUES ('3','','Nobody Inc',NULL);
             INSERT INTO licenses VALUES ('4','N427MP',NULL,NULL);",
        )
        .unwrap();
        drop(conn);
        let rows = load_fcc_licensees(&path).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].n_number, "N425MP");
        assert_eq!(rows[0].uls_id, "1");
        let _ = std::fs::remove_dir_all(dir);
    }
}
