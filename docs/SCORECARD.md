# Mapping scorecard

This is the experiment log for the FAA-registrant → listed-ticker join. It is not an equity-universe coverage target.

**Source of truth is the printer:** `python3 evidence/scorecard.py`. Floors and coverage below are the last print (local feed `refresh_run` 2026-09-09T03:08:03Z). Do not treat `eval` name recall as a north star (the denominator includes AMZN airliners and DE/HON pistons).

Scorecard name precision uses the same definition as `eval` (published gold-name hits must match the gold ticker) over this script’s MASTER corp-aviation + eligible-class slice. It can differ slightly from `tail-to-ticker eval` if refresh’s aircraft slice differs. This printer does not shell out to the binary.

`refresh_run` is still only `as_of_date` / `recorded_at`. Do not persist these counts into sqlite until we decide we are iterating again.

## Floors (must not move)

| Metric | Floor | Last print |
|---|---|---|
| Unpublished-tail false positives | **0** (`refresh` abort-gates this) | **0 / 19 PASS** |
| Gold name precision | **1.000** (0 FPs among published gold-name hits) | **1.000 (70/70) PASS** |
| Identity corroboration | off | Unique listed name / alias publishes; EX-21 still needs corroboration |

Gold tail recall is informational (last print 4/7 present in MASTER, 1 skipped missing). Do not chase name recall. That metric overlaps `mappings.yaml` and is not matcher quality.

The mega-cap `gold.yaml` **companies** list is a census (on MASTER or not), not a matching rubric. Matcher quality is [`overrides/rubric.yaml`](../overrides/rubric.yaml) **holdout by stratum** (`eval --rubric`, also printed by the scorecard). `refresh` does not read the rubric. Gold `unpublished_tails` remains the production suppress gate — eval unpublished FPs=0 tests that gate.

## Eval rubric (holdout)

Gaffes this file exists to stop:

- Easy-set bias (WALMART INC / NIKE INC name precision does not test Homerlease or WARBLER I LLC).
- Dual-use gate (unpublished_tails is both test and suppress).
- Leakage (aliases mined from gold names; yaml tails counted as tail recall; Textron-heavy row samples).
- Wrong target (195 → 5k issuers, or name recall including AMZN airliners).
- One-issuer “wins” (Chevron USA spacing is not an identity program).

**10-Ks of every listed company are not identity coverage.** Subsidiary *names* already come from 10-K Exhibit 21 (PUDL; 383 of 603 published rows). What 10-Ks still add is N-numbers in exhibits — H3 `edgar_nnumber` (8 rows) from labeled EFTS hits, not a 10-K crawl.

Strata: `identity_easy` / `identity_hard` / `subsidiary_tp` / `subsidiary_hold` / `vanity` / `filing` / `namesake` / `not_our_join` (never FN). Holdout N-numbers are disjoint from mappings.yaml, unpublished_tails, and matcher tests. Cap two tails per ticker.

`identity_hard` holdout is **thin** (CVX `CHEVRON USA INC` only). Do not collapse `USA` ≡ `U S A` and call identity solved. `vanity` holdout is empty on purpose. `filing` holdout is labeled H3 TPs only (n=5). That **1.000 at n=5 is not the n≥30 kill test** (kill: precision < 0.95 at n≥30). Do not quote filing holdout as H3 passing.

Re-print holdout numbers with `python3 evidence/scorecard.py` (section `rubric holdout`). Last print (`refresh_run` 2026-09-09T03:08:03Z):

| Stratum | n | recall | precision | Note |
|---|---|---|---|---|
| identity_easy | 3 | 1.000 | 1.000 | Does not stretch the matcher |
| identity_hard | 2 | **0.000** | n/a | CVX `CHEVRON USA INC` only — thin; do not ship a USA-spacing rule |
| subsidiary_tp | 3 | 1.000 | 1.000 | Corroborated EX-21 holdout TPs |
| subsidiary_hold | 4 | n/a | n/a | 0 FPs (stayed on review) |
| namesake | 1 | n/a | n/a | 0 FPs (ARAMCO/EOG not published) |
| vanity | 0 | — | — | Empty on purpose (N40D/N340FL are mappings.yaml) |
| filing | 5 | 1.000 | 1.000 | Labeled H3 TPs (MPC Tesoro, MC, DGX); **not** n≥30 |

## Coverage (last print)

| Metric | Value |
|---|---|
| Published | 603 tails / 197 tickers |
| Method mix | `ex21_subsidiary` 383, `exact_legal_name` 205, `edgar_nnumber` 8, `manual_override` 7 |
| Review queue | 461 (`ex21_subsidiary` 392, `address_cluster` 68, `exact_legal_name` 1) |
| Unresolved trusts | 3115 |
| `aviation_issuer=0` | 366 tails / 185 tickers (flight department / parent) |
| `aviation_issuer=1` | 237 tails / 12 tickers (listed aviation business) |
| Overrides | `issuer_aliases` 2, `mappings.yaml` 7, `edgar_nnumber` 8 |

Do not grow YAML from memory.

## Gold-miss buckets (last print)

| Bucket | n | Meaning |
|---|---|---|
| published | 22 | Gold company has at least one published tail |
| `no_master_legal_name` | 22 | Listed name never appears on MASTER |
| review_queue | 4 | PEP / HAL / GM / ITW — SPVs, not identity |
| airframe_filtered | 4 | DE / HON / LMT piston, AMZN 625 airliners |
| `master_name_not_published` | **0** | Identity harvest is empty at the company grain |

## Already falsified (do not re-learn)

| Experiment | Result | Implication |
|---|---|---|
| Review-queue sample n=66 | 0 promotions; ARAMCO@EOG address FP; generic EX-21 (`HELICOPTERS INC`) | Do not `--publish-address-cluster`. Do not dump review into YAML. |
| Gold-miss census (~52 companies) | 0 `master_name_not_published`; 22 never on MASTER; 4 review SPVs; 4 airframe-filtered | Identity aliases cannot create tails that are not on MASTER as the listed name. |
| Homerlease prefix rule | One pun; would overfit | Keep five cited N-numbers. Do not implement `parent_prefix_corroborated`. |
| EDGAR harvest | Matrix 2026-09-08: github-paren UA → 403 AkamaiGHost (Mac + OCI). SEC sample shape `tail-to-ticker email` → **200 nginx** on both. Safari-string curl also 200 (do not adopt). | The 403 was undeclared-looking UA, not IP denylist and not 10/s. Host env rewritten to sample shape. |
| Identity uniqueness | N100A/N289MT publish as `exact_legal_name`; unpublished-tail FPs still 0 | That rule is done. Do not wrap identity in corroboration again. |
| H1 identity-alias harvest | 1 issuer (`CHEVRON USA INC` / 13 tails), below 3 issuers | **Killed.** Do not add YAML. Do not collapse `USA` ≡ `U S A`. |

## Experiments (closed / blocked)

The printer prints **KILLED / BLOCKED / REOPEN / WATCH**, never `CLEARS`. A watch is not a matching theory.

### H1 — Identity-alias harvest — killed

Leftover watch: `CHEVRON USA INC` (CVX), 13 tails. Spacing variant of an identity already publishing (`CHEVRON U S A INC`). Reopen only if a **new** printer run finds **≥3 issuers** of leftover identity aliases — not because Chevron grew more tails. Still no matcher change and no YAML from the printer.

### H2 — Homerlease-class EX-21 corroboration — killed

Reopen only if a **second independent** unique EX-21 portmanteau appears in the review queue (not Homerlease, not a namesake HoldCo). Manual; the printer does not scrape puns. Until then the five HD rows stay cited tails.

### H3 — EDGAR N-number harvest — labeled allowlist; not on the host timer

Watch is `edgar_nnumber` on the feed (last print **8**). H3 added **0** published tails: those eight were already `ex21_subsidiary` (method mix only). Phrase-set H3 is a second pointer at identity EX-21 already had, not a coverage experiment.

Egress was never the remaining bug: EFTS `_id` is `{accession}:{filename}` for the **matched exhibit**, and harvest was fetching the iXBRL primary 10-K/DEF 14A (no N-numbers). Phrase queries (`"corporate aircraft"`) still hit proxy prose. `"FAA Registration Number"` hits EX-10 fleet/time-sharing text.

**Not a 10-K / DEF 14A census.** Count-only EFTS 2018–2026 (no document GETs, 2026-09-09): `"FAA Registration Number"` **96** hits total (**16** 10-K, **0** DEF 14A, **22** 10-Q, **32** 8-K). `"aircraft bearing U.S. registration"` **72** (**8** 10-K, **0** DEF 14A, **15** 10-Q, **41** 8-K). Cached ticker file: **10391** rows / **8001** unique CIKs (primary listings). Ingest opened **35** of those 96 phrase hits, not ~8k latest 10-Ks. N-numbers that publish live in exhibits (usually EX-10), not proxy/10-K narrative. A later expansion is EFTS roll-forward of new accessions, not a daily universe GET and not `tail-to-ticker-refresh.timer`.

Host sample (`--query '"FAA Registration Number"' --start 2018-01-01 --max-hits 35 --sleep 0.5`, MASTER allowlist): **diag efts_hits=70 filings=35 n_re=464 keyword=191 master=95 written=95**. Analyzer: 33 unique-CIK N-numbers, unpublished-tail overlap **0**.

Phrase-set finish (both queries, all forms, `--max-hits 250`, 2026-09-09): **filings=155 written=679 unique_n=342 unpublished overlap 0**. Almost all hits are airline 8-Ks / Wheels Up indentures (excluded). Non-airline MASTER names that are the issuer or a clear operating sub: the original five plus six DGX `QUEST DIAGNOSTICS CLINICAL LABORATORIES INC` (three Phenoms published; three PC-12s airframe-filtered). Tracked allowlist [`overrides/edgar_allowlist.jsonl`](../overrides/edgar_allowlist.jsonl) has the **8** publishing TPs. **`edgar_nnumber` 8**; published still **603 / 197**. Dropped the rest (trustees, Hilltop/BX, Carlisle/DPZ, USAF N898M, SPVs). **Phrase set exhausted. Stop auto-publishing.** Later work is quarterly EFTS roll-forward of *new* accessions with the same TP gate, not a 10-K crawl and not the host timer.

Hand labels (do not dump into YAML):

- **TP (allowlist):** N1895T/N1901G CVX (`CHEVRON U S A INC`); N457MP/N459MP MPC (`TESORO AVIATION CO`); N909ZM MC (`MOELIS & COMPANY MANAGER LLC`); N288DX/N648DX/N899DX DGX (`QUEST DIAGNOSTICS CLINICAL LABORATORIES INC`). PC-12s N120QD/N338QD/N687QD labeled same registrant but airframe-filtered (not in the allowlist).
- **Dropped:** airline fleets; N898M (USAF); N147CJ (Carlisle ≠ DPZ); Wheels Up indentures; BX Hilltop/GH4; bank-trustee MPC/Carlyle tails; other SPVs.

Local `refresh --use-cache` loads [`overrides/edgar_allowlist.jsonl`](../overrides/edgar_allowlist.jsonl) (not `evidence/edgar_hits.jsonl`): unpublished FPs **0**, gold name precision **1.000**, published **603 / 197**. Filing holdout **1.000 at n=5 is not** the n≥30 kill. Harvest stays off `tail-to-ticker-refresh.timer` because it is a labeled oneshot, not a daily job. Kill: unpublished-tail FPs > 0 or filing-holdout precision < 0.95 at n≥30.

**Matrix 2026-09-08** (laptop, same email, sleep 0.5s) — UA archaeology, closed:

| Cell | HTTP | Server | Class |
|---|---|---|---|
| github-paren declared UA (control, http1.1/v4/v6, `company_tickers.json`) | 403 | AkamaiGHost | HTML deny |
| `data.sec.gov` submissions + github-paren UA | 403 | AkamaiGHost | undeclared automated tool |
| SEC sample shape `tail-to-ticker email` (no github, no parens) | **200** | nginx | JSON |
| Stock Safari UA string (diagnostic only) | **200** | nginx | JSON |
| Same sample shape from ct-firehose | **200** | nginx | JSON |

Do **not** adopt a fake Safari UA as production identity. Host env stays the sample shape.

## Not worth it

- Growing [`overrides/mappings.yaml`](../overrides/mappings.yaml) except cited vanity / one-pun FNs
- Adding `CHEVRON USA INC` as an issuer alias
- Publishing trusts or address clusters
- Loosening EX-21 corroboration (LEAR HOLDING, `HELICOPTERS INC`, CARTER MACHINERY)
- Widening the airframe filter to pick up DE/HON/AMZN
- Changing mosaic `warehouse.issuer` into an equity universe — it is `GROUP BY ticker, cik` on published mappings

## Strategies (rank against holdout; do not run in this slice)

1. **Filing / EFTS N-numbers (H3)** — phrase set exhausted (`edgar_nnumber` 8; **0** new published tails). Not a 10-K crawl and not on the host timer. Kill: holdout `filing` precision below a pre-declared floor (sample n≥30, precision < 0.95) or unpublished-tail FPs > 0. Do not treat filing 1.000 at n=5 as that test.
2. **Identity_hard normalization** — only if holdout has ≥3 issuers. Chevron-only stays killed.
3. **EX-21 corroboration (H2)** — killed until a second independent portmanteau.
4. **Address / trusts / airframe / giant yaml** — not this product.

## Next action

**Phrase set done. Do not change the matcher** to chase 197 issuers. H3 added **0** published tails. The ingest contract is tracked [`overrides/edgar_allowlist.jsonl`](../overrides/edgar_allowlist.jsonl) (8 TPs). Harvest writes raw JSONL only and is not a timer job. Coverage left that this join should not auto-publish: trusts, uncorroborated SPVs, names that never appear on MASTER.
