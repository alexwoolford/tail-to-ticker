//! Current MASTER rows from faa-registry-mirror published sqlite.
//!
//! Production refresh reads this file. Do not parse ReleasableAircraft.zip here.

use std::path::Path;

use rusqlite::Connection;

use crate::{canonical_icao24, canonical_n_number, Aircraft, Result};

/// Host publish path from faa-registry-mirror `VACUUM INTO` + `mv`.
pub const DEFAULT_PUBLISHED_DB: &str =
    "/var/lib/faa-registry-mirror/current/faa-registry.sqlite";

pub fn load_current_aircraft(path: &Path) -> Result<(Vec<Aircraft>, usize)> {
    let conn = Connection::open(path).map_err(|e| {
        crate::Error::Msg(format!("open FAA registry sqlite {}: {e}", path.display()))
    })?;
    let acftref_n: i64 = conn
        .query_row("SELECT count(*) FROM aircraft_ref", [], |row| row.get(0))
        .map_err(|e| crate::Error::Msg(format!("count aircraft_ref: {e}")))?;
    let mut stmt = conn
        .prepare(
            "SELECT a.n_number, a.serial_number, a.type_registrant, a.owner_name,
                    a.street, a.city, a.state, a.type_aircraft, a.type_engine,
                    a.status_code, a.fractional_owner, a.icao24,
                    COALESCE(r.mfr, ''), COALESCE(r.model, '')
             FROM aircraft a
             LEFT JOIN aircraft_ref r ON r.code = a.mfr_mdl_code
             WHERE a.is_current = 1",
        )
        .map_err(|e| crate::Error::Msg(format!("prepare current aircraft: {e}")))?;
    let mapped = stmt
        .query_map([], |row| {
            let fract: String = row.get(10)?;
            Ok(Aircraft {
                n_number: canonical_n_number(&row.get::<_, String>(0)?),
                serial: row.get(1)?,
                type_registrant: row.get(2)?,
                registrant_name: row.get(3)?,
                street: row.get(4)?,
                city: row.get(5)?,
                state: row.get(6)?,
                type_aircraft: row.get(7)?,
                type_engine: row.get(8)?,
                status_code: row.get(9)?,
                fractional_owner: fract.eq_ignore_ascii_case("Y"),
                icao24: canonical_icao24(&row.get::<_, String>(11)?),
                make: row.get(12)?,
                model: row.get(13)?,
            })
        })
        .map_err(|e| crate::Error::Msg(format!("query current aircraft: {e}")))?;
    let mut out = Vec::new();
    for row in mapped {
        out.push(row.map_err(|e| crate::Error::Msg(format!("aircraft row: {e}")))?);
    }
    Ok((out, acftref_n as usize))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn reads_current_row_and_ref_join() {
        let dir = std::env::temp_dir().join(format!(
            "faa-pub-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("faa-registry.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE aircraft (
                n_number TEXT, serial_number TEXT, type_registrant TEXT, owner_name TEXT,
                street TEXT, city TEXT, state TEXT, type_aircraft TEXT, type_engine TEXT,
                status_code TEXT, fractional_owner TEXT, icao24 TEXT, mfr_mdl_code TEXT,
                is_current INTEGER
             );
             CREATE TABLE aircraft_ref (code TEXT PRIMARY KEY, mfr TEXT, model TEXT);
             INSERT INTO aircraft_ref VALUES ('G650', 'GULFSTREAM', 'GVI G650');
             INSERT INTO aircraft VALUES (
                'N1WM','650','3','WALMART INC','S','BENTONVILLE','AR','5','5',
                'V','N','a00b1c','G650',1
             );
             INSERT INTO aircraft VALUES (
                'NOLD','1','3','OLD','S','X','TX','5','5','V','N','ffffff','G650',0
             );",
        )
        .unwrap();
        drop(conn);
        let (rows, nref) = load_current_aircraft(&path).unwrap();
        assert_eq!(nref, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].n_number, "N1WM");
        assert_eq!(rows[0].make, "GULFSTREAM");
        assert_eq!(rows[0].model, "GVI G650");
        assert_eq!(rows[0].icao24, "a00b1c");
        assert!(!rows[0].fractional_owner);
        let _ = std::fs::remove_dir_all(dir);
    }
}
