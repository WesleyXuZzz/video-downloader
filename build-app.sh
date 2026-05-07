#!/usr/bin/env bash
set -euo pipefail

bundle_target="${1:-dmg}"

case "$bundle_target" in
  dmg|app)
    build_args=(--bundles "$bundle_target")
    ;;
  all)
    build_args=()
    ;;
  *)
    echo "Usage: ./build-app.sh [dmg|app|all]"
    echo "  dmg  Build the macOS DMG installer package. This is the default."
    echo "  app  Build only the macOS .app bundle."
    echo "  all  Build all bundle targets configured by Tauri."
    exit 2
    ;;
esac

pnpm install
pnpm tauri build "${build_args[@]}"
