#!/usr/bin/env bash
# Mengukur physical footprint aplikasi beserta proses WebKit/helper-nya.
# Pemakaian: scripts/ukur-memory.sh [--detail] <nama-app>...
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"
cargo run -q --release -p memtool -- "$@"
