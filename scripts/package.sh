#!/usr/bin/env bash
# Mengemas dist/my-browser.app untuk diunduh: DMG (seret ke Applications) dan zip.
# Pemakaian: scripts/package.sh [versi]   (bawaan: CFBundleShortVersionString di Info.plist)
set -euo pipefail
cd "$(dirname "$0")/.."

APP=dist/my-browser.app
[ -d "$APP" ] || scripts/bundle.sh
VERSION="${1:-$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' resources/Info.plist)}"
NAME="my-browser-${VERSION}-macos-arm64"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"

rm -f "dist/$NAME.dmg" "dist/$NAME.zip"
hdiutil create -quiet -volname "my-browser" -srcfolder "$STAGE" -ov -format UDZO "dist/$NAME.dmg"
ditto -c -k --keepParent "$APP" "dist/$NAME.zip"
echo "dist/$NAME.dmg"
echo "dist/$NAME.zip"
