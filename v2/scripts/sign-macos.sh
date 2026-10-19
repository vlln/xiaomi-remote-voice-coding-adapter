#!/usr/bin/env bash
# Ad-hoc sign the built app with a stable identifier so macOS TCC
# (Accessibility) grants can survive rebuilds.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="${1:-$ROOT/src-tauri/target/release/bundle/macos/Xiaomi Remote.app}"
IDENTIFIER="${2:-com.xiaomiremote.app}"

if [[ ! -d "$APP" ]]; then
  echo "app bundle not found: $APP" >&2
  echo "run: pnpm tauri build" >&2
  exit 1
fi

codesign --force --deep --sign - --identifier "$IDENTIFIER" "$APP"
codesign -dv --verbose=2 "$APP" 2>&1 | sed -n '1,8p'
