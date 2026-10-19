#!/usr/bin/env bash
# Build and install Xiaomi Remote into /Applications.
#
# Usage: ./scripts/install-macos.sh [--skip-build]
# Rebuilding changes the ad-hoc signature; macOS may ask you to re-approve the
# keyboard-control permission after installation.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ "${1:-}" != "--skip-build" ]]; then
  pnpm tauri build
fi

APP="$ROOT/src-tauri/target/release/bundle/macos/Xiaomi Remote.app"
if [[ ! -d "$APP" ]]; then
  echo "App bundle not found: $APP" >&2
  exit 1
fi

pkill -f "/Applications/Xiaomi Remote.app/Contents/MacOS/xiaomi-remote-v2" 2>/dev/null || true
sleep 1

rm -rf "/Applications/Xiaomi Remote.app"
ditto "$APP" "/Applications/Xiaomi Remote.app"
open "/Applications/Xiaomi Remote.app"

echo
echo "Installed /Applications/Xiaomi Remote.app"
echo "If the status shows 4/5 or 'hidutil', re-approve Xiaomi Remote in"
echo "System Settings → Privacy & Security → Device Control and Data Access."
