use std::path::Path;

use capturable_state::{
    apply_runtime_pragmas, install, table_is_strict, CaptureConfig, CaptureMode, Nudge, TableSpec,
};
use rusqlite::{params, Connection, OptionalExtension};

use crate::{require_utc_date, require_utc_instant, ChangelogEntry, Mapping, Unresolved};

pub struct FeedDb {
    conn: Connection,
    nudge: Nudge,
}

const FEED_DDL: &str = r#"
        CREATE TABLE IF NOT EXISTS mappings_current (
            n_number TEXT PRIMARY KEY,
            icao24 TEXT,
            serial TEXT,
            make TEXT,
            model TEXT,
            ticker TEXT NOT NULL,
            cik TEXT,
            company_name TEXT,
            registrant_name TEXT,
            match_method TEXT,
            as_of_date TEXT,
            source_url TEXT,
            fleet_size INTEGER NOT NULL DEFAULT 0,
            aviation_issuer INTEGER NOT NULL DEFAULT 0,
            deleted_at INTEGER
        ) STRICT;
        CREATE TABLE IF NOT EXISTS unresolved_trusts (
            n_number TEXT PRIMARY KEY,
            icao24 TEXT,
            make TEXT,
            model TEXT,
            registrant_name TEXT,
            reason TEXT,
            as_of_date TEXT
        ) STRICT;
        CREATE TABLE IF NOT EXISTS review_queue (
            n_number TEXT PRIMARY KEY,
            icao24 TEXT,
            serial TEXT,
            make TEXT,
            model TEXT,
            ticker TEXT,
            cik TEXT,
            company_name TEXT,
            registrant_name TEXT,
            match_method TEXT,
            as_of_date TEXT,
            source_url TEXT
        ) STRICT;
        CREATE TABLE IF NOT EXISTS changelog (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            as_of_date TEXT,
            n_number TEXT,
            change TEXT,
            detail TEXT
        ) STRICT;
        CREATE INDEX IF NOT EXISTS idx_changelog_date ON changelog(as_of_date);
        CREATE TABLE IF NOT EXISTS refresh_run (
            as_of_date TEXT PRIMARY KEY,
            recorded_at TEXT NOT NULL
        ) STRICT;
        "#;

const DB_NAME: &str = "tail-to-ticker";
const MAPPING_EXCLUDE: &[&str] = &["fleet_size", "aviation_issuer"];

pub fn open_db(path: &Path) -> anyhow::Result<FeedDb> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    apply_runtime_pragmas(&conn)?;
    conn.execute_batch(FEED_DDL)?;
    ensure_column(&conn, "mappings_current", "deleted_at", "INTEGER")?;
    migrate_strict(&conn)?;
    conn.execute_batch(FEED_DDL)?;
    let nudge = install_capture(&conn, path)?;
    Ok(FeedDb { conn, nudge })
}

fn install_capture(conn: &Connection, path: &Path) -> anyhow::Result<Nudge> {
    let tables = [
        TableSpec::new("mappings_current", CaptureMode::Full).exclude(MAPPING_EXCLUDE),
        TableSpec::new("refresh_run", CaptureMode::After),
        TableSpec::new("changelog", CaptureMode::After),
    ];
    install(conn, &CaptureConfig::new(DB_NAME, path, &tables))
}

fn ensure_column(conn: &Connection, table: &str, column: &str, decl: &str) -> anyhow::Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let exists = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .any(|name| name.as_deref() == Ok(column));
    if !exists {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"),
            [],
        )?;
    }
    Ok(())
}

fn migrate_strict(conn: &Connection) -> anyhow::Result<()> {
    if table_is_strict(conn, "mappings_current")? {
        return Ok(());
    }
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    conn.execute_batch(
        r#"
        CREATE TABLE mappings_current_strict (
            n_number TEXT PRIMARY KEY,
            icao24 TEXT,
            serial TEXT,
            make TEXT,
            model TEXT,
            ticker TEXT NOT NULL,
            cik TEXT,
            company_name TEXT,
            registrant_name TEXT,
            match_method TEXT,
            as_of_date TEXT,
            source_url TEXT,
            fleet_size INTEGER NOT NULL DEFAULT 0,
            aviation_issuer INTEGER NOT NULL DEFAULT 0,
            deleted_at INTEGER
        ) STRICT;
        INSERT INTO mappings_current_strict
            (n_number, icao24, serial, make, model, ticker, cik, company_name,
             registrant_name, match_method, as_of_date, source_url, fleet_size,
             aviation_issuer, deleted_at)
        SELECT n_number, icao24, serial, make, model, ticker, cik, company_name,
               registrant_name, match_method, as_of_date, source_url, fleet_size,
               aviation_issuer, deleted_at
        FROM mappings_current;
        DROP TABLE mappings_current;
        ALTER TABLE mappings_current_strict RENAME TO mappings_current;

        CREATE TABLE unresolved_trusts_strict (
            n_number TEXT PRIMARY KEY,
            icao24 TEXT,
            make TEXT,
            model TEXT,
            registrant_name TEXT,
            reason TEXT,
            as_of_date TEXT
        ) STRICT;
        INSERT INTO unresolved_trusts_strict SELECT * FROM unresolved_trusts;
        DROP TABLE unresolved_trusts;
        ALTER TABLE unresolved_trusts_strict RENAME TO unresolved_trusts;

        CREATE TABLE review_queue_strict (
            n_number TEXT PRIMARY KEY,
            icao24 TEXT,
            serial TEXT,
            make TEXT,
            model TEXT,
            ticker TEXT,
            cik TEXT,
            company_name TEXT,
            registrant_name TEXT,
            match_method TEXT,
            as_of_date TEXT,
            source_url TEXT
        ) STRICT;
        INSERT INTO review_queue_strict SELECT * FROM review_queue;
        DROP TABLE review_queue;
        ALTER TABLE review_queue_strict RENAME TO review_queue;

        CREATE TABLE changelog_strict (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            as_of_date TEXT,
            n_number TEXT,
            change TEXT,
            detail TEXT
        ) STRICT;
        INSERT INTO changelog_strict (as_of_date, n_number, change, detail)
        SELECT as_of_date, n_number, change, detail FROM changelog;
        DROP TABLE changelog;
        ALTER TABLE changelog_strict RENAME TO changelog;

        CREATE TABLE refresh_run_strict (
            as_of_date TEXT PRIMARY KEY,
            recorded_at TEXT NOT NULL
        ) STRICT;
        INSERT INTO refresh_run_strict SELECT * FROM refresh_run;
        DROP TABLE refresh_run;
        ALTER TABLE refresh_run_strict RENAME TO refresh_run;
        "#,
    )?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

impl FeedDb {
    pub fn current_mappings(&self) -> anyhow::Result<Vec<Mapping>> {
        let mut stmt = self.conn.prepare(
            "SELECT n_number, icao24, serial, make, model, ticker, cik, company_name,
                    registrant_name, match_method, as_of_date, source_url, fleet_size,
                    aviation_issuer
             FROM mappings_current
             WHERE deleted_at IS NULL
             ORDER BY n_number",
        )?;
        let rows = stmt.query_map([], mapping_from_row)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn changelog_for(&self, as_of: &str) -> anyhow::Result<Vec<ChangelogEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT as_of_date, n_number, change, detail FROM changelog WHERE as_of_date = ?1 ORDER BY n_number",
        )?;
        let rows = stmt.query_map([as_of], |r| {
            Ok(ChangelogEntry {
                as_of_date: r.get(0)?,
                n_number: r.get(1)?,
                change: r.get(2)?,
                detail: r.get(3)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn current_mapping_count(&self) -> anyhow::Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM mappings_current WHERE deleted_at IS NULL",
            [],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// Flush WAL into the main file so a byte-copy of this path is consistent.
    pub fn wal_checkpoint(&self) -> anyhow::Result<()> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    pub fn unresolved_trust_count(&self) -> anyhow::Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM unresolved_trusts", [], |r| r.get(0))?)
    }
}

fn mapping_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Mapping> {
    Ok(Mapping {
        n_number: row.get(0)?,
        icao24: row.get(1)?,
        serial: row.get(2)?,
        make: row.get(3)?,
        model: row.get(4)?,
        ticker: row.get(5)?,
        cik: row.get(6)?,
        company_name: row.get(7)?,
        registrant_name: row.get(8)?,
        match_method: row.get(9)?,
        as_of_date: row.get(10)?,
        source_url: row.get(11)?,
        fleet_size: row.get::<_, i64>(12)? as u32,
        aviation_issuer: row.get::<_, i64>(13)? != 0,
    })
}

pub fn lookup(db: &FeedDb, n_number: &str) -> anyhow::Result<Option<Mapping>> {
    let n = faa_ingest::canonical_n_number(n_number);
    let mut stmt = db.conn.prepare(
        "SELECT n_number, icao24, serial, make, model, ticker, cik, company_name,
                registrant_name, match_method, as_of_date, source_url, fleet_size,
                aviation_issuer
         FROM mappings_current WHERE n_number = ?1 AND deleted_at IS NULL",
    )?;
    Ok(stmt.query_row(params![n], mapping_from_row).optional()?)
}

/// Upsert today's published set; changelog new/dropped/updated; rewrite review/unresolved.
pub fn apply_scd2(
    db: &mut FeedDb,
    as_of: &str,
    published: &[Mapping],
    review: &[Mapping],
    unresolved: &[Unresolved],
) -> anyhow::Result<Vec<ChangelogEntry>> {
    apply_scd2_at(
        db,
        as_of,
        published,
        review,
        unresolved,
        &crate::utc_iso(chrono::Utc::now()),
    )
}

/// Same as [`apply_scd2`] with an explicit write instant (tests).
pub fn apply_scd2_at(
    db: &mut FeedDb,
    as_of: &str,
    published: &[Mapping],
    review: &[Mapping],
    unresolved: &[Unresolved],
    recorded_at: &str,
) -> anyhow::Result<Vec<ChangelogEntry>> {
    require_utc_date(as_of, "as_of")?;
    require_utc_instant(recorded_at, "recorded_at")?;
    let previous = db.current_mappings()?;
    for m in previous.iter().chain(published.iter()).chain(review.iter()) {
        require_utc_date(&m.as_of_date, "as_of_date")?;
    }
    let prev_map: std::collections::HashMap<_, _> = previous
        .into_iter()
        .map(|m| (m.n_number.clone(), m))
        .collect();
    let new_map: std::collections::HashMap<_, _> = published
        .iter()
        .map(|m| (m.n_number.clone(), m.clone()))
        .collect();

    let mut log = Vec::new();
    let tx = db.conn.transaction()?;

    for (n, old) in &prev_map {
        match new_map.get(n) {
            None => {
                tx.execute(
                    "UPDATE mappings_current
                     SET deleted_at = CAST(strftime('%s','now') AS INTEGER)
                     WHERE n_number = ?1 AND deleted_at IS NULL",
                    params![n],
                )?;
                log.push(chg(
                    as_of,
                    n,
                    "dropped",
                    &format!("{} {}", old.ticker, old.match_method),
                ));
            }
            Some(new) if mapping_identity(old) != mapping_identity(new) => {
                let kind = if old.ticker != new.ticker {
                    "owner_changed"
                } else {
                    "updated"
                };
                upsert_current(&tx, new)?;
                log.push(chg(
                    as_of,
                    n,
                    kind,
                    &format!(
                        "{} {} -> {} {}",
                        old.ticker, old.match_method, new.ticker, new.match_method
                    ),
                ));
            }
            Some(_new) => {
                // Identity unchanged: do not rewrite the row (quiet days must not
                // emit a no-op `U` for fleet_size / aviation_issuer churn).
            }
        }
    }

    for (n, new) in &new_map {
        if !prev_map.contains_key(n) {
            upsert_current(&tx, new)?;
            log.push(chg(
                as_of,
                n,
                "new",
                &format!("{} {}", new.ticker, new.match_method),
            ));
        }
    }

    tx.execute("DELETE FROM review_queue", [])?;
    for m in review {
        upsert_review(&tx, m)?;
    }

    tx.execute("DELETE FROM unresolved_trusts", [])?;
    for u in unresolved {
        if u.reason == "trustee" {
            tx.execute(
                "INSERT INTO unresolved_trusts
                 (n_number, icao24, make, model, registrant_name, reason, as_of_date)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    u.n_number,
                    u.icao24,
                    u.make,
                    u.model,
                    u.registrant_name,
                    u.reason,
                    as_of
                ],
            )?;
        }
    }

    for e in &log {
        tx.execute(
            "INSERT INTO changelog (as_of_date, n_number, change, detail) VALUES (?1,?2,?3,?4)",
            params![e.as_of_date, e.n_number, e.change, e.detail],
        )?;
    }

    tx.execute(
        "INSERT INTO refresh_run (as_of_date, recorded_at) VALUES (?1, ?2)
         ON CONFLICT(as_of_date) DO UPDATE SET recorded_at = excluded.recorded_at",
        params![as_of, recorded_at],
    )?;

    tx.commit()?;
    db.nudge.send();
    Ok(log)
}

fn mapping_identity(m: &Mapping) -> String {
    format!(
        "{}|{}|{}|{}",
        m.ticker, m.cik, m.match_method, m.registrant_name
    )
}

fn chg(as_of: &str, n: &str, change: &str, detail: &str) -> ChangelogEntry {
    ChangelogEntry {
        as_of_date: as_of.into(),
        n_number: n.into(),
        change: change.into(),
        detail: detail.into(),
    }
}

fn upsert_current(tx: &rusqlite::Transaction<'_>, m: &Mapping) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO mappings_current
         (n_number, icao24, serial, make, model, ticker, cik, company_name, registrant_name,
          match_method, as_of_date, source_url, fleet_size, aviation_issuer, deleted_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,NULL)
         ON CONFLICT(n_number) DO UPDATE SET
            icao24=excluded.icao24, serial=excluded.serial, make=excluded.make, model=excluded.model,
            ticker=excluded.ticker, cik=excluded.cik, company_name=excluded.company_name,
            registrant_name=excluded.registrant_name, match_method=excluded.match_method,
            as_of_date=excluded.as_of_date, source_url=excluded.source_url,
            fleet_size=excluded.fleet_size, aviation_issuer=excluded.aviation_issuer,
            deleted_at=NULL",
        params![
            m.n_number,
            m.icao24,
            m.serial,
            m.make,
            m.model,
            m.ticker,
            m.cik,
            m.company_name,
            m.registrant_name,
            m.match_method,
            m.as_of_date,
            m.source_url,
            m.fleet_size as i64,
            i64::from(m.aviation_issuer),
        ],
    )?;
    Ok(())
}

fn upsert_review(tx: &rusqlite::Transaction<'_>, m: &Mapping) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO review_queue
         (n_number, icao24, serial, make, model, ticker, cik, company_name, registrant_name,
          match_method, as_of_date, source_url)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
         ON CONFLICT(n_number) DO UPDATE SET
            ticker=excluded.ticker, match_method=excluded.match_method, as_of_date=excluded.as_of_date",
        params![
            m.n_number,
            m.icao24,
            m.serial,
            m.make,
            m.model,
            m.ticker,
            m.cik,
            m.company_name,
            m.registrant_name,
            m.match_method,
            m.as_of_date,
            m.source_url,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mapping;

    fn map(n: &str, ticker: &str, method: &str, as_of: &str) -> Mapping {
        Mapping {
            n_number: n.into(),
            icao24: "abc".into(),
            serial: "1".into(),
            make: "GULFSTREAM".into(),
            model: "G650".into(),
            ticker: ticker.into(),
            cik: "0000104169".into(),
            company_name: "Walmart".into(),
            registrant_name: "WALMART INC".into(),
            match_method: method.into(),
            as_of_date: as_of.into(),
            source_url: "faa".into(),
            fleet_size: 1,
            aviation_issuer: false,
        }
    }

    #[test]
    fn scd2_new_then_drop() {
        let dir = std::env::temp_dir().join(format!("ttt-scd2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut db = open_db(&dir.join("feed.sqlite")).unwrap();
        let log = apply_scd2(
            &mut db,
            "2026-01-01",
            &[map("N1WM", "WMT", "exact_legal_name", "2026-01-01")],
            &[],
            &[],
        )
        .unwrap();
        assert_eq!(log[0].change, "new");
        assert_eq!(db.current_mappings().unwrap().len(), 1);

        let log = apply_scd2(&mut db, "2026-01-02", &[], &[], &[]).unwrap();
        assert_eq!(log[0].change, "dropped");
        assert!(db.current_mappings().unwrap().is_empty());
        assert!(lookup(&db, "N1WM").unwrap().is_none());
        let n_all: i64 = db
            .conn
            .query_row("SELECT count(*) FROM mappings_current", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n_all, 1);
        let deleted_at: Option<i64> = db
            .conn
            .query_row(
                "SELECT deleted_at FROM mappings_current WHERE n_number = 'N1WM'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(deleted_at.is_some());
        let mapping_ops: Vec<String> = outbox_ops(&db)
            .into_iter()
            .filter(|(tbl, _)| tbl == "mappings_current")
            .map(|(_, op)| op)
            .collect();
        assert_eq!(mapping_ops, vec!["I".to_string(), "U".to_string()]);
        assert!(!mapping_ops.iter().any(|op| op == "D"));
        assert_eq!(db.changelog_for("2026-01-02").unwrap().len(), 1);
    }

    fn outbox_ops(db: &FeedDb) -> Vec<(String, String)> {
        let mut stmt = db
            .conn
            .prepare("SELECT tbl, op FROM _outbox ORDER BY seq")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    #[test]
    fn scd2_quiet_upsert_does_not_refresh_fleet_size() {
        let dir = std::env::temp_dir().join(format!("ttt-scd2-fleet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut db = open_db(&dir.join("feed.sqlite")).unwrap();
        let mut day1 = map("N1WM", "WMT", "exact_legal_name", "2026-01-01");
        day1.fleet_size = 1;
        apply_scd2(&mut db, "2026-01-01", &[day1], &[], &[]).unwrap();

        let mut day2 = map("N1WM", "WMT", "exact_legal_name", "2026-01-02");
        day2.fleet_size = 6;
        let log = apply_scd2(&mut db, "2026-01-02", &[day2], &[], &[]).unwrap();
        assert!(log.is_empty(), "fleet_size churn must not write changelog");
        let got = db.current_mappings().unwrap();
        assert_eq!(got[0].fleet_size, 1);
        let mapping_ops: Vec<String> = outbox_ops(&db)
            .into_iter()
            .filter(|(tbl, _)| tbl == "mappings_current")
            .map(|(_, op)| op)
            .collect();
        assert_eq!(mapping_ops, vec!["I".to_string()]);
    }

    #[test]
    fn scd2_quiet_upsert_does_not_refresh_aviation_issuer() {
        let dir = std::env::temp_dir().join(format!("ttt-scd2-avn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut db = open_db(&dir.join("feed.sqlite")).unwrap();
        let mut day1 = map("N1027P", "TXT", "ex21_subsidiary", "2026-01-01");
        day1.aviation_issuer = false;
        apply_scd2(&mut db, "2026-01-01", &[day1], &[], &[]).unwrap();

        let mut day2 = map("N1027P", "TXT", "ex21_subsidiary", "2026-01-02");
        day2.aviation_issuer = true;
        let log = apply_scd2(&mut db, "2026-01-02", &[day2], &[], &[]).unwrap();
        assert!(
            log.is_empty(),
            "aviation_issuer churn must not write changelog"
        );
        let got = db.current_mappings().unwrap();
        assert!(!got[0].aviation_issuer);
    }

    #[test]
    fn opens_with_wal() {
        let dir = std::env::temp_dir().join(format!("ttt-wal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = open_db(&dir.join("feed.sqlite")).unwrap();
        let mode: String = db
            .conn
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_ascii_lowercase(), "wal");
    }

    #[test]
    fn wal_checkpoint_makes_byte_copy_see_new_rows() {
        let dir = std::env::temp_dir().join(format!("ttt-wal-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("feed.sqlite");
        let copy = dir.join("copy.sqlite");
        {
            let mut db = open_db(&path).unwrap();
            apply_scd2(
                &mut db,
                "2026-09-08",
                &[map("N83HD", "HD", "manual_override", "2026-09-08")],
                &[],
                &[],
            )
            .unwrap();
            db.wal_checkpoint().unwrap();
        }
        std::fs::copy(&path, &copy).unwrap();
        let copied = open_db(&copy).unwrap();
        assert_eq!(copied.current_mapping_count().unwrap(), 1);
        assert_eq!(lookup(&copied, "N83HD").unwrap().unwrap().ticker, "HD");
    }

    #[test]
    fn scd2_rejects_instant_as_of() {
        let dir = std::env::temp_dir().join(format!("ttt-scd2-date-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut db = open_db(&dir.join("feed.sqlite")).unwrap();
        let err = apply_scd2(
            &mut db,
            "2026-09-01T00:00:00Z",
            &[map("N1WM", "WMT", "exact_legal_name", "2026-09-01")],
            &[],
            &[],
        )
        .unwrap_err();
        assert!(err.to_string().contains("YYYY-MM-DD"));
        assert!(db.current_mappings().unwrap().is_empty());
        let n: i64 = db
            .conn
            .query_row("SELECT count(*) FROM refresh_run", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    fn refresh_run(db: &FeedDb, as_of: &str) -> (String, String) {
        db.conn
            .query_row(
                "SELECT as_of_date, recorded_at FROM refresh_run WHERE as_of_date = ?1",
                [as_of],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }

    #[test]
    fn scd2_writes_refresh_run_and_same_day_overwrites() {
        let dir = std::env::temp_dir().join(format!("ttt-scd2-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut db = open_db(&dir.join("feed.sqlite")).unwrap();
        apply_scd2_at(
            &mut db,
            "2026-09-01",
            &[map("N1WM", "WMT", "exact_legal_name", "2026-09-01")],
            &[],
            &[],
            "2026-09-01T21:19:58Z",
        )
        .unwrap();
        let (as_of, recorded) = refresh_run(&db, "2026-09-01");
        assert_eq!(as_of, "2026-09-01");
        assert_eq!(recorded, "2026-09-01T21:19:58Z");
        assert!(crate::is_utc_instant(&recorded));
        assert!(crate::is_utc_date(
            &db.current_mappings().unwrap()[0].as_of_date
        ));

        apply_scd2_at(
            &mut db,
            "2026-09-01",
            &[map("N1WM", "WMT", "exact_legal_name", "2026-09-01")],
            &[],
            &[],
            "2026-09-01T21:20:00Z",
        )
        .unwrap();
        let n: i64 = db
            .conn
            .query_row("SELECT count(*) FROM refresh_run", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let (_, recorded) = refresh_run(&db, "2026-09-01");
        assert_eq!(recorded, "2026-09-01T21:20:00Z");
    }
}
