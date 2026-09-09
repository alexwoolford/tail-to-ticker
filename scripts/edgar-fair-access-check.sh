#!/usr/bin/env bash
# One GET to company_tickers_exchange.json. Prints HTTP status and Server.
# Distinguishes undeclared-UA 403 from AkamaiGHost on this IP. Does not harvest.
#
# SEC sample shape works:  User-Agent: tail-to-ticker you@real-domain
# The github-paren form (tail-to-ticker/0.1 (https://github.com/...; email)) is
# treated as an undeclared bot (403 AkamaiGHost) even from a residential IP.
set -euo pipefail

UA="${SEC_USER_AGENT:-}"
if [[ -z "$UA" ]] || echo "$UA" | grep -qi 'example.com'; then
  echo "Set SEC_USER_AGENT to a real descriptive contact (not example.com) before contacting SEC." >&2
  echo "SEC sample shape: tail-to-ticker you@real-domain" >&2
  echo "Do not use the github-paren form; Akamai 403s it as undeclared." >&2
  exit 1
fi

URL="https://www.sec.gov/files/company_tickers_exchange.json"
tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
headers="$tmpdir/headers"
body="$tmpdir/body"

set +e
http_code="$(
  curl -sS -o "$body" -D "$headers" -w "%{http_code}" \
    --max-time 60 \
    -4 --http1.1 \
    -A "$UA" \
    -H "Accept-Encoding: gzip, deflate" \
    -H "Accept-Language: en-US,en;q=0.9" \
    -H "Accept: */*" \
    "$URL"
)"
curl_rc=$?
set -e

if [[ "$curl_rc" -ne 0 ]]; then
  echo "curl failed (exit $curl_rc)" >&2
  exit "$curl_rc"
fi

server="$(python3 -c '
from pathlib import Path
import sys
text = Path(sys.argv[1]).read_text(errors="replace")
for line in text.splitlines():
    if line.lower().startswith("server:"):
        print(line.split(":", 1)[1].strip().strip("\r"))
        break
' "$headers")"
echo "HTTP $http_code"
echo "Server: ${server:-unknown}"

if [[ "$http_code" == "200" ]]; then
  exit 0
fi
exit 1
