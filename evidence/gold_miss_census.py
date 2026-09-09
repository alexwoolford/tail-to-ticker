#!/usr/bin/env python3
"""Classify gold.yaml companies vs published mappings, review_queue, trusts, MASTER."""

from __future__ import annotations

import csv
import re
import sqlite3
import zipfile
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GOLD = ROOT / "overrides/gold.yaml"
DB = ROOT / "data/current/tail_to_ticker.sqlite"
ZIP = ROOT / "cache/ReleasableAircraft.zip"
OUT = ROOT / "evidence/gold_miss_census.csv"

SUFFIXES = [
    "INCORPORATED",
    "CORPORATION",
    "COMPANY",
    "LIMITED",
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
        if ch.isalnum() and ord(ch) < 128:
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


def load_acftref(zf: zipfile.ZipFile) -> dict[str, tuple[str, str]]:
    name = next(n for n in zf.namelist() if n.upper().endswith("ACFTREF.TXT"))
    text = zf.read(name).decode("latin-1", errors="replace")
    out: dict[str, tuple[str, str]] = {}
    for i, line in enumerate(text.splitlines()):
        parts = line.split(",")
        if not parts:
            continue
        code = parts[0].strip().upper()
        if not code or i == 0 and "CODE" in code:
            continue
        make = parts[1].strip() if len(parts) > 1 else ""
        model = parts[2].strip() if len(parts) > 2 else ""
        out[code] = (make, model)
    return out


TURBINE_ENGINES = {"2", "3", "4", "5"}
CORP_AIRFRAMES = {"5", "6"}
AIRLINER_MODELS = (
    "737",
    "747",
    "757",
    "767",
    "777",
    "787",
    "A318",
    "A319",
    "A320",
    "A321",
    "A330",
    "A340",
    "A350",
    "A380",
    "ERJ",
    "E170",
    "E175",
    "E190",
    "E195",
    "CRJ",
    "MD-80",
    "MD-90",
    "DC-9",
    "A220",
)
CORP_AIRLINER_HINTS = ("BBJ", "ACJ", "BUSINESS", "VIP", "LINEAGE", "PRESTIGE")


def airframe_reason(type_aircraft: str, type_engine: str, make: str, model: str) -> str | None:
    """None = would pass `is_corporate_aviation` type/engine/mfr gate (name class is separate)."""
    eng = type_engine.strip()
    if eng not in TURBINE_ENGINES:
        return "not_turbine"
    air = type_aircraft.strip()
    if air not in CORP_AIRFRAMES:
        return "not_multi_or_rotor"
    blob = f"{make} {model}".upper()
    if any(h in blob for h in CORP_AIRLINER_HINTS):
        return None
    if "PHENOM" in blob or "PRAETOR" in blob or "LEGACY" in blob:
        return None
    if any(m in blob for m in AIRLINER_MODELS):
        return "airliner_type"
    return None


def load_master_by_norm() -> dict[str, list[tuple[str, str, str | None]]]:
    """normalized registrant -> [(n_number, raw_name, airframe_reason), ...]"""
    out: dict[str, list[tuple[str, str, str | None]]] = defaultdict(list)
    with zipfile.ZipFile(ZIP) as zf:
        refs = load_acftref(zf)
        name = next(n for n in zf.namelist() if n.upper().endswith("MASTER.TXT"))
        raw = zf.read(name)
    if raw.startswith(b"\xef\xbb\xbf"):
        raw = raw[3:]
    text = raw.decode("latin-1", errors="replace")
    reader = csv.DictReader(text.splitlines())
    for rec in reader:
        rec = {(k or "").lstrip("\ufeff").strip(): (v or "").strip() for k, v in rec.items()}
        raw_n = rec.get("N-NUMBER", "")
        n = "N" + re.sub(r"[^0-9A-Z]", "", raw_n.upper().lstrip("N"))
        if len(n) < 2:
            continue
        name = rec.get("NAME", "")
        if not name:
            continue
        code = rec.get("MFR MDL CODE", "").upper()
        make, model = refs.get(code, ("", ""))
        reason = airframe_reason(
            rec.get("TYPE AIRCRAFT", ""),
            rec.get("TYPE ENGINE", ""),
            make,
            model,
        )
        out[normalize_name(name)].append((n, name, reason))
    return out


def load_gold_companies(path: Path) -> list[dict]:
    companies: list[dict] = []
    current: dict | None = None
    in_companies = False
    for line in path.read_text().splitlines():
        if line.startswith("companies:"):
            in_companies = True
            continue
        if in_companies and line.startswith("tails:"):
            break
        if not in_companies:
            continue
        if line.startswith("  - ticker:"):
            if current:
                companies.append(current)
            current = {
                "ticker": line.split(":", 1)[1].strip(),
                "company_name": "",
                "registrant_names": [],
            }
        elif current is None:
            continue
        elif line.startswith("    company_name:"):
            current["company_name"] = line.split(":", 1)[1].strip()
        elif line.startswith("    registrant_names:"):
            rest = line.split(":", 1)[1].strip()
            if rest.startswith("[") and rest.endswith("]"):
                inner = rest[1:-1]
                current["registrant_names"] = [
                    p.strip().strip("'\"") for p in inner.split(",") if p.strip()
                ]
    if current:
        companies.append(current)
    return companies


def main() -> None:
    gold = load_gold_companies(GOLD)
    conn = sqlite3.connect(DB)
    conn.row_factory = sqlite3.Row
    published = {
        r["ticker"].upper(): dict(r)
        for r in conn.execute(
            "SELECT ticker, COUNT(*) tails FROM mappings_current WHERE deleted_at IS NULL GROUP BY 1"
        )
    }
    review_by_ticker: dict[str, list[sqlite3.Row]] = defaultdict(list)
    for r in conn.execute("SELECT n_number, ticker, match_method, registrant_name FROM review_queue"):
        review_by_ticker[r["ticker"].upper()].append(r)
    trusts = list(
        conn.execute(
            "SELECT n_number, registrant_name, reason FROM unresolved_trusts"
        )
    )
    conn.close()

    print("loading MASTER…")
    master = load_master_by_norm()
    print(f"MASTER distinct normalized names {len(master)}")

    rows = []
    for c in gold:
        ticker = c["ticker"].upper()
        names = list(c.get("registrant_names") or [])
        if c.get("company_name"):
            names.append(c["company_name"])
        norms = {normalize_name(n) for n in names if n}
        master_hits = []
        seen_n = set()
        for norm in norms:
            for n_number, raw, reason in master.get(norm, []):
                if n_number in seen_n:
                    continue
                seen_n.add(n_number)
                master_hits.append((n_number, raw, reason))
        airframe_pass = [h for h in master_hits if h[2] is None]
        airframe_fail = [h for h in master_hits if h[2] is not None]
        fail_reasons = sorted({h[2] for h in airframe_fail if h[2]})
        trust_hits = []
        for t in trusts:
            tn = normalize_name(t["registrant_name"])
            if tn in norms or any(norm and norm in tn for norm in norms if len(norm) >= 6):
                trust_hits.append(t)
        rev = review_by_ticker.get(ticker, [])
        pub = published.get(ticker)
        if pub:
            bucket = "published"
        elif rev:
            bucket = "review_queue"
        elif airframe_pass:
            bucket = "master_name_not_published"
        elif master_hits:
            bucket = "airframe_filtered"
        elif trust_hits:
            bucket = "trust_name_only"
        else:
            bucket = "no_master_legal_name"
        rows.append(
            {
                "ticker": ticker,
                "company_name": c.get("company_name", ""),
                "bucket": bucket,
                "published_tails": pub["tails"] if pub else 0,
                "review_n": len(rev),
                "review_registrants": " | ".join(
                    sorted({r["registrant_name"] for r in rev})[:8]
                ),
                "master_n": len(master_hits),
                "master_names": " | ".join(sorted({h[1] for h in master_hits})[:8]),
                "airframe_pass_n": len(airframe_pass),
                "airframe_fail_n": len(airframe_fail),
                "airframe_fail_reasons": " | ".join(fail_reasons),
                "trust_n": len(trust_hits),
            }
        )

    OUT.parent.mkdir(parents=True, exist_ok=True)
    fields = list(rows[0].keys())
    with OUT.open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields)
        w.writeheader()
        w.writerows(rows)
    from collections import Counter

    print(f"wrote {OUT}")
    print(Counter(r["bucket"] for r in rows))
    missing = [r for r in rows if r["bucket"] != "published"]
    print("not published:")
    for r in missing:
        print(
            f"  {r['ticker']:6} {r['bucket']:28} master={r['master_n']} "
            f"pass={r['airframe_pass_n']} fail={r['airframe_fail_n']} "
            f"review={r['review_n']} trust={r['trust_n']}"
            + (f" ({r['airframe_fail_reasons']})" if r["airframe_fail_reasons"] else "")
        )


if __name__ == "__main__":
    main()
