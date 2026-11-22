#!/usr/bin/env bash
# Pemeriksaan sebelum commit atau rilis.
#   scripts/check.sh          format, clippy tanpa peringatan, semua test
#                             (termasuk smoke test WebKit), audit dependensi
#   scripts/check.sh --quick  format dan clippy saja (dipakai git hook)
set -euo pipefail
cd "$(dirname "$0")/.."

quick=0
[[ "${1:-}" == "--quick" ]] && quick=1

echo "== rustfmt"
cargo fmt --all -- --check

echo "== clippy"
cargo clippy --workspace --all-targets --locked -- -D warnings

if (( quick )); then
    exit 0
fi

echo "== test"
cargo test --workspace --locked

echo "== audit dependensi"
if ! command -v cargo-audit >/dev/null 2>&1; then
    echo "cargo-audit belum terpasang: cargo install cargo-audit --locked" >&2
    exit 1
fi
cargo audit

echo "Semua pemeriksaan lulus."
