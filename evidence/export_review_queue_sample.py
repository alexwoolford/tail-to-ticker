#!/usr/bin/env python3
"""Seed-43 stratified sample of review_queue (ex21 + address_cluster)."""

from __future__ import annotations

import csv
import random
import sqlite3
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DB = ROOT / "data/current/tail_to_ticker.sqlite"
OUT = ROOT / "evidence/review_queue_sample.csv"

SEED = 43
N_EX21 = 50
N_ADDR = 15
N_EXACT = 5


def take(rng: random.Random, rows: list[dict], n: int) -> list[dict]:
    rows = sorted(rows, key=lambda r: r["n_number"])
    rng.shuffle(rows)
    if n >= len(rows):
        return rows
    return rows[:n]


def main() -> None:
    conn = sqlite3.connect(DB)
    conn.row_factory = sqlite3.Row
    rows = [dict(r) for r in conn.execute("SELECT * FROM review_queue")]
    conn.close()
    by: dict[str, list[dict]] = {}
    for r in rows:
        by.setdefault(r["match_method"], []).append(r)
    rng = random.Random(SEED)
    sample: list[dict] = []
    for method, quota in [
        ("ex21_subsidiary", N_EX21),
        ("address_cluster", N_ADDR),
        ("exact_legal_name", N_EXACT),
    ]:
        picked = take(rng, by.get(method, []), quota)
        for r in picked:
            r["stratum"] = method
        sample.extend(picked)
    sample.sort(key=lambda r: (r["stratum"], r["n_number"]))
    fields = [
        "stratum",
        "n_number",
        "ticker",
        "cik",
        "company_name",
        "registrant_name",
        "match_method",
        "make",
        "model",
        "source_url",
    ]
    with OUT.open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields, extrasaction="ignore")
        w.writeheader()
        w.writerows(sample)
    print(f"wrote {OUT} rows={len(sample)}")
    print("population", Counter(r["match_method"] for r in rows))
    print("sample", Counter(r["stratum"] for r in sample))


if __name__ == "__main__":
    main()
