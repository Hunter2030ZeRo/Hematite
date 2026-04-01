#!/usr/bin/env bash
set -euo pipefail

APP_DIR="/opt/hematite"
BACKEND_ADDR="127.0.0.1:8989"
BACKEND_WS="ws://${BACKEND_ADDR}/rpc"

cleanup() {
  if [[ -n "${BACKEND_PID:-}" ]]; then
    kill "${BACKEND_PID}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

HEMATITE_BACKEND_ADDR="${BACKEND_ADDR}" "${APP_DIR}/backend" &
BACKEND_PID=$!

sleep 1
HEMATITE_BACKEND_WS="${BACKEND_WS}" "${APP_DIR}/hematite_editor"
