# tail-to-ticker (ops)

## Product

Daily-refreshable **FAA registrant → listed ticker** feed. Not operator, beneficial owner, or “whose jet is this.” Bank trusts stay parked.

The sibling **adsb-trip-journal** reads the published sqlite **read-only** (`icao24` join). This job must not write `trips.sqlite` or import OpenSky / ADS-B keys.

## Scheduler and telemetry

`tail-to-ticker-refresh.timer` starts a `Type=oneshot` service. Do not add an in-process cron.

Operator logs: `tracing` on stderr → journald (`SyslogIdentifier` matches the unit). Default `RUST_LOG=info`.

`refresh_run` is capturable domain telemetry. Query it in mosaic; do not scrape Prometheus from this oneshot.

## Cadence

| UTC | Job |
|---|---|
| 05:45 | `faa-registry-mirror` — publish current MASTER sqlite |
| 06:00 + ≤15m | `adsb-trip-journal-collect` — yesterday’s `seen_airborne` |
| **07:00 + ≤15m** | **`tail-to-ticker-refresh`** — FAA published sqlite + fresh SEC / PUDL, atomic publish |
| every 10 min | `adsb-trip-journal-watch` — re-opens mapping RO |

A tail that appears in the 07:00 map is eligible for watch the same UTC day and for collect the **next** morning. Do not move refresh to 06:00.

## Deploy (systemd)

**Prefer rsync or git clone + [`deploy/install.sh`](../deploy/install.sh) + systemd.** Same `/opt` + `/var/lib` split as adsb-trip-journal on this host. Build **on the box** (`aarch64-unknown-linux-gnu`); do not copy a Mac binary. Do not run production from `$HOME` with cron. Do not collide with Docker **ct-firehose-filter**. GitHub Actions `refresh.yml` does not run the feed (no host FAA sqlite). The journal reads the host-published file.

Host prerequisites: outbound HTTPS to `www.sec.gov` and PUDL Exhibit 21; read access to `/var/lib/faa-registry-mirror/current/faa-registry.sqlite`; Rust toolchain (or a prebuilt aarch64 binary); `SEC_USER_AGENT` with a real contact (not `example.com`).

```bash
cargo build --release
# optional: TAIL_ENV_FILE=/path/tail-to-ticker.env  (chmod 600, SEC_USER_AGENT filled)
sudo ./deploy/install.sh
```

Units in [`deploy/systemd/`](../deploy/systemd/):

| Unit | Schedule |
|---|---|
| `tail-to-ticker-refresh.timer` | daily 07:00 UTC (`Persistent=true`, 15m jitter) |
| `tail-to-ticker-refresh.service` | oneshot, invoked by the timer |

Config: `/opt/tail-to-ticker/etc/tail-to-ticker.env` (from [`deploy/tail-to-ticker.env.example`](../deploy/tail-to-ticker.env.example), **chmod 600**). Install does not overwrite an existing env file. The timer is enabled only when `SEC_USER_AGENT` is set and does not contain `example.com`.

Default `refresh` re-downloads SEC tickers and PUDL. It reads the FAA published sqlite; it does not pass `--use-cache` on the timer. Fail-closed gates (MASTER ≥300k, nonempty EX-21, published-row floor, gold unpublished tails) abort before the atomic publish; `/var/lib/tail-to-ticker/current/` stays the previous good file.

## FAA zip and User-Agents

## FAA registry (published sqlite)

- **Do not** GET `ReleasableAircraft.zip` from this job. `faa-registry-mirror` ingests at 05:45 UTC and publishes `/var/lib/faa-registry-mirror/current/faa-registry.sqlite` (mode 644).
- Set `FAA_REGISTRY_DB` to that path (see env.example). `--faa-zip` is fixtures only (CSV parse; commas inside names).
- `SEC_USER_AGENT` is the SEC fair-access contact. Do not send an FAA Safari token to SEC.
- SEC requires a declared User-Agent in the SEC sample shape (`tail-to-ticker you@domain`, not the github-paren form) and a maximum of **10 requests/second** per IP. That ceiling is not a target. Harvest details: [`python/edgar_harvest/README.md`](../python/edgar_harvest/README.md) and [`scripts/edgar-fair-access-check.sh`](../scripts/edgar-fair-access-check.sh). Harvest stays off this timer because it is a **labeled oneshot** (tracked [`overrides/edgar_allowlist.jsonl`](../overrides/edgar_allowlist.jsonl)), not because egress is blocked. Do not add it as a daily job.
- **SEC cache age:** `stat /var/lib/tail-to-ticker/cache/company_tickers_exchange.json`. After 2026-09-08 the sample-shaped UA **200s from this OCI IP**; a github-paren UA 403s as undeclared (`AkamaiGHost`). Do not treat a successful refresh as “SEC was live” when the log says it used cache.
- Do not set `HTTPS_PROXY` on the tails unit (IPRoyal CONNECT-403s `.gov`). Never put a proxy on adsb-trip-journal.

See [`deploy/tail-to-ticker.env.example`](../deploy/tail-to-ticker.env.example).

## Layout

```
/opt/tail-to-ticker/
  bin/tail-to-ticker
  scripts/run-refresh.sh
  docs/DAILY_OPS.md
  overrides/{mappings,gold,aviation_issuers,issuer_aliases}.yaml
  overrides/edgar_allowlist.jsonl
  etc/tail-to-ticker.env
/var/lib/tail-to-ticker/
  work/current/                 # in-place sqlite while the job runs
  work/snapshots/YYYY-MM-DD/
  cache/                        # SEC tickers, PUDL parquet
  current/tail_to_ticker.sqlite # PUBLISHED — journal reads this (mode 644)
```

Sqlite has parent table `refresh_run(as_of_date, recorded_at)` for the write instant (`YYYY-MM-DDTHH:MM:SSZ`); mapping/changelog columns stay UTC calendar days. Dropped tails stay in `mappings_current` with `deleted_at` (Unix seconds); live reads use `deleted_at IS NULL`.

Upgrades: pull/rsync → `cargo build --release` → `sudo ./deploy/install.sh` (env preserved).

## Verify

```bash
systemctl is-enabled tail-to-ticker-refresh.timer
systemctl list-timers 'tail-to-ticker-*'
journalctl -u tail-to-ticker-refresh.service -n 80 --no-pager

sudo -u adsb test -r /var/lib/tail-to-ticker/current/tail_to_ticker.sqlite

sudo -u tails /opt/tail-to-ticker/bin/tail-to-ticker \
  --data-dir /var/lib/tail-to-ticker/work \
  lookup N1WM \
  --db /var/lib/tail-to-ticker/current/tail_to_ticker.sqlite

sqlite3 /var/lib/tail-to-ticker/current/tail_to_ticker.sqlite \
  "SELECT count(*) FROM mappings_current
   WHERE deleted_at IS NULL
     AND aviation_issuer = 0 AND fleet_size BETWEEN 1 AND 6
     AND icao24 IS NOT NULL AND trim(icao24) <> '';"
```

## Timer failed

`Persistent=true` will retry after a reboot. It will not page you.

1. `systemctl is-failed tail-to-ticker-refresh.service` and `systemctl list-timers 'tail-to-ticker-*'`.
2. `journalctl -u tail-to-ticker-refresh.service -n 80 --no-pager`. FAA 403 aborts before publish (`current/` stays). SEC 403 should fall back to cache — then check cache age (above).
3. Confirm `SEC_USER_AGENT` is set and not `example.com` (`install.sh` will not enable the timer otherwise).
4. Leave `current/` alone. Re-run: `sudo systemctl start tail-to-ticker-refresh.service`.
5. If a fail-closed gate tripped (MASTER rows, EX-21, gold unpublished), fix the source or overrides and re-run; do not `--use-cache` on the timer.

Manual refresh (after `SEC_USER_AGENT` is set):

```bash
sudo systemctl start tail-to-ticker-refresh.service
```

Journal env (consumer, not this unit):

```
TAIL_TO_TICKER_SQLITE=/var/lib/tail-to-ticker/current/tail_to_ticker.sqlite
```

Then `systemctl restart adsb-trip-journal-watch.service`.

## State capture (prep)

Logical name: `tail-to-ticker`. Watch the **work** sqlite the refresh job writes, not the published `current/` copy (`VACUUM INTO` / `mv` duplicates `_outbox`).

| Path | Role |
|---|---|
| `/var/lib/tail-to-ticker/work/current/tail_to_ticker.sqlite` | Watched. `_outbox` + triggers. |
| `/var/lib/tail-to-ticker/current/tail_to_ticker.sqlite` | Published snapshot for the journal. Do not watch. |

Capture set: `mappings_current` (full; exclude derived `fleet_size` / `aviation_issuer`), `refresh_run` (after), `changelog` (after). `review_queue` and `unresolved_trusts` are DELETE+reload and are **not** captured.

Outbox/triggers come from [`capturable-state`](https://github.com/alexwoolford/capturable-state) `v0.1.0`, not a copied `capture.rs`.

Env (collector is `state-capture` on this host; missing socket is ignored):

```
STATE_CAPTURE_SOCK=/run/state/collect.sock
STATE_CAPTURE_ANNOUNCE_DIR=/var/lib/state-capture/announce
```

If the announce dir cannot be created, `open_db()` writes `{sqlite_dir}/.capturable.json`.
