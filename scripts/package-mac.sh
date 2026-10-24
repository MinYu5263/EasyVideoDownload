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
command -v ditto >/dev/null
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
node node_modules/@tauri-apps/cli/tauri.js build --bundles app,dmg

app_path="src-tauri/target/release/bundle/macos/EasyVideoDownload.app"
shopt -s nullglob
dmgs=(src-tauri/target/release/bundle/dmg/EasyVideoDownload_"$version"_*.dmg)
if [[ ! -d "$app_path" || ${#dmgs[@]} -ne 1 ]]; then
  echo 'Expected one DMG and the macOS application bundle' >&2
  exit 1
fi

case "$(uname -m)" in
  arm64) architecture='arm64' ;;
  x86_64) architecture='x64' ;;
  *) echo 'Unsupported macOS architecture' >&2; exit 1 ;;
esac

app_archive="$output_directory/EasyVideoDownload_${version}_${architecture}.app.zip"
cp -p "${dmgs[0]}" "$output_directory/"
ditto -c -k --sequesterRsrc --keepParent "$app_path" "$app_archive"

for artifact in "$output_directory/$(basename "${dmgs[0]}")" "$app_archive"; do
  if [[ ! -s "$artifact" ]]; then
    echo "Empty package: $artifact" >&2
    exit 1
  fi
  shasum -a 256 "$artifact"
done
