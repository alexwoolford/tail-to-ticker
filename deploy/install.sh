#!/usr/bin/env bash
# Install tail-to-ticker under /opt and enable systemd (Linux).
# Usage (as root): ./deploy/install.sh
#
# Prefer building the release binary as a normal user first:
#   cargo build --release
#   sudo ./deploy/install.sh
# Set FORCE_REBUILD=1 to rebuild even when target/release/tail-to-ticker exists.
#
# Optional inputs (do not overwrite existing host copies unless FORCE_*=1):
#   TAIL_ENV_FILE     populated env (chmod 600); used only if dest is missing
#   TAIL_SEED_SQLITE  seed published current/ sqlite if dest is missing
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${TAIL_INSTALL_PREFIX:-/opt/tail-to-ticker}"
STATE="${TAIL_STATE_DIR:-/var/lib/tail-to-ticker}"
USER_NAME="${TAIL_RUN_USER:-tails}"
GROUP_NAME="${TAIL_RUN_GROUP:-$USER_NAME}"
BIN_SRC="$ROOT/target/release/tail-to-ticker"
ENV_DST="$PREFIX/etc/tail-to-ticker.env"
PUBLISH_DST="$STATE/current/tail_to_ticker.sqlite"
JOURNAL_MAPPING="${TAIL_JOURNAL_MAPPING:-/var/lib/adsb-trip-journal/mapping/tail_to_ticker.sqlite}"

if [[ "$(id -u)" -ne 0 ]]; then
  echo "run as root" >&2
  exit 1
fi

build_release() {
  local build_user="${SUDO_USER:-}"
  if [[ -n "$build_user" && "$build_user" != "root" ]] && id -u "$build_user" >/dev/null 2>&1; then
    echo "== build release (as $build_user) =="
    sudo -u "$build_user" -H bash -lc "cd \"$ROOT\" && source \"\$HOME/.cargo/env\" 2>/dev/null || true; cargo build --release"
    return
  fi
  echo "no release binary at $BIN_SRC and no non-root SUDO_USER to build as." >&2
  echo "build first: cargo build --release" >&2
  echo "then re-run: sudo ./deploy/install.sh" >&2
  exit 1
}

if [[ -x "$BIN_SRC" && -z "${FORCE_REBUILD:-}" ]]; then
  echo "== using existing release binary: $BIN_SRC =="
else
  build_release
fi

test -x "$BIN_SRC" || {
  echo "missing $BIN_SRC — build with: cargo build --release" >&2
  exit 1
}

echo "== create user/dirs =="
NLOGIN="/usr/sbin/nologin"
[[ -x "$NLOGIN" ]] || NLOGIN="/sbin/nologin"
if ! id -u "$USER_NAME" >/dev/null 2>&1; then
  useradd --system --home-dir "$STATE" --shell "$NLOGIN" "$USER_NAME" || true
fi
mkdir -p "$PREFIX"/{bin,scripts,etc,docs,overrides} \
  "$STATE"/work/current \
  "$STATE"/work/snapshots \
  "$STATE"/cache \
  "$STATE"/current \
  /etc/systemd/system

echo "== install files =="
install -m 0755 "$BIN_SRC" "$PREFIX/bin/tail-to-ticker"
install -m 0755 "$ROOT/scripts/run-refresh.sh" "$PREFIX/scripts/run-refresh.sh"
install -m 0644 "$ROOT/docs/DAILY_OPS.md" "$PREFIX/docs/DAILY_OPS.md"
install -m 0644 "$ROOT/overrides/mappings.yaml" "$PREFIX/overrides/mappings.yaml"
install -m 0644 "$ROOT/overrides/gold.yaml" "$PREFIX/overrides/gold.yaml"
install -m 0644 "$ROOT/overrides/aviation_issuers.yaml" "$PREFIX/overrides/aviation_issuers.yaml"

if [[ ! -f "$ENV_DST" ]]; then
  if [[ -n "${TAIL_ENV_FILE:-}" && -f "$TAIL_ENV_FILE" ]]; then
    install -m 0600 "$TAIL_ENV_FILE" "$ENV_DST"
  else
    install -m 0600 "$ROOT/deploy/tail-to-ticker.env.example" "$ENV_DST"
  fi
fi
chmod 0600 "$ENV_DST"

if [[ ! -f "$PUBLISH_DST" ]]; then
  seed="${TAIL_SEED_SQLITE:-}"
  if [[ -n "$seed" && -f "$seed" ]]; then
    echo "== seed published sqlite from $seed =="
    install -m 0644 "$seed" "$PUBLISH_DST"
  elif [[ -f "$JOURNAL_MAPPING" ]]; then
    echo "== seed published sqlite from journal copy $JOURNAL_MAPPING =="
    install -m 0644 "$JOURNAL_MAPPING" "$PUBLISH_DST"
  fi
fi

chown -R "$USER_NAME:$GROUP_NAME" "$STATE"
chown -R root:root "$PREFIX"
chown root:"$GROUP_NAME" "$PREFIX/etc" "$ENV_DST"
chmod 0750 "$PREFIX/etc"
chmod 0600 "$ENV_DST"
chmod 0755 "$PREFIX" "$PREFIX/bin" "$PREFIX/scripts" "$PREFIX/docs" "$PREFIX/overrides"
chmod 0755 "$PREFIX/scripts"/*.sh
# Journal user `adsb` must traverse here to open current/*.sqlite (ProtectHome=true).
chmod 0755 "$STATE" "$STATE/current"
chmod 0750 "$STATE/work" "$STATE/cache" 2>/dev/null || true
if [[ -f "$PUBLISH_DST" ]]; then
  chmod 0644 "$PUBLISH_DST"
fi

install -m 0644 "$ROOT/deploy/systemd/tail-to-ticker-refresh.service" \
  /etc/systemd/system/tail-to-ticker-refresh.service
install -m 0644 "$ROOT/deploy/systemd/tail-to-ticker-refresh.timer" \
  /etc/systemd/system/tail-to-ticker-refresh.timer

if command -v restorecon >/dev/null 2>&1; then
  echo "== SELinux restorecon =="
  restorecon -Rv "$PREFIX" "$STATE" || true
fi

systemctl daemon-reload

ua_ok=0
if [[ -f "$ENV_DST" ]]; then
  ua="$(grep -E '^SEC_USER_AGENT=' "$ENV_DST" | tail -n1 | cut -d= -f2- || true)"
  ua="${ua%\"}"
  ua="${ua#\"}"
  if [[ -n "$ua" ]] && ! grep -qi 'example.com' <<<"$ua"; then
    ua_ok=1
  fi
fi

echo "installed:"
echo "  prefix=$PREFIX state=$STATE"
echo "  publish: $PUBLISH_DST"
echo "  logs: journalctl -u tail-to-ticker-refresh.service"
echo "  edit: $ENV_DST (chmod 600)"

if [[ "$ua_ok" -eq 1 ]]; then
  systemctl enable --now tail-to-ticker-refresh.timer
  echo "  timer: tail-to-ticker-refresh.timer enabled (daily 07:00 UTC + 15m jitter)"
else
  echo "  SEC_USER_AGENT is empty or still example.com — timer not enabled."
  echo "  Fill $ENV_DST, then:"
  echo "    sudo systemctl enable --now tail-to-ticker-refresh.timer"
fi
