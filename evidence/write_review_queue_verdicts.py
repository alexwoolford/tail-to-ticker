#!/usr/bin/env python3
"""Verdicts for evidence/review_queue_sample.csv (seed-43). Hold by default."""

from __future__ import annotations

import csv
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SAMPLE = ROOT / "evidence/review_queue_sample.csv"
OUT = ROOT / "evidence/review_queue_verdicts.csv"

# n_number -> (verdict, reason, citation)
# Do not publish unpublished_tails or generic EX-21 namesakes.
VERDICTS: dict[str, tuple[str, str, str]] = {
    "N660S": (
        "must_not_publish",
        "gold unpublished_tails ALPINE II LLC ≠ Aimco (AIV)",
        "overrides/gold.yaml unpublished_tails N660S",
    ),
    "N881RC": (
        "must_not_publish",
        "gold unpublished_tails Cooper Companies Scottsdale ≠ COO San Ramon",
        "overrides/gold.yaml unpublished_tails N881RC",
    ),
    "N584A": (
        "hold_address_cluster",
        "ARAMCO ASSOCIATED CO at EOG HQ street is not EOG; do not enable --publish-address-cluster",
        "FAA registrant ARAMCO ASSOCIATED CO",
    ),
    "N651XA": (
        "hold_address_cluster",
        "Same Aramco / EOG address-cluster false positive as N584A",
        "FAA registrant ARAMCO ASSOCIATED CO",
    ),
    "N40D": (
        "promote_override",
        "Gold at-risk TP WARBLER I LLC → DOW",
        "overrides/gold.yaml tails N40D; overrides/mappings.yaml",
    ),
    "N340FL": (
        "promote_override",
        "Gold at-risk TP HINES HILL AVIATION LLC → ARHS",
        "overrides/gold.yaml tails N340FL; overrides/mappings.yaml",
    ),
}


def default_verdict(row: dict) -> tuple[str, str, str]:
    method = row["match_method"]
    reg = (row.get("registrant_name") or "").upper()
    if method == "address_cluster":
        return (
            "hold_address_cluster",
            "Address cluster stays on review; HQ street is not identity",
            row.get("source_url") or "",
        )
    if reg in {"HELICOPTERS INC", "TURBINES LTD"}:
        return (
            "hold_generic_ex21",
            "Generic aviation word as unique EX-21 key; Bristow/Edison collision risk",
            "pudl:exhibit21",
        )
    if "HOLDING" in reg.split() or "HOLDINGS" in reg.split():
        return (
            "hold_identity_suffix",
            "HoldCo token is the corroboration deny list (LEAR HOLDING class)",
            "name_match_corroborated identity suffixes",
        )
    return (
        "hold_uncorroborated_ex21",
        "Unique EX-21 without brand/ticker/N-number corroboration; leave on review",
        "pudl:exhibit21",
    )


def main() -> None:
    with SAMPLE.open() as f:
        sample = list(csv.DictReader(f))
    out_rows = []
    for row in sample:
        n = row["n_number"]
        verdict, reason, citation = VERDICTS.get(n, default_verdict(row))
        out_rows.append(
            {
                **row,
                "verdict": verdict,
                "reason": reason,
                "citation": citation,
            }
        )
    fields = list(out_rows[0].keys())
    with OUT.open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields)
        w.writeheader()
        w.writerows(out_rows)
    print(f"wrote {OUT} rows={len(out_rows)}")
    print(Counter(r["verdict"] for r in out_rows))
    promo = [r for r in out_rows if r["verdict"].startswith("promote")]
    print("promote", [(r["n_number"], r["ticker"]) for r in promo])


if __name__ == "__main__":
    main()
