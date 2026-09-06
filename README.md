# tail-to-ticker

A daily-refreshable feed that maps **U.S. N-numbers** to **U.S. listed tickers**, with a match method and a source URL on every row.

There is no maintained open tail→ticker table. This repo does not scrape FlightAware or rebuild ADS-B tracking. It joins public FAA registrations to the SEC ticker universe (and optional Exhibit 21 subsidiaries / EDGAR N-number hits).

The feed does **not** emit a probability. `eval` reports unpublished-tail false positives and tail recall against [`overrides/gold.yaml`](overrides/gold.yaml); those counts are not a calibrated confidence score. `refresh` applies `unpublished_tails` as a suppress gate and refuses to write the feed if any of those N-numbers would still publish as `must_not_ticker`.

This is a **public FAA registrant → listed ticker** join. It is not beneficial-owner, operator, or “whose jet is this” intelligence. Bank trusts (~3k tails) are parked, not pierced.

## What you get

After `tail-to-ticker refresh`:

| Path | Contents |
|---|---|
| `data/current/tail_to_ticker.sqlite` | `mappings_current`, changelog, unresolved trusts, review queue, `refresh_run` |
| `data/snapshots/YYYY-MM-DD/` | Dated sqlite copy plus `changelog.jsonl` |

`context/`, `data/`, and `cache/` are local-only and gitignored (directory placeholders stay). Do not commit feeds, FAA/SEC downloads, or private notes.

Columns: `n_number`, `icao24`, `serial`, `make`, `model`, `ticker`, `cik`, `company_name`, `registrant_name`, `match_method`, `as_of_date`, `source_url`, `fleet_size`, `aviation_issuer`.

`aviation_issuer` is 1 when the FAA registrant **is** the listed aviation business (OEM, defense airframer, helicopter operator, lessor). It is 0 for ordinary parents / flight departments. List: [`overrides/aviation_issuers.yaml`](overrides/aviation_issuers.yaml). Typical consumer filter:

```sql
SELECT * FROM mappings_current
WHERE aviation_issuer = 0 AND fleet_size BETWEEN 1 AND 6;
```

Match methods (highest wins; conflicts are not published):

| Method | Published by default? |
|---|---|
| `manual_override` | yes |
| `edgar_nnumber` | yes (N-number must exist in FAA MASTER) |
| `exact_legal_name` | yes, when corroborated (ticker, parent brand token, or N-number SPV) |
| `ex21_subsidiary` | yes, when corroborated the same way |
| `address_cluster` | review queue only (`--publish-address-cluster` to include) |

Exact-name and Exhibit 21 hits that match a unique key but are not corroborated (namesake cores such as REACH / LEAR HOLDING) land on the review queue.

Bank trustees, fractionals (NetJets, Flexjet, …), and Part 121 airline fleets are excluded. Trustee bizjets land in `unresolved_trusts` for a later veil-piercing pass.

When one CIK has many SEC tickers (common + preferreds), the feed emits the **primary common share** (unhyphenated, major exchange), not `AUB-PA` / `JPM-PM` / `FCNCP`.

## Known limitations

- **Registrant, not owner.** The FAA name on the airframe is the join key. Delaware trusts, LLC SPVs, and lessors are usually not the listed issuer. Do not treat a row as “Walmart’s jet” without reading `registrant_name` and `match_method`.
- **OEM and aviation issuers are in the table.** Textron/Bell, Boeing, Northrop, Garmin, Bristow, GE Aviation, aircraft lessors match because they *are* the registrant. Filter `aviation_issuer = 0` (and optionally `fleet_size BETWEEN 1 AND 6`) for flight departments.
- **Low recall.** Hundreds of published rows vs hundreds of thousands of MASTER records. Coverage is the corroborated public-name slice, not a census of corporate aviation.
- **Host feed vs GitHub artifacts.** Production for adsb-trip-journal is the host systemd timer on ct-firehose, which atomically publishes `/var/lib/tail-to-ticker/current/tail_to_ticker.sqlite`. [`.github/workflows/refresh.yml`](.github/workflows/refresh.yml) (cron 07:00 UTC + `workflow_dispatch`) is an **optional artifact backup** (sqlite + changelog retained 14 days). It is not the consumer path and does not replace the host timer. Local `refresh` **re-downloads** FAA, SEC tickers, and PUDL Exhibit 21 unless you pass `--use-cache` or `--skip-download`. Set repo secret / env `SEC_USER_AGENT` to a real contact; placeholder `example.com` is rejected on network fetches.
- **Refresh fail-closed.** Empty/truncated MASTER (production runs require ≥300k parsed rows; `--skip-download` skips that floor), empty Exhibit 21 (unless `--skip-pudl` or an explicit `--ex21` fixture), published-row collapse vs the previous table (below max(50, N/2)), or gold unpublished-tail false positives abort before overwrite.
- **Precision is a labeled sample, not a score.** After a local refresh, `evidence/export_published_precision_sample.py` and `evidence/write_published_precision_verdicts.py` write a seed-43 sample and verdicts (n=70) under `evidence/`. Those CSVs are gitignored; they are not part of the repo. SQLite does not store a `confidence` column.
- **Not investment advice.** Code is MIT; FAA data is public domain; PUDL Exhibit 21 is CC-BY-4.0 (cite Catalyst Cooperative). Redistributing a derived feed should keep that citation.

## Build

```bash
cargo build --release
# real descriptive contact; see .env.example (do not use example.com)
export SEC_USER_AGENT="Your Name email@domain"
./target/release/tail-to-ticker refresh
# local iteration without re-download:
./target/release/tail-to-ticker refresh --use-cache
./target/release/tail-to-ticker lookup N1WM
./target/release/tail-to-ticker eval --gold overrides/gold.yaml
```

Offline / fixture refresh:

```bash
cargo run -p tail-to-ticker -- refresh \
  --skip-download --skip-pudl \
  --faa-zip /path/to/ReleasableAircraft.zip \
  --tickers-json tests/fixtures/sec/company_tickers_exchange.json \
  --ex21 tests/fixtures/sec/ex21.csv \
  --edgar-jsonl tests/fixtures/edgar_hits.jsonl
```

FAA zip: `https://registry.faa.gov/database/ReleasableAircraft.zip` (nightly ~05:30 UTC).  
SEC tickers: `https://www.sec.gov/files/company_tickers_exchange.json` (requires a descriptive User-Agent).

## EDGAR harvest (Python sidecar)

```bash
cd python/edgar_harvest
python -m edgar_harvest --output ../../evidence/edgar_hits.jsonl
```

Optional `pip install edgartools` for cleaner proxy text. The Rust resolver ignores harvest hits whose N-number is not in MASTER.

## Prior art this reuses

- FAA Releasable Aircraft Database (public domain)
- SEC `company_tickers_exchange.json`
- [PUDL](https://catalyst.coop/pudl/) Exhibit 21 subsidiaries (`out_sec10k__parents_and_subsidiaries`, CC-BY-4.0 — cite Catalyst Cooperative)
- EDGAR full-text search (`efts.sec.gov`)

Not reused as a feed (proprietary or not redistributable): JETNET Jettrack, ch-aviation, AMSTAT, AeroPattern.

## Production (Linux)

Do **not** install these units on a Mac. Production is **git clone / rsync + [`deploy/install.sh`](deploy/install.sh) + systemd**, same `/opt` + `/var/lib` split as adsb-trip-journal. Layout and timer: [`docs/DAILY_OPS.md`](docs/DAILY_OPS.md).

```bash
cargo build --release
sudo ./deploy/install.sh
```

Refresh is a daily **host** timer at **07:00 UTC** (FAA zip is ~05:30 UTC). That published file is what adsb-trip-journal reads. GitHub Actions `refresh.yml` is an optional artifact backup, not the consumer path. Set `SEC_USER_AGENT` in `/opt/tail-to-ticker/etc/tail-to-ticker.env` (chmod 600) to a real contact; placeholder `example.com` is rejected. The published feed is `/var/lib/tail-to-ticker/current/tail_to_ticker.sqlite` (atomic replace after a successful refresh). Tracking should consume that file, not generate mappings.

## v2 (not this repo’s default path)

Trust piercing (FCC ULS, UCC-1, FOIA Declaration of Trust) and ADS-B tracking. Tracking should consume this feed, not generate it.
