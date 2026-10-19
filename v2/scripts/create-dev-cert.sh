#!/usr/bin/env bash
# Create a self-signed code-signing certificate for stable local TCC grants.
# For personal use only; do not ship apps signed with this certificate.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NAME="${1:-Xiaomi Remote Dev}"
KEYCHAIN="${2:-$HOME/Library/Keychains/login.keychain-db}"
APP="${3:-$ROOT/src-tauri/target/release/bundle/macos/Xiaomi Remote.app}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

cat > "$TMP/openssl.cnf" <<CNF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no

[dn]
CN = $NAME

[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CNF

openssl req -x509 -newkey rsa:2048 -sha256 -days 3650 -nodes \
  -keyout "$TMP/key.pem" -out "$TMP/cert.pem" -config "$TMP/openssl.cnf"

openssl pkcs12 -export -out "$TMP/cert.p12" \
  -inkey "$TMP/key.pem" -in "$TMP/cert.pem" -passout pass:

security import "$TMP/cert.p12" -k "$KEYCHAIN" -P "" -T /usr/bin/codesign
security add-trusted-cert -r trustRoot -k "$KEYCHAIN" "$TMP/cert.pem" || true

security find-identity -v -p codesigning

cat <<MSG

Certificate "$NAME" is available for codesign.

Sign the built app:
  codesign --force --deep --sign "$NAME" --identifier com.xiaomiremote.app "$APP"

Then grant Accessibility once; rebuilding with the same certificate keeps the
TCC grant associated.
MSG
