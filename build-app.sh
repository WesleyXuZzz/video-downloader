#!/usr/bin/env bash
set -euo pipefail

bundle_target="${1:-dmg}"
release_dir="release"
dmg_bundle_dir="src-tauri/target/release/bundle/dmg"
app_bundle_dir="src-tauri/target/release/bundle/macos"

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

copy_artifacts() {
  local artifact_type="$1"
  local allow_missing="${2:-false}"
  local -a artifacts=()

  case "$artifact_type" in
    dmg)
      artifacts=("$dmg_bundle_dir"/*.dmg)
      ;;
    app)
      artifacts=("$app_bundle_dir"/*.app)
      ;;
  esac

  if ((${#artifacts[@]} == 0)); then
    if [[ "$allow_missing" == "true" ]]; then
      return 0
    fi

    echo "No $artifact_type artifacts found after build." >&2
    exit 1
  fi

  for artifact in "${artifacts[@]}"; do
    local output_path="$release_dir/$(basename "$artifact")"

    if [[ "$artifact_type" == "app" ]]; then
      ditto "$artifact" "$output_path"
    else
      cp -f "$artifact" "$output_path"
    fi

    copied_artifacts+=("$output_path")
  done
}

pnpm install
pnpm tauri build "${build_args[@]}"

mkdir -p "$release_dir"
shopt -s nullglob
copied_artifacts=()

case "$bundle_target" in
  dmg)
    copy_artifacts dmg
    ;;
  app)
    copy_artifacts app
    ;;
  all)
    copy_artifacts dmg true
    copy_artifacts app true
    ;;
esac

if ((${#copied_artifacts[@]} == 0)); then
  echo "No macOS .dmg or .app artifacts found after build." >&2
  exit 1
fi

printf "\nArtifacts copied to:\n"
printf "  %s\n" "${copied_artifacts[@]}"
