#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo 'Run this script on macOS' >&2
  exit 1
fi

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$project_root"
command -v node >/dev/null
command -v npm >/dev/null
if [[ ! -f node_modules/@tauri-apps/cli/tauri.js ]]; then
  echo 'Install project dependencies before packaging' >&2
  exit 1
fi

version="$(node -p 'require("./package.json").version')"
tauri_version_source="$(node -p 'require("./src-tauri/tauri.conf.json").version')"
cargo_version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' src-tauri/Cargo.toml | head -n 1)"
if [[ "$tauri_version_source" != '../package.json' || "$version" != "$cargo_version" ]]; then
  echo 'The Tauri version source and Cargo version must match package.json' >&2
  exit 1
fi

# Clear the previous package output before building
output_directory="$project_root/dist"
if [[ -L "$output_directory" ]]; then
  echo 'The output directory must not be a link' >&2
  exit 1
fi
if [[ -e "$output_directory" ]]; then
  rm -rf -- "$output_directory"
fi

node scripts/generate-icons.mjs
# Skip Finder layout customization to keep packaging non-interactive
CI=true TAURI_BUNDLER_DMG_IGNORE_CI=false \
  node node_modules/@tauri-apps/cli/tauri.js build --ci --bundles dmg

# Export and verify the DMG
shopt -s nullglob
dmgs=(src-tauri/target/release/bundle/dmg/EasyVideoDownload_"$version"_*.dmg)
if [[ ${#dmgs[@]} -ne 1 ]]; then
  echo 'Expected one DMG' >&2
  exit 1
fi

artifact="$output_directory/$(basename "${dmgs[0]}")"
cp -p "${dmgs[0]}" "$artifact"

if [[ ! -s "$artifact" ]]; then
  echo "Empty package: $artifact" >&2
  exit 1
fi
shasum -a 256 "$artifact"
