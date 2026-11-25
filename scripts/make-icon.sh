#!/usr/bin/env bash
# Membuat resources/AppIcon.icns dari resources/icon/make-icon.swift.
# Jalankan ulang setelah mengubah gambar ikon, lalu commit AppIcon.icns.
set -euo pipefail
cd "$(dirname "$0")/.."

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
swift resources/icon/make-icon.swift "$WORK/icon-1024.png" >/dev/null
SET="$WORK/AppIcon.iconset"
mkdir -p "$SET"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$WORK/icon-1024.png" --out "$SET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$WORK/icon-1024.png" --out "$SET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$SET" -o resources/AppIcon.icns
echo "resources/AppIcon.icns"
