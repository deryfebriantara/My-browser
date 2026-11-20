#!/usr/bin/env bash
# Mendaftarkan server MCP my-browser ke Claude Code (scope user, jadi tersedia
# dari folder mana pun).
# Pemakaian: scripts/pasang-mcp.sh [folder-profil-claude]   (bawaan: ~/.claude)
set -euo pipefail
cd "$(dirname "$0")/.."

BIN="$PWD/dist/my-browser.app/Contents/MacOS/my-browser"
[ -x "$BIN" ] || scripts/bundle.sh
export CLAUDE_CONFIG_DIR="${1:-${CLAUDE_CONFIG_DIR:-$HOME/.claude}}"
CLAUDE="$(command -v claude)" || { echo "perintah claude tidak ditemukan di PATH" >&2; exit 1; }

"$CLAUDE" mcp remove -s user my-browser >/dev/null 2>&1 || true
"$CLAUDE" mcp add -s user my-browser -- "$BIN" --mcp
echo "Terpasang di profil $CLAUDE_CONFIG_DIR. Mulai sesi Claude Code baru supaya tool my-browser muncul."
