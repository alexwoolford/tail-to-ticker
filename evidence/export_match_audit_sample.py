#!/usr/bin/env python3
"""Seed-42 stratified sample of mappings_current for the match-quality audit."""

from __future__ import annotations

import csv
import json
import random
import sqlite3
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DB = ROOT / "data/current/tail_to_ticker.sqlite"
ZIP = ROOT / "cache/ReleasableAircraft.zip"
TICKERS = ROOT / "cache/company_tickers_exchange.json"
OUT = ROOT / "evidence/match_audit_sample.csv"

DROP_TOKENS = {
    "AVIATION",
    "AIRCRAFT",
    "AIRPLANE",
    "AIR",
    "AERO",
    "LEASING",
    "LEASE",
    "SERVICES",
    "SERVICE",
    "SALES",
}
SUFFIXES = [
    "INCORPORATED",
    "CORPORATION",
    "COMPANY",
    "LIMITED",
    "PARTNERS",
    "PARTNERSHIP",
    "HOLDINGS",
    "HOLDING",
    "GROUP",
    "TRUSTEE",
    "TRUST",
    "LLC",
    "L L C",
    "LLP",
    "LP",
    "PLC",
    "PC",
    "PA",
    "NA",
    "N A",
    "INC",
    "CORP",
    "CO",
    "LTD",
    "THE",
]


def normalize_name(raw: str) -> str:
    upper = raw.upper().replace("&", " AND ").replace("+", " AND ")
    s = []
    prev_space = False
    for ch in upper:
        if ch.isalnum():
            s.append(ch)
            prev_space = False
        elif not prev_space:
            s.append(" ")
            prev_space = True
    s = "".join(s).strip()
    while True:
        before = s
        if s.startswith("THE "):
            s = s[4:]
        for suf in SUFFIXES:
            pad = f" {suf}"
            if s.endswith(pad):
                s = s[: -len(pad)].rstrip()
            if s == suf:
                s = ""
        if s == before:
            break
    return s


def distinctive_core(normalized: str) -> str:
    return " ".join(t for t in normalized.split() if t not in DROP_TOKENS)


def load_exchange() -> dict[str, str]:
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


def pick_sample(rows: list[dict], exchange: dict[str, str]) -> list[dict]:
    rng = random.Random(42)
    for r in rows:
        r["exchange"] = exchange.get(r["ticker"], "")
        r["norm"] = normalize_name(r["registrant_name"])
        r["core"] = distinctive_core(r["norm"])
        r["core_len"] = sum(c.isalnum() for c in r["core"])
        r["n_tokens"] = len(r["core"].split()) if r["core"] else 0

    exact = [
        r
        for r in rows
        if r["match_method"] == "exact_legal_name"
    ]
    ex21_corp = [
        r
        for r in rows
        if r["match_method"] == "ex21_subsidiary"
    ]
    exact.sort(key=lambda r: r["n_number"])
    ex21_corp.sort(key=lambda r: r["n_number"])
    rng.shuffle(exact)
    rng.shuffle(ex21_corp)
    sample = []
    for r in exact[:30]:
        r["stratum"] = "exact_legal_name_non_operator"
        sample.append(r)
    for r in ex21_corp[:40]:
        r["stratum"] = "ex21_non_operator"
        sample.append(r)
    used = {r["n_number"] for r in sample}

    remaining = [
        r
        for r in rows
        if r["match_method"] == "ex21_subsidiary" and r["n_number"] not in used
    ]
    otc = {e for e in ("OTC", "OTCQB", "OTCQX", "PINK", "OTHER OTC")}

    def stress_key(r: dict) -> tuple:
        is_otc = 0 if r["exchange"] in otc or r["exchange"].startswith("OTC") else 1
        return (r["core_len"], r["n_tokens"], is_otc, r["n_number"])

    remaining.sort(key=stress_key)
    for r in remaining:
        if len([s for s in sample if s["stratum"] == "ex21_stress"]) >= 10:
            break
        if r["n_number"] in used:
            continue
        r["stratum"] = "ex21_stress"
        sample.append(r)
        used.add(r["n_number"])
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
        raw = rec.get("N-NUMBER", "")
        key = raw.upper().lstrip("N")
        row = want.get(key)
        if not row:
            continue
        row["faa_city"] = rec.get("CITY", "")
        row["faa_state"] = rec.get("STATE", "")
        row["faa_street"] = rec.get("STREET", "")


def main() -> None:
    exchange = load_exchange()
    rows = load_mappings()
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
        "norm",
        "core",
        "core_len",
        "n_tokens",
    ]
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with OUT.open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields, extrasaction="ignore")
        w.writeheader()
        w.writerows(sample)
    print(f"wrote {OUT} rows={len(sample)}")
    from collections import Counter

    print(Counter(r["stratum"] for r in sample))


if __name__ == "__main__":
    main()
