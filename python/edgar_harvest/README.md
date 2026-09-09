# Harvest N-numbers mentioned in SEC filings.

Search EDGAR full text (EFTS) for aviation language, extract N-numbers that
appear next to those keywords, and write JSONL for the Rust resolver.

The resolver **must** still see each N-number in the current FAA MASTER file.
Regex hits that are not real tails are dropped.

## Fair access (required)

Connecting to EDGAR is legitimate public access. Honor the SEC’s rules; do not
probe around them. Official:

- [Accessing EDGAR Data](https://www.sec.gov/search-filings/edgar-search-assistance/accessing-edgar-data)
- [Webmaster FAQ](https://www.sec.gov/about/webmaster-frequently-asked-questions) (“Undeclared Automated Tool”)

**User-Agent is required**, not optional branding. Use the SEC sample shape
(app name + real email). This is what 200s:

```text
tail-to-ticker you@real-domain
```

Do **not** use `tail-to-ticker/0.1 (https://github.com/alexwoolford/tail-to-ticker; you@real-domain)`.
Akamai returns **403 `AkamaiGHost`** for that github-paren form and treats it as
an undeclared bot — even from a residential IP where Safari loads the same URL.
Do not use `example.com`, an empty string, or `FAA_USER_AGENT` (that Safari-like
token is for `registry.faa.gov` only). The harvest CLI refuses placeholder UAs
the same way `refresh` does.

**Rate:** maximum **10 requests per second per IP** across `www.sec.gov`,
`efts.sec.gov`, and `data.sec.gov`. That is a ceiling, not a target. Default
`--sleep` is **0.5s** (about 2 req/s) because harvest also GETs filing HTML.
Sleep below **0.1s** is rejected. Do not rotate User-Agents to dodge the cap
(it is per IP). Download only the EFTS hits we need; do not crawl every 10-K.

EFTS `_id` is `{accession}:{filename}` for the **document that matched** (often
an EX-10), not the primary 10-K/DEF 14A. Phrase queries like `"corporate aircraft"`
hit proxy prose with no N-numbers; `"FAA Registration Number"` hits the exhibit
that actually lists tails. Do not substitute the iXBRL primary for that file.

**Two different 403s** (do not mix them up):

| Symptom | Likely cause | Not |
|---|---|---|
| `403` + undeclared-bot / github-paren UA / `example.com` | Policy: undeclared tool | Rate limit |
| `403` + `Server: AkamaiGHost` on a datacenter IP **after** the sample-shaped UA already 200s from home | Datacenter/IP reputation | 10/s cap |
| `CONNECT` 403 via `HTTPS_PROXY` | IPRoyal blocking `.gov` | SEC fair access |

Do not put harvest on `tail-to-ticker-refresh.timer`. It is a labeled oneshot:
refresh publishes from tracked [`overrides/edgar_allowlist.jsonl`](../../overrides/edgar_allowlist.jsonl),
not from a harvest dump. A declared GET 200 from this IP is a fair-access check,
not a reason to add harvest to the daily job.

```bash
export SEC_USER_AGENT='tail-to-ticker you@real-domain'
./scripts/edgar-fair-access-check.sh
```

Harvest GETs the EFTS-matched **exhibit** (`_id` = `{accession}:{filename}`), not
the primary 10-K/DEF 14A. Do not crawl the 10-K universe. Default queries are the
two live phrases only (`"FAA Registration Number"`, `"aircraft bearing U.S. registration"`),
`--start 2018-01-01`, `--sleep 0.5`. Output is **raw** JSONL
(`evidence/edgar_hits_raw.jsonl`). The CLI refuses `--output` that would overwrite
the allowlist.

```bash
export SEC_USER_AGENT='tail-to-ticker you@real-domain'
python -m edgar_harvest.cli --output ../../evidence/edgar_hits_raw.jsonl --max-hits 50
```

Do **not** pass `--output ../../overrides/edgar_allowlist.jsonl`.
Hand-label TPs into the allowlist after the TP gate (unique CIK, MASTER, issuer
or operating sub). `unique_edgar` does not encode that gate.
