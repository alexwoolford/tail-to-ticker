use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use faa_ingest::{load_current_aircraft, parse_registry_zip, DEFAULT_PUBLISHED_DB};
use resolve::{
    evaluate_gold, evaluate_rubric, is_utc_date, load_gold, load_rubric, lookup, open_db,
};

mod refresh;

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
        default_value = "tail-to-ticker contact@example.com"
    )]
    user_agent: String,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Download sources, resolve, and write SQLite snapshots.
    Refresh {
        #[arg(long)]
        as_of: Option<String>,
        #[arg(long)]
        faa_zip: Option<PathBuf>,
        /// Published faa-registry-mirror sqlite. Default: FAA_REGISTRY_DB or host current/.
        #[arg(long, env = "FAA_REGISTRY_DB")]
        faa_db: Option<PathBuf>,
        #[arg(long)]
        tickers_json: Option<PathBuf>,
        #[arg(long)]
        ex21: Option<PathBuf>,
        #[arg(long)]
        addresses_json: Option<PathBuf>,
        /// Explicit EDGAR JSONL. Default is `edgar_allowlist.jsonl` next to `--overrides`.
        /// Does not load `evidence/edgar_hits.jsonl`.
        #[arg(long)]
        edgar_jsonl: Option<PathBuf>,
        #[arg(long, default_value = "overrides/mappings.yaml")]
        overrides: PathBuf,
        #[arg(long)]
        skip_download: bool,
        /// Reuse cached SEC tickers / EX-21 if present (no freshness).
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
        #[arg(long, default_value = "overrides/issuer_aliases.yaml")]
        issuer_aliases: PathBuf,
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
    /// Precision/recall against overrides/gold.yaml (production gate) and
    /// optional eval-only rubric holdout by stratum.
    Eval {
        #[arg(long, default_value = "overrides/gold.yaml")]
        gold: PathBuf,
        #[arg(long, default_value = "overrides/rubric.yaml")]
        rubric: PathBuf,
        #[arg(long)]
        faa_zip: Option<PathBuf>,
        #[arg(long, env = "FAA_REGISTRY_DB")]
        faa_db: Option<PathBuf>,
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
            faa_db,
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
            issuer_aliases,
        } => {
            refresh::run(refresh::RefreshOpts {
                data_dir: cli.data_dir,
                cache_dir: cli.cache_dir,
                user_agent: cli.user_agent,
                as_of,
                faa_zip,
                faa_db,
                tickers_json,
                ex21,
                addresses_json,
                edgar_jsonl,
                overrides,
                gold,
                aviation_issuers,
                issuer_aliases,
                skip_download,
                use_cache,
                skip_pudl,
                publish_address_cluster,
            })
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
            if !is_utc_date(&date) {
                anyhow::bail!("--as-of must be UTC calendar day YYYY-MM-DD, got {date:?}");
            }
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
        Commands::Eval {
            gold,
            rubric,
            faa_zip,
            faa_db,
            db,
        } => {
            let gold = load_gold(&gold).with_context(|| format!("gold file {}", gold.display()))?;
            let db_path =
                db.unwrap_or_else(|| cli.data_dir.join("current").join("tail_to_ticker.sqlite"));
            let feed = open_db(&db_path)?;
            let published = feed.current_mappings()?;
            let trusts = feed.unresolved_trust_count()?;
            let aircraft = load_eval_aircraft(faa_db, faa_zip, &cli.cache_dir)?;
            let report = evaluate_gold(&gold, &published, &aircraft, trusts as usize);
            print!("{report}");
            if rubric.exists() {
                let rubric = load_rubric(&rubric)
                    .with_context(|| format!("rubric file {}", rubric.display()))?;
                let rr = evaluate_rubric(&rubric, &published, &aircraft);
                print!("{rr}");
            }
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

fn load_eval_aircraft(
    faa_db: Option<PathBuf>,
    faa_zip: Option<PathBuf>,
    cache_dir: &std::path::Path,
) -> Result<Vec<faa_ingest::Aircraft>> {
    if let Some(zip) = faa_zip {
        return Ok(parse_registry_zip(&std::fs::read(&zip)?)?.0);
    }
    let db = faa_db.unwrap_or_else(|| {
        let host = PathBuf::from(DEFAULT_PUBLISHED_DB);
        if host.exists() {
            host
        } else {
            cache_dir.join("faa-registry.sqlite")
        }
    });
    if db.exists() {
        return Ok(load_current_aircraft(&db)?.0);
    }
    Ok(Vec::new())
}
