#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${1:-0.1.0}"
APP_NAME="Hematite IDE"
BUILD_DIR="${ROOT_DIR}/build/package/macos"
APP_BUNDLE="${ROOT_DIR}/frontend/build/macos/Build/Products/Release/${APP_NAME}.app"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script must run on macOS." >&2
  exit 1
fi

pushd "${ROOT_DIR}/backend" >/dev/null
cargo build --release
popd >/dev/null

pushd "${ROOT_DIR}/frontend" >/dev/null
flutter pub get
flutter build macos --release
popd >/dev/null

mkdir -p "${BUILD_DIR}"
cp "${ROOT_DIR}/backend/target/release/hematite-backend" "${APP_BUNDLE}/Contents/MacOS/hematite-backend"

if ! command -v create-dmg >/dev/null 2>&1; then
  echo "create-dmg is required. Install with: brew install create-dmg" >&2
  exit 1
fi

create-dmg \
  --volname "Hematite IDE ${VERSION}" \
  --window-pos 200 120 \
  --window-size 800 400 \
  --icon-size 100 \
  --app-drop-link 600 185 \
  "${BUILD_DIR}/hematite-${VERSION}.dmg" \
  "${APP_BUNDLE}"
