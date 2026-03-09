#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! command -v wasm-pack >/dev/null 2>&1; then
  echo "error: wasm-pack not found in PATH" >&2
  exit 1
fi

wasm-pack build --target web --dev --no-default-features --features wasm

mkdir -p docs/app
cp -f wasm/index.html docs/app/index.html
cp -f pkg/fabbula.js docs/app/fabbula.js
cp -f pkg/fabbula_bg.wasm docs/app/fabbula_bg.wasm

echo "Built and copied to docs/app"
