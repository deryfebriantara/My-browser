#!/usr/bin/env bash
# Mengunduh EasyList + EasyPrivacy lalu membuat resources/blocklist/*.json.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
mkdir -p resources/filters
curl -fsSL https://easylist.to/easylist/easylist.txt -o resources/filters/easylist.txt
curl -fsSL https://easylist.to/easylist/easyprivacy.txt -o resources/filters/easyprivacy.txt
cargo run -q --release -p blocklist-gen -- resources/filters resources/blocklist
