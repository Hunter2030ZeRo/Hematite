#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BACKEND_ADDR="${HEMATITE_BACKEND_ADDR:-127.0.0.1:8989}"
BACKEND_WS="${HEMATITE_BACKEND_WS:-ws://${BACKEND_ADDR}/rpc}"

cleanup() {
  if [[ -n "${BACKEND_PID:-}" ]]; then
    kill "${BACKEND_PID}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

(
  cd "${ROOT_DIR}/backend"
  HEMATITE_BACKEND_ADDR="${BACKEND_ADDR}" cargo run
) &
BACKEND_PID=$!

sleep 1

cd "${ROOT_DIR}/frontend"
flutter pub get
flutter run -d linux --dart-define="HEMATITE_BACKEND_WS=${BACKEND_WS}"
