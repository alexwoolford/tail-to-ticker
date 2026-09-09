use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use faa_ingest::{load_current_aircraft, parse_registry_zip, DEFAULT_PUBLISHED_DB};
use resolve::{
    annotate_aviation_issuer, apply_issuer_aliases, apply_scd2, evaluate_gold, is_utc_date,
    load_aviation_issuers, load_edgar_jsonl, load_gold, load_issuer_aliases, load_overrides,
    open_db, published_row_floor, resolve_all, unpublished_by_n, AviationIssuers, EdgarHit,
    GoldFile, Mapping,
};
use sec_universe::{
    apply_addresses, download_ex21_parquet, download_tickers, load_addresses_json, load_ex21,
    load_tickers, Company, Subsidiary,
};

const MIN_PRODUCTION_MASTER_ROWS: usize = 300_000;

pub struct RefreshOpts {
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub user_agent: String,
    pub as_of: Option<String>,
    pub faa_zip: Option<PathBuf>,
    pub faa_db: Option<PathBuf>,
    pub tickers_json: Option<PathBuf>,
    pub ex21: Option<PathBuf>,
    pub addresses_json: Option<PathBuf>,
    pub edgar_jsonl: Option<PathBuf>,
    pub overrides: PathBuf,
    pub gold: PathBuf,
    pub aviation_issuers: PathBuf,
    pub issuer_aliases: PathBuf,
    pub skip_download: bool,
    pub use_cache: bool,
    pub skip_pudl: bool,
    pub publish_address_cluster: bool,
}

#[derive(Clone, Copy)]
enum SourceMode {
    Fresh,
    Cache,
    Offline,
}

impl SourceMode {
    fn from_flags(skip_download: bool, use_cache: bool) -> Self {
        if skip_download {
            Self::Offline
        } else if use_cache {
            Self::Cache
        } else {
            Self::Fresh
        }
    }

    fn skip_master_floor(&self) -> bool {
        matches!(self, Self::Offline)
    }
}

/// Offline, `--use-cache` hit, or an explicit path that exists: read local, do not download.
fn use_local(mode: SourceMode, explicit: bool, exists: bool) -> bool {
    (explicit && exists)
        || match mode {
            SourceMode::Offline => true,
            SourceMode::Cache => exists,
            SourceMode::Fresh => false,
        }
}

pub async fn run(opts: RefreshOpts) -> Result<()> {
    let as_of = opts
        .as_of
        .clone()
        .unwrap_or_else(|| Utc::now().date_naive().to_string());
    if !is_utc_date(&as_of) {
        anyhow::bail!("--as-of must be UTC calendar day YYYY-MM-DD, got {as_of:?}");
    }
    std::fs::create_dir_all(&opts.cache_dir)?;
    std::fs::create_dir_all(opts.data_dir.join("current"))?;
    std::fs::create_dir_all(opts.data_dir.join("snapshots").join(&as_of))?;

    let mode = SourceMode::from_flags(opts.skip_download, opts.use_cache);
    let sources = load_sources(&opts, mode).await?;
    let out = resolve_and_gate(&opts, &as_of, &sources)?;
    write_outputs(&opts, &as_of, out)?;
    Ok(())
}

struct Sources {
    aircraft: Vec<faa_ingest::Aircraft>,
    zip_path: PathBuf,
    companies: Vec<Company>,
    subsidiaries: Vec<Subsidiary>,
    edgar_hits: Vec<EdgarHit>,
    overrides: std::collections::HashMap<String, resolve::OverrideEntry>,
    unpublished: std::collections::HashMap<String, String>,
    gold: Option<GoldFile>,
}

async fn load_sources(opts: &RefreshOpts, mode: SourceMode) -> Result<Sources> {
    let (aircraft, faa_source, acftref_n) = load_faa_aircraft(opts)?;
    tracing::info!(
        aircraft = aircraft.len(),
        acftref = acftref_n,
        source = %faa_source.display(),
        "loaded FAA registry"
    );
    check_master_count(aircraft.len(), mode.skip_master_floor())?;

    let mut companies = load_ticker_universe(opts, mode).await?;
    tracing::info!(companies = companies.len(), "SEC ticker universe");

    if let Some(p) = &opts.addresses_json {
        let addrs = load_addresses_json(p)?;
        apply_addresses(&mut companies, &addrs);
        tracing::info!(addresses = addrs.len(), "merged HQ addresses");
    } else {
        let p = opts.cache_dir.join("sec_addresses.json");
        if p.exists() {
            let addrs = load_addresses_json(&p)?;
            apply_addresses(&mut companies, &addrs);
            tracing::info!(addresses = addrs.len(), "merged cached HQ addresses");
        }
    }

    if opts.issuer_aliases.exists() {
        let aliases = load_issuer_aliases(&opts.issuer_aliases)
            .with_context(|| format!("issuer aliases {}", opts.issuer_aliases.display()))?;
        let n = apply_issuer_aliases(&mut companies, &aliases);
        tracing::info!(
            aliases = aliases.len(),
            applied = n,
            "issuer identity aliases"
        );
    } else {
        tracing::warn!(
            path = %opts.issuer_aliases.display(),
            "issuer_aliases file missing"
        );
    }

    let ex21_explicit = opts.ex21.is_some();
    let subsidiaries = load_ex21_rows(opts, mode, ex21_explicit).await?;
    tracing::info!(subsidiaries = subsidiaries.len(), "Exhibit 21 aliases");
    if subsidiaries.is_empty() && !opts.skip_pudl && !ex21_explicit {
        anyhow::bail!(
            "Exhibit 21 loaded 0 subsidiaries; refusing to refresh (would drop EX-21 mappings). Pass --skip-pudl for a name-only run."
        );
    }

    let edgar_path = resolve_edgar_jsonl(opts.edgar_jsonl.clone(), &opts.overrides);
    let edgar_hits = match &edgar_path {
        Some(p) if p.exists() => load_edgar_jsonl(p)?,
        _ => Vec::new(),
    };
    tracing::info!(
        path = %edgar_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none)".into()),
        edgar_hits = edgar_hits.len(),
        "EDGAR allowlist hits"
    );

    let overrides = if opts.overrides.exists() {
        load_overrides(&opts.overrides)?
    } else {
        tracing::warn!(path = %opts.overrides.display(), "overrides file missing");
        Default::default()
    };
    let gold = if opts.gold.exists() {
        Some(load_gold(&opts.gold).with_context(|| format!("gold file {}", opts.gold.display()))?)
    } else {
        tracing::warn!(path = %opts.gold.display(), "gold file missing; unpublished-tail gate skipped");
        None
    };
    let unpublished = gold.as_ref().map(unpublished_by_n).unwrap_or_default();

    Ok(Sources {
        aircraft,
        zip_path: faa_source,
        companies,
        subsidiaries,
        edgar_hits,
        overrides,
        unpublished,
        gold,
    })
}

async fn load_ticker_universe(opts: &RefreshOpts, mode: SourceMode) -> Result<Vec<Company>> {
    if let Some(p) = &opts.tickers_json {
        return load_tickers(p).map_err(Into::into);
    }
    let cached = opts.cache_dir.join("company_tickers_exchange.json");
    if use_local(mode, false, cached.exists()) {
        if !cached.exists() {
            anyhow::bail!("tickers JSON missing");
        }
        return load_tickers(&cached).map_err(Into::into);
    }
    require_network_user_agent(&opts.user_agent)?;
    match download_tickers(&opts.user_agent).await {
        Ok(rows) => {
            std::fs::write(&cached, serde_json::to_vec(&simple_tickers_dump(&rows))?)?;
            Ok(rows)
        }
        Err(e) if e.is_http_forbidden() && cached.exists() => {
            tracing::warn!(
                error = %e,
                path = %cached.display(),
                "SEC ticker GET 403; using cache (this egress is blocked; IPRoyal CONNECT to sec.gov is also 403)"
            );
            load_tickers(&cached).map_err(Into::into)
        }
        Err(e) => Err(e.into()),
    }
}

async fn load_ex21_rows(
    opts: &RefreshOpts,
    mode: SourceMode,
    ex21_explicit: bool,
) -> Result<Vec<Subsidiary>> {
    let path = opts
        .ex21
        .clone()
        .unwrap_or_else(|| opts.cache_dir.join("ex21.parquet"));
    let csv_fallback = opts.cache_dir.join("ex21.csv");
    if ex21_explicit {
        return load_ex21(&path).with_context(|| format!("load Exhibit 21 {}", path.display()));
    }
    if opts.skip_pudl {
        return Ok(Vec::new());
    }
    let local = path.exists() || csv_fallback.exists();
    if use_local(mode, false, local) {
        if path.exists() {
            return load_ex21(&path).with_context(|| format!("load Exhibit 21 {}", path.display()));
        }
        if csv_fallback.exists() {
            return load_ex21(&csv_fallback)
                .with_context(|| format!("load Exhibit 21 {}", csv_fallback.display()));
        }
        anyhow::bail!(
            "Exhibit 21 file missing at {} and --skip-download/--use-cache set",
            path.display()
        );
    }
    require_network_user_agent(&opts.user_agent)?;
    download_ex21_parquet(&opts.user_agent, &path)
        .await
        .with_context(|| "PUDL Exhibit 21 download")?;
    load_ex21(&path).with_context(|| format!("load Exhibit 21 {}", path.display()))
}

struct Gated {
    published: Vec<Mapping>,
    review_queue: Vec<Mapping>,
    unresolved: Vec<resolve::Unresolved>,
    conflicts: Vec<resolve::Conflict>,
}

fn resolve_and_gate(opts: &RefreshOpts, as_of: &str, sources: &Sources) -> Result<Gated> {
    let faa_source = format!("faa:{}", sources.zip_path.display());
    let mut out = resolve_all(
        &sources.aircraft,
        &sources.companies,
        &sources.subsidiaries,
        &sources.edgar_hits,
        &sources.overrides,
        &sources.unpublished,
        as_of,
        &faa_source,
        opts.publish_address_cluster,
    );

    tracing::info!(
        published = out.published.len(),
        review = out.review_queue.len(),
        unresolved = out.unresolved.len(),
        conflicts = out.conflicts.len(),
        excluded = out.excluded_count,
        "resolved"
    );

    let issuers = if opts.aviation_issuers.exists() {
        load_aviation_issuers(&opts.aviation_issuers)
            .with_context(|| format!("aviation issuers {}", opts.aviation_issuers.display()))?
    } else {
        tracing::warn!(
            path = %opts.aviation_issuers.display(),
            "aviation_issuers file missing; aviation_issuer will be false"
        );
        AviationIssuers::default()
    };
    annotate_aviation_issuer(&mut out.published, &issuers);

    let db_path = opts.data_dir.join("current").join("tail_to_ticker.sqlite");
    let previous_n = if db_path.exists() {
        open_db(&db_path)?.current_mapping_count()?
    } else {
        0
    };
    if let Some(min) = published_row_floor(previous_n) {
        if out.published.len() < min {
            anyhow::bail!(
                "published {} rows is below floor {min} (previous {previous_n}); refusing to overwrite",
                out.published.len()
            );
        }
    }

    if let Some(gold_file) = &sources.gold {
        let trusts = out
            .unresolved
            .iter()
            .filter(|u| u.reason == "trustee")
            .count();
        let report = evaluate_gold(gold_file, &out.published, &sources.aircraft, trusts);
        print!("{report}");
        if report.tail_false_positives > 0 {
            anyhow::bail!(
                "{} tail false positive(s): published as a forbidden ticker; not writing feed",
                report.tail_false_positives
            );
        }
    }

    Ok(Gated {
        published: out.published,
        review_queue: out.review_queue,
        unresolved: out.unresolved,
        conflicts: out.conflicts,
    })
}

fn load_faa_aircraft(opts: &RefreshOpts) -> Result<(Vec<faa_ingest::Aircraft>, PathBuf, usize)> {
    if let Some(zip) = &opts.faa_zip {
        if !zip.exists() {
            anyhow::bail!("FAA zip not found at {}", zip.display());
        }
        let (aircraft, acftref_n) = parse_registry_zip(&std::fs::read(zip)?)?;
        return Ok((aircraft, zip.clone(), acftref_n));
    }
    let db = opts.faa_db.clone().unwrap_or_else(|| {
        let host = PathBuf::from(DEFAULT_PUBLISHED_DB);
        if host.exists() {
            host
        } else {
            opts.cache_dir.join("faa-registry.sqlite")
        }
    });
    if !db.exists() {
        anyhow::bail!(
            "FAA published sqlite not found at {} (faa-registry-mirror current/). Pass --faa-db or --faa-zip for fixtures. This job does not download ReleasableAircraft.zip.",
            db.display()
        );
    }
    let (aircraft, acftref_n) = load_current_aircraft(&db)?;
    Ok((aircraft, db, acftref_n))
}

fn write_outputs(opts: &RefreshOpts, as_of: &str, out: Gated) -> Result<()> {
    let snap = opts.data_dir.join("snapshots").join(as_of);

    let db_path = opts.data_dir.join("current").join("tail_to_ticker.sqlite");
    let mut db = open_db(&db_path)?;
    let log = apply_scd2(
        &mut db,
        as_of,
        &out.published,
        &out.review_queue,
        &out.unresolved,
    )?;
    db.wal_checkpoint()?;
    drop(db);
    std::fs::copy(&db_path, snap.join("tail_to_ticker.sqlite"))?;

    let mut buf = String::new();
    for e in &log {
        buf.push_str(&serde_json::to_string(e)?);
        buf.push('\n');
    }
    std::fs::write(snap.join("changelog.jsonl"), buf)?;

    write_json(&snap.join("conflicts.json"), &out.conflicts)?;
    write_json(
        &snap.join("unresolved_trusts.json"),
        &out.unresolved
            .iter()
            .filter(|u| u.reason == "trustee")
            .collect::<Vec<_>>(),
    )?;

    println!(
        "as_of={as_of} published={} review={} trusts={} conflicts={} changelog={}",
        out.published.len(),
        out.review_queue.len(),
        out.unresolved
            .iter()
            .filter(|u| u.reason == "trustee")
            .count(),
        out.conflicts.len(),
        log.len()
    );
    Ok(())
}

fn placeholder_user_agent(user_agent: &str) -> bool {
    let t = user_agent.trim().to_ascii_lowercase();
    t.is_empty() || t.contains("example.com")
}

fn require_network_user_agent(user_agent: &str) -> Result<()> {
    if placeholder_user_agent(user_agent) {
        anyhow::bail!(
            "Set SEC_USER_AGENT to a real descriptive contact (not example.com) before downloading"
        );
    }
    Ok(())
}

fn write_json<T: serde::Serialize>(path: &Path, val: &T) -> Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(val)?)?;
    Ok(())
}

fn simple_tickers_dump(rows: &[Company]) -> serde_json::Value {
    serde_json::json!({
        "fields": ["cik", "name", "ticker", "exchange"],
        "data": rows.iter().map(|c| serde_json::json!([
            c.cik.parse::<u64>().unwrap_or(0),
            c.name,
            c.ticker,
            c.exchange
        ])).collect::<Vec<_>>(),
    })
}

/// Labeled TPs next to `mappings.yaml`. Harvest dumps (`evidence/*.jsonl`) are never implicit.
pub(crate) fn resolve_edgar_jsonl(
    explicit: Option<PathBuf>,
    overrides_path: &Path,
) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    let allow = overrides_path
        .parent()
        .unwrap_or_else(|| Path::new("overrides"))
        .join("edgar_allowlist.jsonl");
    allow.exists().then_some(allow)
}

fn check_master_count(n: usize, skip_floor: bool) -> Result<()> {
    if n == 0 {
        anyhow::bail!("FAA MASTER parsed 0 rows; refusing to refresh");
    }
    if !skip_floor && n < MIN_PRODUCTION_MASTER_ROWS {
        anyhow::bail!(
            "FAA MASTER parsed {n} rows (min {MIN_PRODUCTION_MASTER_ROWS}); refusing to refresh"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_floor_refuses_truncated_parse() {
        assert!(check_master_count(0, false).is_err());
        assert!(check_master_count(299_999, false).is_err());
        assert!(check_master_count(MIN_PRODUCTION_MASTER_ROWS, false).is_ok());
        assert!(check_master_count(1, true).is_ok());
    }

    #[test]
    fn default_edgar_path_is_allowlist_not_evidence_hits() {
        let dir = std::env::temp_dir().join(format!(
            "ttt-edgar-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let evidence = dir.join("evidence");
        let overrides = dir.join("overrides");
        std::fs::create_dir_all(&evidence).unwrap();
        std::fs::create_dir_all(&overrides).unwrap();
        let evidence_hits = evidence.join("edgar_hits.jsonl");
        std::fs::write(&evidence_hits, "{\"n_number\":\"N147CJ\"}\n").unwrap();
        let mappings = overrides.join("mappings.yaml");
        std::fs::write(&mappings, "").unwrap();
        let allow = overrides.join("edgar_allowlist.jsonl");
        std::fs::write(&allow, "").unwrap();

        let got = resolve_edgar_jsonl(None, &mappings).expect("allowlist");
        assert_eq!(got, allow);
        assert_ne!(got, evidence_hits);

        let fixture = dir.join("fixture.jsonl");
        std::fs::write(&fixture, "").unwrap();
        assert_eq!(
            resolve_edgar_jsonl(Some(fixture.clone()), &mappings),
            Some(fixture)
        );

        std::fs::remove_file(&allow).unwrap();
        assert!(
            resolve_edgar_jsonl(None, &mappings).is_none(),
            "missing allowlist must not fall back to evidence/edgar_hits.jsonl"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
