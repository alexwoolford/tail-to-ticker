"""Harvest N-numbers from SEC EDGAR full-text search into JSONL for the Rust resolver.

Uses the public EFTS index (https://efts.sec.gov/LATEST/search-index) and optional
edgartools for cleaner DEF 14A text. Does not replace the FAA round-trip check —
the Rust resolver drops any N-number that is not in the current MASTER file.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import re
import sys
import time
from pathlib import Path
from typing import Any
from urllib.parse import urlencode

import urllib.request

EFTS = "https://efts.sec.gov/LATEST/search-index"
N_RE = re.compile(r"\bN[1-9][0-9A-Z]{2,4}\b")
KEYWORDS = (
    "corporate aircraft",
    "corporate jet",
    "time-sharing",
    "timesharing",
    "dry lease",
    "aggregate incremental cost",
    "gulfstream",
    "bombardier",
    "dassault",
    "falcon",
    "citation",
    "personal use of the company",
    "aviation",
    "aircraft",
    "n-number",
    "tail number",
)
WINDOW = 180


def user_agent() -> str:
    return os.environ.get(
        "SEC_USER_AGENT",
        "tail-to-ticker-edgar-harvest/0.1 (contact@example.com)",
    )


def fetch(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": user_agent(), "Accept": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as resp:
        return resp.read()


def efts_search(query: str, forms: str, start: str, end: str, offset: int) -> dict[str, Any]:
    params = {
        "q": query,
        "forms": forms,
        "dateRange": "custom",
        "startdt": start,
        "enddt": end,
        "from": offset,
        "size": 100,
    }
    url = f"{EFTS}?{urlencode(params)}"
    return json.loads(fetch(url).decode("utf-8"))


def keyword_hit(text: str) -> bool:
    low = text.lower()
    return any(k in low for k in KEYWORDS)


def extract_hits(text: str, n_filter: set[str] | None) -> list[tuple[str, str]]:
    out: list[tuple[str, str]] = []
    for m in N_RE.finditer(text):
        n = m.group(0)
        if n_filter and n not in n_filter:
            continue
        start = max(0, m.start() - WINDOW)
        end = min(len(text), m.end() + WINDOW)
        snippet = re.sub(r"\s+", " ", text[start:end]).strip()
        if keyword_hit(snippet) or keyword_hit(text[max(0, m.start() - 800) : m.end() + 800]):
            out.append((n, snippet))
    return out


def filing_urls(cik: str, accession: str, filename: str | None) -> tuple[str, str]:
    cik_n = str(int(cik))
    acc_nodash = accession.replace("-", "")
    index = f"https://www.sec.gov/Archives/edgar/data/{cik_n}/{acc_nodash}/{accession}-index.html"
    doc = (
        f"https://www.sec.gov/Archives/edgar/data/{cik_n}/{acc_nodash}/{filename}"
        if filename
        else index
    )
    return index, doc


def try_edgartools_text(cik: str, accession: str) -> str | None:
    try:
        from edgar import Filing, set_identity
    except ImportError:
        return None
    try:
        set_identity(user_agent())
        filing = Filing(cik=int(cik), filing_date="1900-01-01", form="DEF 14A", company="", accession_no=accession)
        return filing.text()
    except Exception:
        return None


def harvest(args: argparse.Namespace) -> None:
    out_path = Path(args.output)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    n_filter = None
    if args.n_numbers_file:
        n_filter = {ln.strip().upper() for ln in Path(args.n_numbers_file).read_text().splitlines() if ln.strip()}

    seen: set[tuple[str, str, str]] = set()
    written = 0
    with out_path.open("w", encoding="utf-8") as fh:
        for query in args.query:
            offset = 0
            while offset < args.max_hits:
                time.sleep(args.sleep + random.random() * 0.05)
                try:
                    payload = efts_search(query, args.forms, args.start, args.end, offset)
                except Exception as exc:  # noqa: BLE001
                    print(f"EFTS error at offset {offset}: {exc}", file=sys.stderr)
                    break
                hits = payload.get("hits", {}).get("hits") or payload.get("hits") or []
                if isinstance(hits, dict):
                    hits = hits.get("hits", [])
                if not hits:
                    break
                for hit in hits:
                    src = hit.get("_source") or hit
                    cik = str(src.get("ciks", src.get("cik", [""]))[0] if isinstance(src.get("ciks"), list) else src.get("cik") or src.get("ciks") or "")
                    if isinstance(src.get("ciks"), list) and src["ciks"]:
                        cik = str(src["ciks"][0])
                    display = src.get("display_names") or src.get("entity_name") or []
                    ticker = None
                    if isinstance(display, list) and display:
                        # "Walmart Inc.  (WMT)  (CIK 0000104169)"
                        m = re.search(r"\(([A-Z][A-Z0-9.\-]{0,6})\)", display[0])
                        if m:
                            ticker = m.group(1)
                    adsh = src.get("adsh") or src.get("accession_number") or ""
                    form = src.get("form") or src.get("root_forms") or ""
                    if isinstance(form, list):
                        form = form[0] if form else ""
                    file_desc = None
                    docs = src.get("file_types") or src.get("file_names")
                    if isinstance(src.get("file_name"), str):
                        file_desc = src["file_name"]
                    index_url, doc_url = filing_urls(cik, adsh, file_desc)
                    key = (cik, adsh, query)
                    if key in seen:
                        continue
                    seen.add(key)
                    text = try_edgartools_text(cik, adsh)
                    if text is None:
                        try:
                            time.sleep(args.sleep)
                            raw = fetch(doc_url).decode("utf-8", errors="ignore")
                            text = re.sub(r"<[^>]+>", " ", raw)
                        except Exception as exc:  # noqa: BLE001
                            print(f"skip {adsh}: {exc}", file=sys.stderr)
                            continue
                    for n_number, snippet in extract_hits(text, n_filter):
                        rec = {
                            "cik": cik.zfill(10) if cik.isdigit() else cik,
                            "ticker": ticker,
                            "accession": adsh,
                            "form": form,
                            "n_number": n_number,
                            "snippet": snippet[:500],
                            "filing_url": doc_url,
                            "keyword_hit": True,
                        }
                        fh.write(json.dumps(rec) + "\n")
                        written += 1
                offset += 100
                if len(hits) < 100:
                    break
    print(f"wrote {written} hits to {out_path}")


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--query",
        nargs="+",
        default=["\"corporate aircraft\"", "\"time-sharing agreement\"", "\"corporate jet\""],
        help="EFTS query strings",
    )
    p.add_argument("--forms", default="DEF 14A,8-K,10-K,10-Q,EX-10")
    p.add_argument("--start", default="2020-01-01")
    p.add_argument("--end", default="2026-12-31")
    p.add_argument("--max-hits", type=int, default=400)
    p.add_argument("--sleep", type=float, default=0.12, help="SEC fair-access pause (seconds)")
    p.add_argument("--output", default="evidence/edgar_hits.jsonl")
    p.add_argument("--n-numbers-file", help="optional allowlist of N-numbers (one per line)")
    return p


def main() -> None:
    harvest(build_parser().parse_args())


if __name__ == "__main__":
    main()
