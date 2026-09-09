#!/usr/bin/env python3
"""Count MASTER-unique EDGAR harvest hits and print a precision sample.

Does not publish. Resolver still requires a unique CIK and a corporate-aviation
MASTER row. Exit 0 with zeros if the JSONL is missing/empty (403 spike).
"""

from __future__ import annotations

import argparse
import json
import random
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_HITS = ROOT / "evidence/edgar_hits.jsonl"
MASTER_N = ROOT / "cache/master_n_numbers.txt"
SAMPLE_N = 20
SEED = 43


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--hits", type=Path, default=DEFAULT_HITS)
    args = p.parse_args()
    hits_path = args.hits
    if not hits_path.exists():
        print(f"missing {hits_path}; harvest did not write a file")
        print("do_not_ingest_host_timer=1")
        return
    rows = []
    for line in hits_path.read_text().splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))
    print(f"jsonl_rows={len(rows)}")
    if not rows:
        print("empty harvest file; not a precision sample")
        print("do_not_ingest_host_timer=1")
        return

    master = set()
    if MASTER_N.exists():
        master = {ln.strip().upper() for ln in MASTER_N.read_text().splitlines() if ln.strip()}
    print(f"master_n_allowlist={len(master)}")

    by_n: dict[str, set[str]] = defaultdict(set)
    master_hits = []
    for r in rows:
        n = str(r.get("n_number") or "").upper()
        cik = str(r.get("cik") or "")
        in_master = (not master) or n in master
        if in_master:
            master_hits.append(r)
            by_n[n].add(cik)
        r["_in_master"] = in_master

    unique_cik = {n: ciks for n, ciks in by_n.items() if len(ciks) == 1}
    many_cik = {n: ciks for n, ciks in by_n.items() if len(ciks) > 1}
    print(f"master_unique_n={len(by_n)}")
    print(f"unique_cik_n={len(unique_cik)}  (resolver edgar_nnumber candidates)")
    print(f"conflict_n={len(many_cik)}")
    print("tickers", Counter((r.get("ticker") or "?") for r in master_hits).most_common(15))

    rng = random.Random(SEED)
    sample = master_hits[:]
    rng.shuffle(sample)
    sample = sample[:SAMPLE_N]
    print("precision_sample (label by hand; do not auto-publish):")
    for r in sample:
        print(
            f"  {r.get('n_number')} {r.get('ticker') or '-'} cik={r.get('cik')} "
            f"{r.get('form')} {r.get('accession')} {(r.get('snippet') or '')[:80]}"
        )
    if len(unique_cik) == 0:
        print("do_not_ingest_host_timer=1")
    else:
        print("sample_then_decide_host_timer=1")


if __name__ == "__main__":
    main()
