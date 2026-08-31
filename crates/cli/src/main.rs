use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use faa_ingest::{download_registry_to, parse_registry_zip};
use resolve::{
    annotate_aviation_issuer, apply_scd2, evaluate_gold, load_aviation_issuers, load_edgar_jsonl,
    load_gold, load_overrides, lookup, open_db, published_row_floor, resolve_all, unpublished_by_n,
    write_mappings_csv, write_mappings_parquet, AviationIssuers,
};
use sec_universe::{
    apply_addresses, download_ex21_parquet, download_tickers, load_addresses_json, load_ex21,
    load_tickers,
};

#[derive(Parser)]
#[command(
    name = "tail-to-ticker",
    about = "Daily-refreshable N-number → ticker feed"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        default_value = "data",
        env = "TAIL_TO_TICKER_DATA"
    )]
    data_dir: PathBuf,
    #[arg(
        long,
        global = true,
        default_value = "cache",
        env = "TAIL_TO_TICKER_CACHE"
    )]
    cache_dir: PathBuf,
    #[arg(
        long,
        global = true,
        env = "SEC_USER_AGENT",
        default_value = "tail-to-ticker/0.1 (https://github.com/alexwoolford/tail-to-ticker; contact@example.com)"
    )]
    user_agent: String,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Download sources, resolve, and write parquet + SQLite snapshots.
    Refresh {
        #[arg(long)]
        as_of: Option<String>,
        #[arg(long)]
        faa_zip: Option<PathBuf>,
        #[arg(long)]
        tickers_json: Option<PathBuf>,
        #[arg(long)]
        ex21: Option<PathBuf>,
        #[arg(long)]
        addresses_json: Option<PathBuf>,
        #[arg(long)]
        edgar_jsonl: Option<PathBuf>,
        #[arg(long, default_value = "overrides/mappings.yaml")]
        overrides: PathBuf,
        #[arg(long)]
        skip_download: bool,
        /// Reuse cache/FAA zip / tickers / EX-21 if present (no freshness).
        #[arg(long)]
        use_cache: bool,
        #[arg(long)]
        skip_pudl: bool,
        /// Include address_cluster rows in the published feed (default: review queue only).
        #[arg(long)]
        publish_address_cluster: bool,
        #[arg(long, default_value = "overrides/gold.yaml")]
        gold: PathBuf,
        #[arg(long, default_value = "overrides/aviation_issuers.yaml")]
        aviation_issuers: PathBuf,
    },
    /// Look up one N-number in the current SQLite feed.
    Lookup {
        n_number: String,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Print changelog rows for a date (default: latest snapshot date).
    Diff {
        #[arg(long)]
        as_of: Option<String>,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Precision/recall against overrides/gold.yaml.
    Eval {
        #[arg(long, default_value = "overrides/gold.yaml")]
        gold: PathBuf,
        #[arg(long)]
        faa_zip: Option<PathBuf>,
        #[arg(long)]
        db: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let cli = Cli::parse();
    match cli.command {
        Commands::Refresh {
            as_of,
            faa_zip,
            tickers_json,
            ex21,
            addresses_json,
            edgar_jsonl,
            overrides,
            skip_download,
            use_cache,
            skip_pudl,
            publish_address_cluster,
            gold,
            aviation_issuers,
        } => {
            refresh(
                &cli.data_dir,
                &cli.cache_dir,
                &cli.user_agent,
                as_of,
                faa_zip,
                tickers_json,
                ex21,
                addresses_json,
                edgar_jsonl,
                overrides,
                gold,
                aviation_issuers,
                skip_download,
                use_cache,
                skip_pudl,
                publish_address_cluster,
            )
            .await?;
        }
        Commands::Lookup { n_number, db } => {
            let db_path =
                db.unwrap_or_else(|| cli.data_dir.join("current").join("tail_to_ticker.sqlite"));
            let feed = open_db(&db_path)?;
            match lookup(&feed, &n_number)? {
                Some(m) => {
                    println!(
                        "{}  {}  {}  {}  fleet={}  aviation_issuer={}  {}",
                        m.n_number,
                        m.ticker,
                        m.company_name,
                        m.match_method,
                        m.fleet_size,
                        u8::from(m.aviation_issuer),
                        m.registrant_name
                    );
                }
                None => {
                    println!(
                        "{} not in mappings_current",
                        faa_ingest::canonical_n_number(&n_number)
                    );
                    std::process::exit(1);
                }
            }
        }
        Commands::Diff { as_of, db } => {
            let db_path =
                db.unwrap_or_else(|| cli.data_dir.join("current").join("tail_to_ticker.sqlite"));
            let feed = open_db(&db_path)?;
            let date = as_of.unwrap_or_else(|| Utc::now().date_naive().to_string());
            let rows = feed.changelog_for(&date)?;
            if rows.is_empty() {
                println!("no changelog rows for {date}");
            } else {
                for e in rows {
                    println!(
                        "{}  {}  {}  {}",
                        e.as_of_date, e.n_number, e.change, e.detail
                    );
                }
            }
        }
        Commands::Eval { gold, faa_zip, db } => {
            let gold = load_gold(&gold).with_context(|| format!("gold file {}", gold.display()))?;
            let db_path =
                db.unwrap_or_else(|| cli.data_dir.join("current").join("tail_to_ticker.sqlite"));
            let feed = open_db(&db_path)?;
            let published = feed.current_mappings()?;
            let trusts = feed.unresolved_trust_count()?;
            let aircraft = if let Some(zip) = faa_zip {
                let bytes = std::fs::read(&zip)?;
                parse_registry_zip(&bytes)?.0
            } else {
                let zip = cli.cache_dir.join("ReleasableAircraft.zip");
                if zip.exists() {
                    parse_registry_zip(&std::fs::read(&zip)?)?.0
                } else {
                    Vec::new()
                }
            };
            let report = evaluate_gold(&gold, &published, &aircraft, trusts as usize);
            print!("{report}");
            if report.tail_false_positives > 0 {
                anyhow::bail!(
                    "{} tail false positive(s): published as a forbidden ticker",
                    report.tail_false_positives
                );
            }
        }
    }
    Ok(())
}

const MIN_PRODUCTION_MASTER_ROWS: usize = 50_000;

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

#[allow(clippy::too_many_arguments)]
async fn refresh(
    data_dir: &Path,
    cache_dir: &Path,
    user_agent: &str,
    as_of: Option<String>,
    faa_zip: Option<PathBuf>,
    tickers_json: Option<PathBuf>,
    ex21: Option<PathBuf>,
    addresses_json: Option<PathBuf>,
    edgar_jsonl: Option<PathBuf>,
    overrides: PathBuf,
    gold: PathBuf,
    aviation_issuers: PathBuf,
    skip_download: bool,
    use_cache: bool,
    skip_pudl: bool,
    publish_address_cluster: bool,
) -> Result<()> {
    let as_of = as_of.unwrap_or_else(|| Utc::now().date_naive().to_string());
    std::fs::create_dir_all(cache_dir)?;
    std::fs::create_dir_all(data_dir.join("current"))?;
    std::fs::create_dir_all(data_dir.join("snapshots").join(&as_of))?;

    let faa_explicit = faa_zip.is_some();
    let zip_path = faa_zip.unwrap_or_else(|| cache_dir.join("ReleasableAircraft.zip"));
    let faa_bytes = if skip_download || ((use_cache || faa_explicit) && zip_path.exists()) {
        if !zip_path.exists() {
            anyhow::bail!(
                "FAA zip not found at {} and --skip-download/--use-cache set",
                zip_path.display()
            );
        }
        std::fs::read(&zip_path)?
    } else {
        require_network_user_agent(user_agent)?;
        let (bytes, _sha) = download_registry_to(&zip_path, user_agent, false).await?;
        bytes
    };
    let (aircraft, acftref_n) = parse_registry_zip(&faa_bytes)?;
    tracing::info!(
        aircraft = aircraft.len(),
        acftref = acftref_n,
        "parsed FAA registry"
    );
    if aircraft.is_empty() {
        anyhow::bail!("FAA MASTER parsed 0 rows; refusing to refresh");
    }
    if !skip_download && aircraft.len() < MIN_PRODUCTION_MASTER_ROWS {
        anyhow::bail!(
            "FAA MASTER parsed {} rows (min {MIN_PRODUCTION_MASTER_ROWS}); refusing to refresh",
            aircraft.len()
        );
    }

    let mut companies = if let Some(p) = tickers_json {
        load_tickers(&p)?
    } else {
        let cached = cache_dir.join("company_tickers_exchange.json");
        if skip_download || (use_cache && cached.exists()) {
            if !cached.exists() {
                anyhow::bail!("tickers JSON missing");
            }
            load_tickers(&cached)?
        } else {
            require_network_user_agent(user_agent)?;
            let rows = download_tickers(user_agent).await?;
            std::fs::write(&cached, serde_json::to_vec(&simple_tickers_dump(&rows))?)?;
            rows
        }
    };
    tracing::info!(companies = companies.len(), "SEC ticker universe");

    if let Some(p) = addresses_json {
        let addrs = load_addresses_json(&p)?;
        apply_addresses(&mut companies, &addrs);
        tracing::info!(addresses = addrs.len(), "merged HQ addresses");
    } else {
        let p = cache_dir.join("sec_addresses.json");
        if p.exists() {
            let addrs = load_addresses_json(&p)?;
            apply_addresses(&mut companies, &addrs);
            tracing::info!(addresses = addrs.len(), "merged cached HQ addresses");
        }
    }

    let ex21_explicit = ex21.is_some();
    let subsidiaries = {
        let path = ex21.unwrap_or_else(|| cache_dir.join("ex21.parquet"));
        let csv_fallback = cache_dir.join("ex21.csv");
        if ex21_explicit {
            load_ex21(&path).with_context(|| format!("load Exhibit 21 {}", path.display()))?
        } else if skip_pudl {
            Vec::new()
        } else if skip_download || (use_cache && (path.exists() || csv_fallback.exists())) {
            if path.exists() {
                load_ex21(&path).with_context(|| format!("load Exhibit 21 {}", path.display()))?
            } else if csv_fallback.exists() {
                load_ex21(&csv_fallback)
                    .with_context(|| format!("load Exhibit 21 {}", csv_fallback.display()))?
            } else {
                anyhow::bail!(
                    "Exhibit 21 file missing at {} and --skip-download/--use-cache set",
                    path.display()
                );
            }
        } else {
            require_network_user_agent(user_agent)?;
            download_ex21_parquet(user_agent, &path)
                .await
                .with_context(|| "PUDL Exhibit 21 download")?;
            load_ex21(&path).with_context(|| format!("load Exhibit 21 {}", path.display()))?
        }
    };
    tracing::info!(subsidiaries = subsidiaries.len(), "Exhibit 21 aliases");
    if subsidiaries.is_empty() && !skip_pudl && !ex21_explicit {
        anyhow::bail!(
            "Exhibit 21 loaded 0 subsidiaries; refusing to refresh (would drop EX-21 mappings). Pass --skip-pudl for a name-only run."
        );
    }

    let edgar_path = edgar_jsonl.unwrap_or_else(|| {
        let p = PathBuf::from("evidence/edgar_hits.jsonl");
        if p.exists() {
            p
        } else {
            cache_dir.join("edgar_hits.jsonl")
        }
    });
    let edgar_hits = if edgar_path.exists() {
        load_edgar_jsonl(&edgar_path)?
    } else {
        Vec::new()
    };
    tracing::info!(edgar_hits = edgar_hits.len(), "EDGAR harvest hits");

    let ov = if overrides.exists() {
        load_overrides(&overrides)?
    } else {
        tracing::warn!(path = %overrides.display(), "overrides file missing");
        Default::default()
    };
    let unpublished = if gold.exists() {
        unpublished_by_n(&load_gold(&gold).with_context(|| format!("gold file {}", gold.display()))?)
    } else {
        tracing::warn!(path = %gold.display(), "gold file missing; unpublished-tail gate skipped");
        Default::default()
    };

    let faa_source = format!("faa:{}", zip_path.display());
    let mut out = resolve_all(
        &aircraft,
        &companies,
        &subsidiaries,
        &edgar_hits,
        &ov,
        &unpublished,
        &as_of,
        &faa_source,
        publish_address_cluster,
    );

    tracing::info!(
        published = out.published.len(),
        review = out.review_queue.len(),
        unresolved = out.unresolved.len(),
        conflicts = out.conflicts.len(),
        excluded = out.excluded.len(),
        "resolved"
    );

    let issuers = if aviation_issuers.exists() {
        load_aviation_issuers(&aviation_issuers)
            .with_context(|| format!("aviation issuers {}", aviation_issuers.display()))?
    } else {
        tracing::warn!(
            path = %aviation_issuers.display(),
            "aviation_issuers file missing; aviation_issuer will be false"
        );
        AviationIssuers::default()
    };
    annotate_aviation_issuer(&mut out.published, &issuers);

    let db_path = data_dir.join("current").join("tail_to_ticker.sqlite");
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

    if gold.exists() {
        let gold_file =
            load_gold(&gold).with_context(|| format!("gold file {}", gold.display()))?;
        let trusts = out
            .unresolved
            .iter()
            .filter(|u| u.reason == "trustee")
            .count();
        let report = evaluate_gold(&gold_file, &out.published, &aircraft, trusts);
        print!("{report}");
        if report.tail_false_positives > 0 {
            anyhow::bail!(
                "{} tail false positive(s): published as a forbidden ticker; not writing feed",
                report.tail_false_positives
            );
        }
    }

    let snap = data_dir.join("snapshots").join(&as_of);
    write_mappings_parquet(&snap.join("tail_to_ticker.parquet"), &out.published)?;
    write_mappings_csv(&snap.join("tail_to_ticker.csv"), &out.published)?;
    write_mappings_parquet(
        &data_dir.join("current").join("tail_to_ticker.parquet"),
        &out.published,
    )?;
    write_mappings_csv(
        &data_dir.join("current").join("tail_to_ticker.csv"),
        &out.published,
    )?;

    let mut db = open_db(&db_path)?;
    let log = apply_scd2(
        &mut db,
        &as_of,
        &out.published,
        &out.review_queue,
        &out.unresolved,
    )?;
    std::fs::copy(&db_path, snap.join("tail_to_ticker.sqlite"))?;

    let changelog_path = snap.join("changelog.jsonl");
    let mut buf = String::new();
    for e in &log {
        buf.push_str(&serde_json::to_string(e)?);
        buf.push('\n');
    }
    std::fs::write(changelog_path, buf)?;

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

fn write_json<T: serde::Serialize>(path: &Path, val: &T) -> Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(val)?)?;
    Ok(())
}

fn simple_tickers_dump(rows: &[sec_universe::Company]) -> serde_json::Value {
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
