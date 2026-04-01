#!/usr/bin/env bash
set -euo pipefail

TARGET="${1:-linux-deb}"
VERSION="${2:-0.1.0}"

case "${TARGET}" in
  linux-deb)
    "$(dirname "$0")/linux/package_deb.sh" "${VERSION}"
    ;;
  macos-dmg)
    "$(dirname "$0")/macos/package_dmg.sh" "${VERSION}"
    ;;
  windows-msi)
    pwsh "$(dirname "$0")/windows/package_msi.ps1" -Version "${VERSION}"
    ;;
  *)
    echo "Unsupported target: ${TARGET}" >&2
    echo "Use one of: linux-deb | macos-dmg | windows-msi" >&2
    exit 1
    ;;
esac
