#!/usr/bin/env python3
"""Living mapping scorecard: floors, coverage, gold buckets, closed experiments.

Stdout only. Does not write YAML or change the matcher.
"""

from __future__ import annotations

import csv
import json
import re
import sqlite3
import sys
import zipfile
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(Path(__file__).resolve().parent))

import gold_miss_census as gmc  # noqa: E402

GOLD = ROOT / "overrides/gold.yaml"
ALIASES = ROOT / "overrides/issuer_aliases.yaml"
MAPPINGS = ROOT / "overrides/mappings.yaml"
RUBRIC = ROOT / "overrides/rubric.yaml"
DB = ROOT / "data/current/tail_to_ticker.sqlite"
ZIP = ROOT / "cache/ReleasableAircraft.zip"
TICKERS = ROOT / "cache/company_tickers_exchange.json"

H1_REOPEN_ISSUERS = 3

MFR_ALLOW = (
    "GULFSTREAM",
    "BOMBARDIER",
    "CANADAIR",
    "LEARJET",
    "DASSAULT",
    "FALCON",
    "EMBRAER",
    "CESSNA",
    "TEXTRON",
    "HAWKER",
    "BEECHCRAFT",
    "BEECH",
    "PILATUS",
    "HONDA",
    "ECLIPSE",
    "CIRRUS",
    "IAI",
    "ISRAEL",
    "SABRELINER",
    "MITSUBISHI",
    "PIAGGIO",
    "SOCATA",
    "DAHER",
    "BRITISH AEROSPACE",
    "BAE",
    "WESTWIND",
    "ASTRA",
    "SIKORSKY",
    "LEONARDO",
    "AGUSTA",
    "AIRBUS HELICOPTER",
    "EUROCOPTER",
    "BELL",
    "BOEING",
    "AIRBUS",
)

TRUSTEE_NEEDLES = (
    "TRUSTEE",
    "OWNER TRUST",
    "AIRCRAFT TRUST",
    "BANK OF UTAH",
    "WILMINGTON TRUST",
    "TVPX",
    "WELLS FARGO BANK NORTHWEST",
    "WELLS FARGO TRUST",
    "US BANK TRUST",
    "U.S. BANK TRUST",
    "UMB BANK",
    "BANK OF OKLAHOMA",
    "BOKF",
    "FIRST SECURITY BANK",
)
FRACTIONAL_NEEDLES = (
    "NETJETS",
    "FLEXJET",
    "VISTAJET",
    "VISTA JET",
    "WHEELS UP",
    "PLANESENSE",
    "PLANE SENSE",
    "FLIGHT OPTIONS",
    "CITATIONSHARES",
    "CITATION SHARES",
    "DIRECTIONAL AVIATION",
    "NICHOLAS AIR",
    "JET LINX",
    "JETLINX",
    "SOLAIRUS",
    "EXECUTIVE JET",
    "AIRSHARE",
    "NJASPE",
)
AIRLINE_NEEDLES = (
    "AMERICAN AIRLINES",
    "DELTA AIR LINES",
    "DELTA AIRLINES",
    "UNITED AIRLINES",
    "SOUTHWEST AIRLINES",
    "JETBLUE",
    "ALASKA AIRLINES",
    "SPIRIT AIRLINES",
    "FRONTIER AIRLINES",
    "HAWAIIAN AIRLINES",
    "SKYWEST",
    "FEDERAL EXPRESS",
    "FEDEX CORPORATION",
    "FEDEX CORP",
    "UNITED PARCEL",
    "ENVOY AIR",
    "REPUBLIC AIRWAYS",
    "REPUBLIC AIRLINE",
    "MESA AIRLINES",
    "ALLEGIANT AIR",
    "SUN COUNTRY",
    "ATLAS AIR",
    "KALITTA",
    "HORIZON AIR",
    "PSA AIRLINES",
    "ENDEAVOR AIR",
    "AIR WISCONSIN",
    "COMMUTEAIR",
    "GOJET",
    "PIEDMONT AIRLINES",
    "AMERIFLIGHT",
    "ABX AIR",
    "POLAR AIR",
    "BREEZE AVIATION",
    "JETBLUE AIRWAYS",
)

MAJOR_EXCHANGES = {
    "NYSE",
    "NASDAQ",
    "NYSE ARCA",
    "NYSE AMERICAN",
    "NYSE MKT",
    "AMEX",
}


def pad_cik(raw: str) -> str:
    digits = re.sub(r"\D", "", str(raw))
    return digits.zfill(10) if digits else ""


def canonical_n(raw: str) -> str:
    s = re.sub(r"[^0-9A-Z]", "", (raw or "").upper().lstrip("N"))
    return f"N{s}" if s else ""


def yaml_block_records(path: Path, key: str) -> list[dict]:
    """Tiny list-of-maps reader for our override YAML (no PyYAML required)."""
    lines = path.read_text().splitlines()
    records: list[dict] = []
    current: dict | None = None
    in_block = False
    for line in lines:
        if line.startswith(f"{key}:"):
            in_block = True
            continue
        if in_block and line and not line[0].isspace() and line.rstrip().endswith(":"):
            break
        if not in_block:
            continue
        if re.match(r"  - \w", line):
            if current:
                records.append(current)
            field, _, rest = line.strip()[2:].partition(":")
            current = {field.strip(): rest.strip().strip('"')}
        elif current is not None and re.match(r"    \w", line):
            field, _, rest = line.strip().partition(":")
            current[field.strip()] = rest.strip().strip('"')
    if current:
        records.append(current)
    return records


def load_gold() -> tuple[list[dict], dict[str, str]]:
    companies = []
    current: dict | None = None
    in_companies = False
    for line in GOLD.read_text().splitlines():
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
                "ticker": line.split(":", 1)[1].strip().upper(),
                "cik": "",
                "company_name": "",
                "registrant_names": [],
            }
        elif current is None:
            continue
        elif line.startswith("    cik:"):
            current["cik"] = pad_cik(line.split(":", 1)[1].strip().strip('"'))
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
    unpublished = {
        canonical_n(r["n_number"]): r["must_not_ticker"].upper()
        for r in yaml_block_records(GOLD, "unpublished_tails")
        if r.get("n_number") and r.get("must_not_ticker")
    }
    return companies, unpublished


def load_aliases() -> list[dict]:
    out = []
    for r in yaml_block_records(ALIASES, "aliases"):
        if not r.get("name"):
            continue
        out.append(
            {
                "cik": pad_cik(r.get("cik", "")),
                "ticker": (r.get("ticker") or "").upper(),
                "name": r["name"].strip(),
            }
        )
    return out


def common_share_rank(ticker: str, exchange: str) -> tuple:
    t = ticker.upper()
    hyphen = "-" in t or "." in t
    structured = False
    if "-" in t:
        rest = t.split("-", 1)[1]
        structured = rest.startswith("P") or rest.startswith("W")
    series = (not hyphen) and len(t) >= 5 and t[-1] in "POW"
    major = exchange.upper() in MAJOR_EXCHANGES
    return (
        0 if hyphen else 1,
        0 if structured else 1,
        0 if series else 1,
        1 if major else 0,
        t,
    )


def primary_listings(rows: list[dict]) -> list[dict]:
    best: dict[str, dict] = {}
    for c in rows:
        cik = c["cik"]
        cur = best.get(cik)
        if cur is None or common_share_rank(c["ticker"], c["exchange"]) > common_share_rank(
            cur["ticker"], cur["exchange"]
        ):
            best[cik] = c
    return list(best.values())


def load_sec_companies() -> list[dict]:
    body = json.loads(TICKERS.read_text())
    fields = [f.lower() for f in body["fields"]]
    idx = {name: i for i, name in enumerate(fields)}
    companies = []
    for row in body["data"]:
        ticker = str(row[idx["ticker"]]).upper()
        if not ticker:
            continue
        companies.append(
            {
                "cik": pad_cik(row[idx["cik"]]),
                "name": str(row[idx["name"]]),
                "ticker": ticker,
                "exchange": str(row[idx.get("exchange", 3)] if "exchange" in idx else ""),
                "former_names": [],
            }
        )
    primaries = primary_listings(companies)
    aliases = load_aliases()
    by_cik = {c["cik"]: c for c in primaries}
    by_ticker: dict[str, list[dict]] = defaultdict(list)
    for c in primaries:
        by_ticker[c["ticker"]].append(c)
    for a in aliases:
        key = gmc.normalize_name(a["name"])
        if not key:
            continue
        targets = []
        if a["cik"] in by_cik:
            targets.append(by_cik[a["cik"]])
        for c in by_ticker.get(a["ticker"], []):
            if c not in targets:
                targets.append(c)
        for c in targets:
            already = gmc.normalize_name(c["name"]) == key or any(
                gmc.normalize_name(n) == key for n in c["former_names"]
            )
            if not already:
                c["former_names"].append(a["name"])
    return primaries


def build_name_index(companies: list[dict]) -> dict[str, list[str]]:
    index: dict[str, list[str]] = defaultdict(list)
    for c in companies:
        for raw in [c["name"], *c["former_names"]]:
            key = gmc.normalize_name(raw)
            if not key:
                continue
            if c["ticker"] not in index[key]:
                index[key].append(c["ticker"])
    return index


def classify_name(name: str, type_registrant: str, fract_owner: str) -> str | None:
    if type_registrant.strip() == "1":
        return "individual"
    if (fract_owner or "").strip().upper() in {"Y", "1", "TRUE"}:
        return "faa_fractional"
    n = name.upper()
    if any(k in n for k in TRUSTEE_NEEDLES):
        return "trustee"
    if any(k in n for k in FRACTIONAL_NEEDLES):
        return "fractional"
    if any(k in n for k in AIRLINE_NEEDLES):
        return "airline"
    return None


def corporate_reason(
    type_aircraft: str,
    type_engine: str,
    make: str,
    model: str,
    type_registrant: str,
    status: str,
) -> str | None:
    if status.strip() not in {"V", "v"}:
        return "invalid_status"
    if type_registrant.strip() == "1":
        return "individual"
    air = gmc.airframe_reason(type_aircraft, type_engine, make, model)
    if air:
        return air
    blob = f"{make} {model}".upper()
    eng = type_engine.strip()
    mfr_ok = any(m in blob for m in MFR_ALLOW)
    jet = eng in {"4", "5"}
    if mfr_ok or (jet and type_aircraft.strip() == "5"):
        return None
    return "manufacturer"


def load_master_rows() -> list[dict]:
    rows = []
    with zipfile.ZipFile(ZIP) as zf:
        refs = gmc.load_acftref(zf)
        name = next(n for n in zf.namelist() if n.upper().endswith("MASTER.TXT"))
        raw = zf.read(name)
    if raw.startswith(b"\xef\xbb\xbf"):
        raw = raw[3:]
    text = raw.decode("latin-1", errors="replace")
    reader = csv.DictReader(text.splitlines())
    for rec in reader:
        rec = {(k or "").lstrip("\ufeff").strip(): (v or "").strip() for k, v in rec.items()}
        n = canonical_n(rec.get("N-NUMBER", ""))
        if len(n) < 2:
            continue
        name = rec.get("NAME", "")
        if not name:
            continue
        code = rec.get("MFR MDL CODE", "").upper()
        make, model = refs.get(code, ("", ""))
        rows.append(
            {
                "n_number": n,
                "name": name,
                "norm": gmc.normalize_name(name),
                "make": make,
                "model": model,
                "corp_reason": corporate_reason(
                    rec.get("TYPE AIRCRAFT", ""),
                    rec.get("TYPE ENGINE", ""),
                    make,
                    model,
                    rec.get("TYPE REGISTRANT", ""),
                    rec.get("STATUS CODE", ""),
                ),
                "class_reason": classify_name(
                    name, rec.get("TYPE REGISTRANT", ""), rec.get("FRACT OWNER", "")
                ),
            }
        )
    return rows


def print_counts(title: str, rows: list[tuple]) -> None:
    print(title)
    if not rows:
        print("  (none)")
        return
    width = max(len(str(k)) for k, _ in rows)
    for k, n in rows:
        print(f"  {str(k):<{width}}  {n}")


def gold_miss_buckets(gold: list[dict], conn: sqlite3.Connection, master_by_norm: dict) -> list[dict]:
    published = {
        r[0].upper(): r[1]
        for r in conn.execute(
            "SELECT ticker, COUNT(*) FROM mappings_current WHERE deleted_at IS NULL GROUP BY 1"
        )
    }
    review_by_ticker: dict[str, list] = defaultdict(list)
    for r in conn.execute(
        "SELECT n_number, ticker, match_method, registrant_name FROM review_queue"
    ):
        review_by_ticker[r[1].upper()].append(r)
    trusts = list(conn.execute("SELECT n_number, registrant_name, reason FROM unresolved_trusts"))
    rows = []
    for c in gold:
        ticker = c["ticker"]
        names = list(c.get("registrant_names") or [])
        if c.get("company_name"):
            names.append(c["company_name"])
        norms = {gmc.normalize_name(n) for n in names if n}
        master_hits = []
        seen = set()
        for norm in norms:
            for rec in master_by_norm.get(norm, []):
                if rec["n_number"] in seen:
                    continue
                seen.add(rec["n_number"])
                master_hits.append(rec)
        airframe_pass = [h for h in master_hits if h["corp_reason"] is None]
        airframe_fail = [h for h in master_hits if h["corp_reason"]]
        trust_hits = []
        for t in trusts:
            tn = gmc.normalize_name(t[1])
            if tn in norms or any(norm and norm in tn for norm in norms if len(norm) >= 6):
                trust_hits.append(t)
        rev = review_by_ticker.get(ticker, [])
        if ticker in published:
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
                "bucket": bucket,
                "published_tails": published.get(ticker, 0),
                "review_n": len(rev),
                "master_n": len(master_hits),
                "airframe_pass_n": len(airframe_pass),
                "airframe_fail_n": len(airframe_fail),
            }
        )
    return rows


def unique_tickers(index: dict[str, list[str]], key: str) -> list[str]:
    return list(index.get(key, []))


def harvest_h1(
    gold: list[dict],
    unpublished: dict[str, str],
    name_index: dict[str, list[str]],
    master_by_norm: dict[str, list[dict]],
    published_by_n: dict[str, str],
    review_by_n: dict[str, str],
) -> tuple[list[dict], Counter]:
    candidates = []
    why = Counter()
    seen_pairs: set[tuple[str, str]] = set()
    for c in gold:
        ticker = c["ticker"]
        names = list(c.get("registrant_names") or [])
        raw_names = []
        for n in names:
            if n and n not in raw_names:
                raw_names.append(n)
        for raw in raw_names:
            key = gmc.normalize_name(raw)
            if not key:
                continue
            hits = master_by_norm.get(key, [])
            if not hits:
                continue
            indexed = unique_tickers(name_index, key)
            addable_ns = []
            for rec in hits:
                n = rec["n_number"]
                if rec["corp_reason"]:
                    why[f"airframe:{rec['corp_reason']}"] += 1
                    continue
                if rec["class_reason"]:
                    why[f"class:{rec['class_reason']}"] += 1
                    continue
                forbidden = unpublished.get(n)
                if forbidden == ticker:
                    why["unpublished_suppress"] += 1
                    continue
                pub = published_by_n.get(n)
                if pub == ticker:
                    why["already_published_same_ticker"] += 1
                    continue
                if pub and pub != ticker:
                    why["published_other_ticker"] += 1
                    continue
                if n in review_by_n:
                    why["review_queue"] += 1
                    continue
                if len(indexed) == 1 and indexed[0] == ticker:
                    why["already_in_name_index"] += 1
                    continue
                if len(indexed) > 1 or (len(indexed) == 1 and indexed[0] != ticker):
                    why["uniqueness_collision"] += 1
                    continue
                addable_ns.append(n)
            if not addable_ns:
                continue
            pair = (ticker, raw.upper())
            if pair in seen_pairs:
                continue
            seen_pairs.add(pair)
            candidates.append(
                {
                    "ticker": ticker,
                    "cik": c["cik"],
                    "faa_name": raw,
                    "tails": sorted(set(addable_ns)),
                }
            )
    return candidates, why


def expected_gold_names(gold: list[dict]) -> dict[str, str]:
    expected: dict[str, str] = {}
    for c in gold:
        ticker = c["ticker"]
        for name in c.get("registrant_names") or []:
            key = gmc.normalize_name(name)
            if key:
                expected[key] = ticker
        if c.get("company_name"):
            key = gmc.normalize_name(c["company_name"])
            if key:
                expected[key] = ticker
    return expected


def unpublished_fps(
    unpublished: dict[str, str], published_by_n: dict[str, str]
) -> list[tuple[str, str]]:
    hits = []
    for n, forbidden in unpublished.items():
        got = published_by_n.get(n)
        if got == forbidden:
            hits.append((n, forbidden))
    return hits


def name_precision(
    expected: dict[str, str],
    master_rows: list[dict],
    published_by_n: dict[str, str],
) -> tuple[int, int, list[tuple[str, str, str]]]:
    """Eval-style name precision over this script's MASTER corp-aviation eligible slice."""
    tp = 0
    fps: list[tuple[str, str, str]] = []
    for rec in master_rows:
        if rec["corp_reason"] or rec["class_reason"]:
            continue
        exp = expected.get(rec["norm"])
        if not exp:
            continue
        pub = published_by_n.get(rec["n_number"])
        if pub is None:
            continue
        if pub == exp:
            tp += 1
        else:
            fps.append((rec["n_number"], pub, exp))
    return tp, len(fps), fps


def tail_recall(
    gold_tails: list[dict],
    master_ns: set[str],
    published_by_n: dict[str, str],
) -> tuple[int, int, int]:
    tp = fn = skip = 0
    for t in gold_tails:
        n = canonical_n(t.get("n_number", ""))
        if not n or n not in master_ns:
            skip += 1
            continue
        want = (t.get("ticker") or "").upper()
        if published_by_n.get(n) == want:
            tp += 1
        else:
            fn += 1
    return tp, fn, skip


def rubric_holdout(
    rows: list[dict], master_ns: set[str], published_by_n: dict[str, str]
) -> tuple[int, int, dict[str, dict[str, int]]]:
    """Eval-only holdout by stratum. not_our_join is counted but not scored as FN."""
    from collections import OrderedDict

    strata: dict[str, dict[str, int]] = OrderedDict()
    holdout = 0
    not_our = 0
    for r in rows:
        if (r.get("split") or "").lower() != "holdout":
            continue
        holdout += 1
        stratum = r.get("stratum") or ""
        if stratum == "not_our_join":
            not_our += 1
            continue
        slot = strata.setdefault(
            stratum, {"n": 0, "skip": 0, "tp": 0, "fn": 0, "fp": 0}
        )
        slot["n"] += 1
        n = canonical_n(r.get("n_number", ""))
        if not n or n not in master_ns:
            slot["skip"] += 1
            continue
        ticker = (r.get("ticker") or "").upper()
        forbidden = (r.get("must_not_ticker") or "").upper()
        pub = published_by_n.get(n)
        if ticker:
            if pub == ticker:
                slot["tp"] += 1
            else:
                slot["fn"] += 1
        elif forbidden and pub == forbidden:
            slot["fp"] += 1
    return holdout, not_our, strata


def pf(ok: bool) -> str:
    return "PASS" if ok else "FAIL"


def main() -> None:
    for path, label in (
        (DB, "sqlite"),
        (ZIP, "FAA zip"),
        (TICKERS, "SEC tickers"),
        (GOLD, "gold.yaml"),
        (ALIASES, "issuer_aliases.yaml"),
        (MAPPINGS, "mappings.yaml"),
        (RUBRIC, "rubric.yaml"),
    ):
        if not path.is_file():
            sys.exit(f"missing {label}: {path}")

    conn = sqlite3.connect(DB)
    published_n = conn.execute(
        "SELECT COUNT(*) FROM mappings_current WHERE deleted_at IS NULL"
    ).fetchone()[0]
    issuers = conn.execute(
        "SELECT COUNT(DISTINCT ticker) FROM mappings_current WHERE deleted_at IS NULL"
    ).fetchone()[0]
    methods = list(
        conn.execute(
            "SELECT match_method, COUNT(*) FROM mappings_current "
            "WHERE deleted_at IS NULL GROUP BY 1 ORDER BY 2 DESC"
        )
    )
    edgar_n = conn.execute(
        "SELECT COUNT(*) FROM mappings_current "
        "WHERE deleted_at IS NULL AND match_method = 'edgar_nnumber'"
    ).fetchone()[0]
    aviation = list(
        conn.execute(
            "SELECT aviation_issuer, COUNT(*), COUNT(DISTINCT ticker) "
            "FROM mappings_current WHERE deleted_at IS NULL GROUP BY 1 ORDER BY 1"
        )
    )
    review_n = conn.execute("SELECT COUNT(*) FROM review_queue").fetchone()[0]
    review_methods = list(
        conn.execute("SELECT match_method, COUNT(*) FROM review_queue GROUP BY 1 ORDER BY 2 DESC")
    )
    trusts = conn.execute("SELECT COUNT(*) FROM unresolved_trusts").fetchone()[0]
    refresh = conn.execute(
        "SELECT as_of_date, recorded_at FROM refresh_run ORDER BY recorded_at DESC LIMIT 1"
    ).fetchone()
    published_by_n = {
        r[0]: r[1].upper()
        for r in conn.execute(
            "SELECT n_number, ticker FROM mappings_current WHERE deleted_at IS NULL"
        )
    }
    review_by_n = {
        r[0]: r[1].upper() if r[1] else ""
        for r in conn.execute("SELECT n_number, ticker FROM review_queue")
    }

    print("loading MASTER…")
    master_rows = load_master_rows()
    master_by_norm: dict[str, list[dict]] = defaultdict(list)
    master_ns = set()
    for rec in master_rows:
        master_by_norm[rec["norm"]].append(rec)
        master_ns.add(rec["n_number"])

    gold, unpublished = load_gold()
    gold_tails = yaml_block_records(GOLD, "tails")
    alias_n = len(load_aliases())
    mapping_n = len(yaml_block_records(MAPPINGS, "mappings"))
    buckets = gold_miss_buckets(gold, conn, master_by_norm)
    conn.close()

    fp_hits = unpublished_fps(unpublished, published_by_n)
    expected = expected_gold_names(gold)
    name_tp, name_fp, name_fp_rows = name_precision(expected, master_rows, published_by_n)
    name_denom = name_tp + name_fp
    name_prec = 1.0 if name_denom == 0 else name_tp / name_denom
    tail_tp, tail_fn, tail_skip = tail_recall(gold_tails, master_ns, published_by_n)
    tail_denom = tail_tp + tail_fn
    tail_rec = 1.0 if tail_denom == 0 else tail_tp / tail_denom

    print("\n== floors ==")
    if refresh:
        print(f"refresh_run              {refresh[0]}  {refresh[1]}")
    print(
        f"unpublished-tail FPs     {len(fp_hits)} / {len(unpublished)} gold  "
        f"{pf(len(fp_hits) == 0)}"
    )
    for n, ticker in fp_hits:
        print(f"  FAIL  {n} published as {ticker}")
    print(
        f"name precision           {name_prec:.3f}  ({name_tp}/{name_denom} "
        f"published gold-name hits)  {pf(name_fp == 0)}"
    )
    for n, got, exp in name_fp_rows[:8]:
        print(f"  FAIL  {n} published {got} expected {exp}")
    print(
        f"gold tail recall         {tail_rec:.3f}  ({tail_tp}/{tail_denom}; "
        f"skipped missing MASTER {tail_skip})  info  (not matcher quality; yaml overlap)"
    )

    rubric_rows = yaml_block_records(RUBRIC, "rows")
    rh, not_our, rstrata = rubric_holdout(rubric_rows, master_ns, published_by_n)
    print("\n== rubric holdout (eval-only; not the production gate) ==")
    print(f"holdout rows             {rh}  (not_our_join {not_our} unscored)")
    for stratum, slot in rstrata.items():
        scored = slot["tp"] + slot["fn"]
        rec = "n/a" if scored == 0 else f"{slot['tp'] / scored:.3f}"
        pden = slot["tp"] + slot["fp"]
        prec = "n/a" if pden == 0 else f"{slot['tp'] / pden:.3f}"
        print(
            f"  {stratum:<18} n={slot['n']}  skip_master={slot['skip']}  "
            f"tp={slot['tp']}  fn={slot['fn']}  fp={slot['fp']}  "
            f"recall={rec}  precision={prec}"
        )

    print("\n== coverage ==")
    print(f"published                {published_n} tails / {issuers} tickers")
    print(f"review_queue             {review_n}")
    print(f"trusts                   {trusts}")
    print_counts("method mix (published)", methods)
    print_counts("method mix (review)", review_methods)
    print("aviation_issuer")
    for flag, n, tickers in aviation:
        label = "listed aviation business" if flag else "flight department / parent"
        print(f"  {flag}  {n} tails / {tickers} tickers  ({label})")
    print(
        f"overrides                issuer_aliases {alias_n}  "
        f"mappings.yaml {mapping_n}  edgar_nnumber {edgar_n}"
    )

    print("\n== gold-miss buckets ==")
    print_counts("companies", sorted(Counter(r["bucket"] for r in buckets).items()))
    missing = [r for r in buckets if r["bucket"] != "published"]
    print("not published")
    for r in missing:
        print(
            f"  {r['ticker']:6} {r['bucket']:28} master={r['master_n']} "
            f"pass={r['airframe_pass_n']} fail={r['airframe_fail_n']} review={r['review_n']}"
        )

    print("\nloading SEC name index…")
    companies = load_sec_companies()
    name_index = build_name_index(companies)
    print(f"primary listings {len(companies)}  name keys {len(name_index)}")

    candidates, why = harvest_h1(
        gold, unpublished, name_index, master_by_norm, published_by_n, review_by_n
    )
    h1_issuers = sorted({c["ticker"] for c in candidates})
    h1_tails = sorted({n for c in candidates for n in c["tails"]})
    h1_status = "REOPEN" if len(h1_issuers) >= H1_REOPEN_ISSUERS else "KILLED"

    print("\n== experiments ==")
    print(
        f"H1 identity-alias harvest  {h1_status}  "
        f"{len(h1_issuers)} issuers / {len(h1_tails)} leftover tails  "
        f"(reopen only if ≥{H1_REOPEN_ISSUERS} issuers, not more Chevron tails)"
    )
    for c in candidates:
        print(
            f"  watch {c['ticker']:6} {c['cik']}  {c['faa_name']!r}  "
            f"tails={len(c['tails'])} {','.join(c['tails'][:8])}"
            + ("…" if len(c["tails"]) > 8 else "")
        )
    if why:
        print("unpublished MASTER gold-name hits (explanation, not a rule)")
        print_counts("why", sorted(why.items(), key=lambda kv: (-kv[1], kv[0])))
    print(
        "H2 Homerlease corroboration  KILLED  "
        "reopen only on a second independent EX-21 portmanteau in review (manual)"
    )
    print(
        f"H3 EDGAR N-number harvest    WATCH  edgar_nnumber={edgar_n}  "
        "added 0 published tails (EX-21 re-rank); labeled allowlist only; "
        "not on the host timer; filing 1.000 at n=5 is not n≥30; "
        "kill if unpublished-tail FPs>0 or filing holdout precision <0.95 at n≥30"
    )


if __name__ == "__main__":
    main()
