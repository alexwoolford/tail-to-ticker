#!/usr/bin/env bash
# Oneshot: force-fresh FAA/SEC/PUDL refresh, then atomically publish sqlite.
# A failed refresh leaves /var/lib/tail-to-ticker/current/ untouched.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${TAIL_TO_TICKER_BIN:-$ROOT/bin/tail-to-ticker}"
STATE="${TAIL_TO_TICKER_STATE:-/var/lib/tail-to-ticker}"
DATA="${TAIL_TO_TICKER_DATA:-$STATE/work}"
CACHE="${TAIL_TO_TICKER_CACHE:-$STATE/cache}"
PUBLISH="${TAIL_TO_TICKER_PUBLISH:-$STATE/current/tail_to_ticker.sqlite}"
OVERRIDES="${TAIL_TO_TICKER_OVERRIDES:-$ROOT/overrides}"
LOCK="${TAIL_REFRESH_LOCK:-$STATE/.refresh.lock}"
AS_OF="${TAIL_TO_TICKER_AS_OF:-$(date -u +%Y-%m-%d)}"

test -x "$BIN" || {
  echo "missing $BIN — build with: cargo build --release" >&2
  exit 1
}
test -f "$OVERRIDES/mappings.yaml" || {
  echo "missing $OVERRIDES/mappings.yaml" >&2
  exit 1
}
test -f "$OVERRIDES/gold.yaml" || {
  echo "missing $OVERRIDES/gold.yaml" >&2
  exit 1
}
test -f "$OVERRIDES/aviation_issuers.yaml" || {
  echo "missing $OVERRIDES/aviation_issuers.yaml" >&2
  exit 1
}
test -f "$OVERRIDES/issuer_aliases.yaml" || {
  echo "missing $OVERRIDES/issuer_aliases.yaml" >&2
  exit 1
}

mkdir -p "$DATA" "$CACHE" "$(dirname "$PUBLISH")"

acquire_lock() {
  if command -v flock >/dev/null 2>&1; then
    exec 9>"$LOCK"
    if ! flock -n 9; then
      echo "refresh already running (lock $LOCK)" >&2
      exit 1
    fi
  else
    if ! mkdir "$LOCK.d" 2>/dev/null; then
      echo "refresh already running (lock $LOCK.d)" >&2
      exit 1
    fi
    trap 'rmdir "$LOCK.d" 2>/dev/null || true' EXIT
  fi
}
acquire_lock

echo "== tail-to-ticker refresh as_of=$AS_OF =="
echo "bin=$BIN data=$DATA cache=$CACHE publish=$PUBLISH"
"$BIN" \
  --data-dir "$DATA" \
  --cache-dir "$CACHE" \
  refresh \
  --as-of "$AS_OF" \
  --overrides "$OVERRIDES/mappings.yaml" \
  --gold "$OVERRIDES/gold.yaml" \
  --aviation-issuers "$OVERRIDES/aviation_issuers.yaml" \
  --issuer-aliases "$OVERRIDES/issuer_aliases.yaml"

SRC="$DATA/snapshots/$AS_OF/tail_to_ticker.sqlite"
test -s "$SRC" || {
  echo "missing snapshot sqlite $SRC" >&2
  exit 1
}
TMP="${PUBLISH}.tmp"
install -m 0644 "$SRC" "$TMP"
mv -f "$TMP" "$PUBLISH"
echo "published → $PUBLISH"
