use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use faa_ingest::{parse_registry_zip, FAA_DOWNLOAD_USER_AGENT};
use resolve::{evaluate_gold, is_utc_date, load_gold, lookup, open_db};

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
        default_value = "tail-to-ticker/0.1 (https://github.com/alexwoolford/tail-to-ticker; contact@example.com)"
    )]
    user_agent: String,
    /// User-Agent for `registry.faa.gov` only. Akamai 403s the SEC contact string.
    #[arg(long, global = true, env = "FAA_USER_AGENT")]
    faa_user_agent: Option<String>,
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
            refresh::run(refresh::RefreshOpts {
                data_dir: cli.data_dir,
                cache_dir: cli.cache_dir,
                user_agent: cli.user_agent,
                faa_user_agent: cli
                    .faa_user_agent
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(FAA_DOWNLOAD_USER_AGENT)
                    .to_string(),
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
