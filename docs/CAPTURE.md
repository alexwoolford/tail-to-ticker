# Capture contract (work sqlite)

Decision: **capture the published N-number → ticker trickle, not the review queue, trustee dump, harvest JSONL, or a collector snapshot of this database.** Work sqlite is `--data-dir/current/tail_to_ticker.sqlite` (prod `/var/lib/tail-to-ticker/work/current/tail_to_ticker.sqlite`). Logical name `tail-to-ticker`. The collector watches work sqlite only.

Canonical contract: [capturable-state design principles](https://github.com/alexwoolford/capturable-state/blob/main/docs/design-principles.md) §0 / §7 and [datetime.md](https://github.com/alexwoolford/capturable-state/blob/main/docs/datetime.md). Capture the trickle, not the hose.

Pin: `capturable-state` git tag `v0.1.1` (not a path dep; do not copy `src/*.rs`).

Published `/var/lib/tail-to-ticker/current/tail_to_ticker.sqlite` is a sibling API for adsb-trip-journal (`VACUUM INTO` / `mv`). Do not watch it — that copy duplicates `_outbox`.

## What is captured

| Table / stream | Capture? | Mode | Why |
| --- | --- | --- | --- |
| `mappings_current` | **yes** | full, exclude `fleet_size` / `aviation_issuer` | Product. Key `n_number`. Derived filters stay local so quiet days do not hose `U` |
| `refresh_run` | **yes** | after | Did last night finish? Key `as_of_date` |
| `changelog` | **yes** | after | Dropped / new / updated tails that day |
| `review_queue` | **no** | — | DELETE+reload each refresh |
| `unresolved_trusts` | **no** | — | DELETE+reload each refresh |
| Harvest JSONL / `evidence/` | **no** | — | Sidecar; resolver still requires MASTER |
| SEC ticker JSON / PUDL parquet | **no** | — | Cache, not facts |
| `_outbox` | platform | — | Generated |

Identity: `n_number` (leading `N`). Live rows have `deleted_at IS NULL`. Tails that leave the published set keep the row with `deleted_at = CAST(strftime('%s','now') AS INTEGER)` (no `DELETE`). Returning tails upsert and set `deleted_at=NULL`. Quiet identity (same ticker / method / names) does not rewrite the row.

`cik` stays TEXT (leading zeros). `icao24` is lowercase hex TEXT. `fleet_size` / `aviation_issuer` are INTEGER in sqlite and **not** captured. These are **labels**, not a pierce score.

Do not `collect --snapshot` this database.

Python `edgar_harvest` writes JSONL only. It is not on `tail-to-ticker-refresh.timer`. Refresh loads tracked `overrides/edgar_allowlist.jsonl`, not `evidence/edgar_hits.jsonl`.

## Clocks

| Layer | Columns | Type |
| --- | --- | --- |
| Facts | `as_of_date` on mappings / changelog / `refresh_run` | TEXT `YYYY-MM-DD` |
| Facts | `refresh_run.recorded_at` | TEXT `YYYY-MM-DDTHH:MM:SSZ` via `utc_iso` |
| Envelope | `_outbox.ts`, `deleted_at` | INTEGER Unix seconds |

Order outbox by `seq`, not `ts`.

## Announce / nudge

`install()` on work sqlite. Announce file stem equals `db_name` `tail-to-ticker`. `ReadWritePaths` include `/var/lib/state-capture/announce` (required when the collector is present) and `-/run/state`. `tails` must be in group `state-capture`. Collector host inventory lives in mosaic `deploy/ct-firehose/`, not in this crate.
