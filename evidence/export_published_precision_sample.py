#!/usr/bin/env python3
"""Seed-43 simple-random-within-method sample of published mappings_current."""

from __future__ import annotations

import csv
import json
import random
import sqlite3
import zipfile
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DB = ROOT / "data/current/tail_to_ticker.sqlite"
ZIP = ROOT / "cache/ReleasableAircraft.zip"
TICKERS = ROOT / "cache/company_tickers_exchange.json"
OUT = ROOT / "evidence/published_precision_sample.csv"

SEED = 43
N_EXACT = 30
N_EX21 = 40
N_OVERRIDE = 10
N_EDGAR = 10


def load_exchange() -> dict[str, str]:
    if not TICKERS.exists():
        return {}
    data = json.loads(TICKERS.read_text())
    out = {}
    fields = [f.lower() for f in data["fields"]]
    ti = fields.index("ticker")
    ei = fields.index("exchange")
    for row in data["data"]:
        out[str(row[ti]).upper()] = str(row[ei]).upper()
    return out


def load_mappings() -> list[dict]:
    conn = sqlite3.connect(DB)
    conn.row_factory = sqlite3.Row
    rows = conn.execute(
        """
        SELECT n_number, ticker, cik, company_name, registrant_name, match_method,
               fleet_size, make, model
        FROM mappings_current
        """
    ).fetchall()
    conn.close()
    return [dict(r) for r in rows]


def take(rng: random.Random, rows: list[dict], n: int) -> list[dict]:
    rows = sorted(rows, key=lambda r: r["n_number"])
    rng.shuffle(rows)
    if n >= len(rows):
        return rows
    return rows[:n]


def pick_sample(rows: list[dict], exchange: dict[str, str]) -> list[dict]:
    rng = random.Random(SEED)
    for r in rows:
        r["exchange"] = exchange.get(r["ticker"], "")

    by_method: dict[str, list[dict]] = {}
    for r in rows:
        by_method.setdefault(r["match_method"], []).append(r)

    sample: list[dict] = []
    quotas = [
        ("manual_override", N_OVERRIDE),
        ("edgar_nnumber", N_EDGAR),
        ("exact_legal_name", N_EXACT),
        ("ex21_subsidiary", N_EX21),
    ]
    for method, quota in quotas:
        group = by_method.get(method, [])
        if not group:
            continue
        picked = take(rng, group, quota)
        for r in picked:
            r["stratum"] = method
        sample.extend(picked)
    sample.sort(key=lambda r: (r["stratum"], r["n_number"]))
    return sample


def join_master(sample: list[dict]) -> None:
    want = {r["n_number"].lstrip("N"): r for r in sample}
    with zipfile.ZipFile(ZIP) as zf:
        name = next(n for n in zf.namelist() if n.upper().endswith("MASTER.TXT"))
        with zf.open(name) as fh:
            raw = fh.read()
            if raw.startswith(b"\xef\xbb\xbf"):
                raw = raw[3:]
            text = raw.decode("latin-1", errors="replace")
    reader = csv.DictReader(text.splitlines())
    for rec in reader:
        rec = {(k or "").lstrip("\ufeff").strip(): (v or "").strip() for k, v in rec.items()}
        raw_n = rec.get("N-NUMBER", "")
        key = raw_n.upper().lstrip("N")
        row = want.get(key)
        if not row:
            continue
        row["faa_city"] = rec.get("CITY", "")
        row["faa_state"] = rec.get("STATE", "")
        row["faa_street"] = rec.get("STREET", "")


def main() -> None:
    exchange = load_exchange()
    rows = load_mappings()
    print("population", Counter(r["match_method"] for r in rows))
    sample = pick_sample(rows, exchange)
    join_master(sample)
    fields = [
        "stratum",
        "n_number",
        "ticker",
        "cik",
        "company_name",
        "registrant_name",
        "match_method",
        "fleet_size",
        "exchange",
        "make",
        "model",
        "faa_city",
        "faa_state",
        "faa_street",
    ]
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with OUT.open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields, extrasaction="ignore")
        w.writeheader()
        w.writerows(sample)
    print(f"wrote {OUT} rows={len(sample)}")
    print(Counter(r["stratum"] for r in sample))
    missing = [r["n_number"] for r in sample if not r.get("faa_city")]
    if missing:
        print("missing MASTER city", missing)


if __name__ == "__main__":
    main()
