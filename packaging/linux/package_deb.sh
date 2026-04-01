#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${1:-0.1.0}"
ARCH="${2:-amd64}"
PKG_ROOT="${ROOT_DIR}/build/package/linux"
APP_DIR="${PKG_ROOT}/opt/hematite"
BIN_DIR="${PKG_ROOT}/usr/bin"
DESKTOP_DIR="${PKG_ROOT}/usr/share/applications"
ICON_DIR="${PKG_ROOT}/usr/share/icons/hicolor/256x256/apps"

rm -rf "${PKG_ROOT}"
mkdir -p "${APP_DIR}" "${BIN_DIR}" "${DESKTOP_DIR}" "${ICON_DIR}"

pushd "${ROOT_DIR}/backend" >/dev/null
cargo build --release
popd >/dev/null

pushd "${ROOT_DIR}/frontend" >/dev/null
flutter pub get
flutter build linux --release
popd >/dev/null

cp -r "${ROOT_DIR}/frontend/build/linux/x64/release/bundle"/* "${APP_DIR}/"
cp "${ROOT_DIR}/backend/target/release/hematite-backend" "${APP_DIR}/backend"
cp "${ROOT_DIR}/packaging/linux/hematite.sh" "${APP_DIR}/hematite"
cp "${ROOT_DIR}/packaging/linux/hematite.desktop" "${DESKTOP_DIR}/hematite.desktop"

if [[ -f "${ROOT_DIR}/packaging/linux/icon.png" ]]; then
  cp "${ROOT_DIR}/packaging/linux/icon.png" "${ICON_DIR}/hematite.png"
fi

chmod +x "${APP_DIR}/hematite" "${APP_DIR}/backend"
ln -sf /opt/hematite/hematite "${BIN_DIR}/hematite"

if ! command -v fpm >/dev/null 2>&1; then
  echo "fpm is required to build .deb packages. Install with: gem install --no-document fpm" >&2
  exit 1
fi

fpm \
  -s dir \
  -t deb \
  -n hematite \
  -v "${VERSION}" \
  --architecture "${ARCH}" \
  --description "Hematite Flutter + Rust IDE" \
  --maintainer "Hematite Team <dev@hematite.local>" \
  --url "https://example.com/hematite" \
  -C "${PKG_ROOT}" \
  .
