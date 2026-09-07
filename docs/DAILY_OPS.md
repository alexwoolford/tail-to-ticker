# tail-to-ticker (ops)

## Product

Daily-refreshable **FAA registrant → listed ticker** feed. Not operator, beneficial owner, or “whose jet is this.” Bank trusts stay parked.

The sibling **adsb-trip-journal** reads the published sqlite **read-only** (`icao24` join). This job must not write `trips.sqlite` or import OpenSky / ADS-B keys.

## Cadence

| UTC | Job |
|---|---|
| ~05:30 | FAA Releasable Aircraft zip |
| 06:00 + ≤15m | `adsb-trip-journal-collect` — yesterday’s `seen_airborne` |
| **07:00 + ≤15m** | **`tail-to-ticker-refresh`** — force-fresh FAA / SEC / PUDL, atomic publish |
| every 10 min | `adsb-trip-journal-watch` — re-opens mapping RO |

A tail that appears in the 07:00 map is eligible for watch the same UTC day and for collect the **next** morning. Do not move refresh to 06:00.

## Deploy (systemd)

**Prefer rsync or git clone + [`deploy/install.sh`](../deploy/install.sh) + systemd.** Same `/opt` + `/var/lib` split as adsb-trip-journal on this host. Build **on the box** (`aarch64-unknown-linux-gnu`); do not copy a Mac binary. Do not run production from `$HOME` with cron. Do not collide with Docker **ct-firehose-filter**. GitHub Actions `refresh.yml` is an optional artifact backup; the journal reads the host-published sqlite, not those artifacts.

Host prerequisites: outbound HTTPS to `registry.faa.gov`, `www.sec.gov`, and PUDL Exhibit 21; Rust toolchain (or a prebuilt aarch64 binary); `SEC_USER_AGENT` with a real contact (not `example.com`).

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

Default `refresh` **re-downloads** sources. Do not pass `--use-cache` on the timer. Fail-closed gates (MASTER ≥300k, nonempty EX-21, published-row floor, gold unpublished tails) abort before the atomic publish; `/var/lib/tail-to-ticker/current/` stays the previous good file.

## FAA zip and User-Agents

- Zip: `https://registry.faa.gov/database/ReleasableAircraft.zip` (~70 MB) — `MASTER.txt` + `ACFTREF.txt`. Nightly ~05:30 UTC. No per-tail API.
- `FAA_USER_AGENT` (Safari-like default) is for `registry.faa.gov` only; `SEC_USER_AGENT` is the SEC fair-access contact. Do not send the FAA token to SEC.
- This OCI IP 403s SEC company-tickers JSON; refresh falls back to `cache/company_tickers_exchange.json`. FAA stays fail-closed.
- Do not set `HTTPS_PROXY` on the tails unit (IPRoyal CONNECT-403s `.gov`). Never put a proxy on adsb-trip-journal.

See [`deploy/tail-to-ticker.env.example`](../deploy/tail-to-ticker.env.example).

## Layout

```
/opt/tail-to-ticker/
  bin/tail-to-ticker
  scripts/run-refresh.sh
  docs/DAILY_OPS.md
  overrides/{mappings,gold,aviation_issuers}.yaml
  etc/tail-to-ticker.env
/var/lib/tail-to-ticker/
  work/current/                 # in-place sqlite while the job runs
  work/snapshots/YYYY-MM-DD/
  cache/                        # FAA zip, SEC tickers, PUDL parquet
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

Env (optional until the collector exists; missing socket is ignored):

```
STATE_CAPTURE_SOCK=/run/state/collect.sock
STATE_CAPTURE_ANNOUNCE_DIR=/var/lib/state-capture/announce
```

If the announce dir cannot be created, `open_db()` writes `{sqlite_dir}/.capturable.json`.
