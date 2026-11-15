#!/usr/bin/env bash
# Membangun bundle lalu membukanya.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/bundle.sh
open dist/my-browser.app
