# Harvest N-numbers mentioned in SEC filings.

Search EDGAR full text (EFTS) for aviation language, extract N-numbers that
appear next to those keywords, and write JSONL for the Rust resolver.

The resolver **must** still see each N-number in the current FAA MASTER file.
Regex hits that are not real tails are dropped.

```bash
export SEC_USER_AGENT="your-name your-email@domain"
python -m edgar_harvest.cli --output ../../evidence/edgar_hits.jsonl
```

Optional: `pip install edgartools` for cleaner DEF 14A text. Without it, the
harvester falls back to downloading the filing HTML and stripping tags.
