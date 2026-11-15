#!/usr/bin/env bash
# Membangun dist/my-browser.app (release, ad-hoc codesign).
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"

cargo build --release -p my-browser
[ -d resources/blocklist ] || scripts/update-filters.sh

APP=dist/my-browser.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/my-browser "$APP/Contents/MacOS/my-browser"
cp resources/Info.plist "$APP/Contents/Info.plist"
mkdir -p "$APP/Contents/Resources/blocklist"
cp resources/blocklist/*.json "$APP/Contents/Resources/blocklist/"

codesign --force --deep -s - "$APP"
echo "Bundle siap: $APP"
