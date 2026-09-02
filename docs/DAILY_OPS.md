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

Default `refresh` **re-downloads** sources. Do not pass `--use-cache` on the timer. Fail-closed gates (MASTER ≥50k, nonempty EX-21, published-row floor, gold unpublished tails) abort before the atomic publish; `/var/lib/tail-to-ticker/current/` stays the previous good file.

## FAA zip, Akamai, and proxy

The FAA does not offer a per-tail API. Refresh pulls **`https://registry.faa.gov/database/ReleasableAircraft.zip`** (~60–70 MB): comma-delimited `MASTER.txt` (N-number, registrant, Mode S / `icao24`) and `ACFTREF.txt` (make/model). Nightly ~05:30 UTC.

Akamai in front of that zip returns **403 `AkamaiGHost`** for the SEC contact `User-Agent` even with `Accept: */*` and `Accept-Language: en-US` (same 403 from this OCI IP and from a residential laptop). Origin (Microsoft-IIS) serves the file when `FAA_USER_AGENT` is a normal browser token — not Chrome; the binary default is Safari-like and is **not** sent to SEC. SEC fair-access still uses `SEC_USER_AGENT`. `www.sec.gov` company-tickers JSON is **403 from this OCI IP** with that contact string (200 from a residential path). The existing IPRoyal HTTP(S) proxy **CONNECT-403s `faa.gov` and `sec.gov`**, so do **not** set `HTTPS_PROXY` on the tails unit for that product (and never put it on adsb-trip-journal). Reqwest honors `HTTPS_PROXY`/`NO_PROXY` if a future proxy allows `.gov`; PUDL Exhibit 21 stays **direct S3** (`no_proxy`). On SEC 403, refresh may reuse `cache/company_tickers_exchange.json` if present; FAA stays fail-closed. HTTP errors log `Server` (e.g. AkamaiGHost). 503s from origin are retried with backoff.

## Layout

```
/opt/tail-to-ticker/
  bin/tail-to-ticker
  scripts/run-refresh.sh
  docs/DAILY_OPS.md
  overrides/{mappings,gold,aviation_issuers}.yaml
  etc/tail-to-ticker.env
/var/lib/tail-to-ticker/
  work/current/                 # in-place SCD2 while the job runs
  work/snapshots/YYYY-MM-DD/
  cache/                        # FAA zip, SEC tickers, PUDL parquet
  current/tail_to_ticker.sqlite # PUBLISHED — journal reads this (mode 644)
```

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
   WHERE aviation_issuer = 0 AND fleet_size BETWEEN 1 AND 6
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
