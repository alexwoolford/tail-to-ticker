"""Harvest N-numbers from SEC EDGAR full-text search into JSONL for the Rust resolver.

Uses the public EFTS index (https://efts.sec.gov/LATEST/search-index). EFTS `_id`
is the matched document (often an EX-10), not the primary 10-K/DEF 14A. Does not
replace the FAA round-trip check — the Rust resolver drops any N-number that is
not in the current MASTER file.
"""

from __future__ import annotations

import argparse
import gzip
import html as html_lib
import json
import os
import random
import re
import sys
import time
import zlib
from pathlib import Path
from typing import Any
from urllib.parse import urlencode

import urllib.request

EFTS = "https://efts.sec.gov/LATEST/search-index"
N_RE = re.compile(r"\bN[1-9][0-9A-Z]{2,4}\b")
# SEC max is 10 req/s per IP. That is a ceiling, not a target. Harvest also GETs
# filing HTML after each EFTS hit, so default well under the cap.
MIN_SLEEP_S = 0.1
DEFAULT_SLEEP_S = 0.5
DEFAULT_OUTPUT = "evidence/edgar_hits_raw.jsonl"
DEFAULT_START = "2018-01-01"
# Live phrases that hit EX-10 fleet/time-sharing text. Dead proxy phrases
# ("corporate aircraft") are not default queries.
DEFAULT_QUERIES = (
    '"FAA Registration Number"',
    '"aircraft bearing U.S. registration"',
)
ALLOWLIST_NAME = "edgar_allowlist.jsonl"
# Skip giant S-4 primaries; EFTS hits are usually ~100KB exhibits.
MAX_DOC_BYTES = 6_000_000
SKIP_SUFFIXES = (
    ".jpg",
    ".jpeg",
    ".png",
    ".gif",
    ".svg",
    ".pdf",
    ".zip",
    ".xsd",
    ".xml",
    ".js",
    ".css",
)
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


def placeholder_user_agent(user_agent: str) -> bool:
    t = user_agent.strip().lower()
    return not t or "example.com" in t


def user_agent() -> str:
    return os.environ.get("SEC_USER_AGENT", "").strip()


def require_network_user_agent(ua: str) -> None:
    if placeholder_user_agent(ua):
        raise SystemExit(
            "Set SEC_USER_AGENT to a real descriptive contact (not example.com) "
            "before contacting SEC. SEC sample shape: tail-to-ticker you@real-domain "
            "(Akamai 403s the github-paren form as an undeclared bot)."
        )
    if "github.com" in ua.lower() or "(" in ua:
        print(
            "warning: SEC/Akamai 403s User-Agents that look like "
            "tail-to-ticker/0.1 (https://github.com/...; email). "
            "Use: tail-to-ticker you@real-domain",
            file=sys.stderr,
        )


def require_sleep(sleep: float) -> None:
    if sleep < MIN_SLEEP_S:
        raise SystemExit(
            f"--sleep {sleep} is below {MIN_SLEEP_S}s "
            "(SEC max is 10 requests/second per IP; that ceiling is not a target)"
        )


def decode_body(raw: bytes, content_encoding: str | None) -> bytes:
    enc = (content_encoding or "").lower()
    if "gzip" in enc:
        return gzip.decompress(raw)
    if "deflate" in enc:
        return zlib.decompress(raw)
    return raw


def _request(url: str) -> urllib.request.Request:
    return urllib.request.Request(
        url,
        headers={
            "User-Agent": user_agent(),
            "Accept": "application/json, text/html, */*",
            "Accept-Encoding": "gzip, deflate",
            "Accept-Language": "en-US,en;q=0.9",
        },
    )


def fetch(url: str) -> bytes:
    with urllib.request.urlopen(_request(url), timeout=60) as resp:
        return decode_body(resp.read(), resp.headers.get("Content-Encoding"))


def fetch_document(url: str, max_bytes: int = MAX_DOC_BYTES) -> bytes | None:
    """GET a filing document. None if it is larger than max_bytes (compressed or raw)."""
    with urllib.request.urlopen(_request(url), timeout=60) as resp:
        cl = resp.headers.get("Content-Length")
        if cl and cl.isdigit() and int(cl) > max_bytes:
            return None
        raw = resp.read(max_bytes + 1)
        if len(raw) > max_bytes:
            return None
        try:
            body = decode_body(raw, resp.headers.get("Content-Encoding"))
        except (OSError, EOFError, gzip.BadGzipFile, zlib.error):
            return None
        if len(body) > max_bytes:
            return None
        return body


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


def to_plain_text(raw: bytes) -> str:
    return html_lib.unescape(re.sub(r"<[^>]+>", " ", raw.decode("utf-8", errors="ignore")))


def skip_document_filename(name: str) -> bool:
    nl = name.lower().rsplit("?", 1)[0]
    return any(nl.endswith(suf) for suf in SKIP_SUFFIXES) or nl.endswith("-index.html") or nl.endswith("-index.htm")


def efts_document_filename(hit_id: str | None, adsh: str) -> str | None:
    """EFTS `_id` is `{accession}:{filename}` for the document that matched, not the primary 10-K/DEF 14A."""
    if not hit_id:
        return None
    if ":" in hit_id:
        prefix, name = hit_id.split(":", 1)
        if prefix == adsh and name and not skip_document_filename(name):
            return name
        if name and not skip_document_filename(name) and name.lower().endswith((".htm", ".html", ".txt")):
            return name
        return None
    if hit_id.lower().endswith((".htm", ".html", ".txt")) and not skip_document_filename(hit_id):
        return hit_id
    return None


def archives_url(cik: str, accession: str, filename: str) -> str:
    cik_n = str(int(cik))
    acc_nodash = accession.replace("-", "")
    return f"https://www.sec.gov/Archives/edgar/data/{cik_n}/{acc_nodash}/{filename}"


IX_DOC_RE = re.compile(
    r"ix\?doc=(/Archives/edgar/data/\d+/\d+/[^&\"']+\.(?:htm|html))",
    re.I,
)
ARCHIVES_HREF_RE = re.compile(
    r'href="(/Archives/edgar/data/\d+/\d+/([^"]+\.(?:htm|html)))"',
    re.I,
)
# Current EDGAR index.html document table (sequence, description, href).
INDEX_SEQ_RE = re.compile(
    r'<td scope="row">(\d+)</td>\s*'
    r'<td scope="row">[^<]*</td>\s*'
    r'<td scope="row"><a href="(?:/ix\?doc=)?(/Archives/edgar/data/\d+/\d+/([^"]+))"',
    re.I,
)


def document_url_for_sequence(index_html: str, sequence: int | None) -> str | None:
    if sequence is None:
        return None
    for seq_s, path, name in INDEX_SEQ_RE.findall(index_html):
        try:
            seq = int(seq_s)
        except ValueError:
            continue
        if seq != sequence or skip_document_filename(name):
            continue
        if path.startswith("/Archives/"):
            return "https://www.sec.gov" + path
        return "https://www.sec.gov/Archives/" + path.lstrip("/")
    return None


def primary_document_url(index_html: str, cik: str, accession: str) -> str:
    """Index pages do not contain filing prose. Resolve the primary .htm (or complete .txt)."""
    cik_n = str(int(cik))
    acc_nodash = accession.replace("-", "")
    index_name = f"{accession}-index.html".lower()
    for path in IX_DOC_RE.findall(index_html):
        name = path.rsplit("/", 1)[-1].lower()
        if name == index_name or name.endswith("-index.html"):
            continue
        return "https://www.sec.gov" + path
    for path, name in ARCHIVES_HREF_RE.findall(index_html):
        nl = name.lower()
        if nl == index_name or nl.endswith("-index.html"):
            continue
        return "https://www.sec.gov" + path
    return f"https://www.sec.gov/Archives/edgar/data/{cik_n}/{acc_nodash}/{accession}.txt"


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


def hit_document_url(
    cik: str,
    adsh: str,
    hit: dict[str, Any],
    src: dict[str, Any],
    sleep: float,
) -> str | None:
    """Resolve the EFTS-matched document (exhibit), not the primary 10-K/DEF 14A."""
    hit_id = str(hit.get("_id") or "")
    filename = efts_document_filename(hit_id, adsh)
    if not filename and isinstance(src.get("file_name"), str):
        filename = src["file_name"]
    if filename and skip_document_filename(filename):
        return None
    if filename:
        return archives_url(cik, adsh, filename)
    sequence = src.get("sequence")
    try:
        seq = int(sequence) if sequence is not None else None
    except (TypeError, ValueError):
        seq = None
    index_url, _ = filing_urls(cik, adsh, None)
    time.sleep(sleep)
    index_html = fetch(index_url).decode("utf-8", errors="ignore")
    return document_url_for_sequence(index_html, seq) or primary_document_url(index_html, cik, adsh)


def refuse_allowlist_output(path: Path) -> None:
    if path.name == ALLOWLIST_NAME:
        raise SystemExit(
            "Harvest writes raw JSONL only; do not overwrite "
            "overrides/edgar_allowlist.jsonl (hand-labeled TPs)."
        )


def harvest(args: argparse.Namespace) -> None:
    require_network_user_agent(user_agent())
    require_sleep(args.sleep)
    out_path = Path(args.output)
    refuse_allowlist_output(out_path)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    n_filter = None
    if args.n_numbers_file:
        n_filter = {ln.strip().upper() for ln in Path(args.n_numbers_file).read_text().splitlines() if ln.strip()}

    seen: set[tuple[str, str, str]] = set()
    written = 0
    efts_hits_n = 0
    filings_n = 0
    n_re_n = 0
    keyword_n = 0
    master_n = 0
    skip_bin_n = 0
    skip_size_n = 0
    with out_path.open("w", encoding="utf-8") as fh:
        for query in args.query:
            offset = 0
            while True:
                if filings_n >= args.max_hits:
                    break
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
                efts_hits_n += len(hits)
                for hit in hits:
                    if filings_n >= args.max_hits:
                        break
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
                    try:
                        doc_url = hit_document_url(cik, adsh, hit, src, args.sleep)
                    except Exception as exc:  # noqa: BLE001
                        print(f"skip {adsh}: {exc}", file=sys.stderr)
                        continue
                    if not doc_url:
                        skip_bin_n += 1
                        continue
                    filename = doc_url.rsplit("/", 1)[-1]
                    if skip_document_filename(filename):
                        skip_bin_n += 1
                        continue
                    key = (cik, adsh, filename.lower())
                    if key in seen:
                        continue
                    seen.add(key)
                    if filings_n >= args.max_hits:
                        break
                    filings_n += 1
                    if filings_n <= 5:
                        print(f"open {doc_url}", file=sys.stderr)
                    try:
                        time.sleep(args.sleep)
                        raw = fetch_document(doc_url)
                    except Exception as exc:  # noqa: BLE001
                        print(f"skip {adsh}: {exc}", file=sys.stderr)
                        continue
                    if raw is None:
                        skip_size_n += 1
                        print(f"skip_size {doc_url}", file=sys.stderr)
                        continue
                    text = to_plain_text(raw)
                    n_re_n += len(N_RE.findall(text))
                    keyword_n += len(extract_hits(text, None))
                    hits_out = extract_hits(text, n_filter)
                    master_n += len(hits_out)
                    for n_number, snippet in hits_out:
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
    print(
        f"diag efts_hits={efts_hits_n} filings={filings_n} n_re={n_re_n} "
        f"keyword={keyword_n} master={master_n} written={written} "
        f"skip_bin={skip_bin_n} skip_size={skip_size_n}",
        file=sys.stderr,
    )
    print(f"wrote {written} hits to {out_path}")


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--query",
        nargs="+",
        default=DEFAULT_QUERIES,
        help="EFTS query strings (default: live exhibit phrases only)",
    )
    p.add_argument("--forms", default="DEF 14A,8-K,10-K,10-Q,EX-10")
    p.add_argument("--start", default=DEFAULT_START)
    p.add_argument("--end", default="2026-12-31")
    p.add_argument("--max-hits", type=int, default=400, help="max EFTS filings to open (not the 10/s cap)")
    p.add_argument(
        "--sleep",
        type=float,
        default=DEFAULT_SLEEP_S,
        help="pause between SEC requests in seconds (default 0.5; min 0.1; 10/s is the SEC max, not a target)",
    )
    p.add_argument(
        "--output",
        default=DEFAULT_OUTPUT,
        help="raw harvest JSONL (never overrides/edgar_allowlist.jsonl)",
    )
    p.add_argument("--n-numbers-file", help="optional allowlist of N-numbers (one per line)")
    return p


def main() -> None:
    harvest(build_parser().parse_args())


if __name__ == "__main__":
    main()
