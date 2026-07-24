#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

gate_contract() {
  local label="$1"
  local contract="$2"

  echo "==> $label: format"
  cargo fmt --manifest-path "$contract/Cargo.toml" --all -- --check
  echo "==> $label: lint"
  cargo clippy --manifest-path "$contract/Cargo.toml" --all-targets -- -D warnings
  echo "==> $label: tests"
  cargo test --manifest-path "$contract/Cargo.toml"
  echo "==> $label: release WASM"
  cargo build --manifest-path "$contract/Cargo.toml" --target wasm32v1-none --release
}

gate_contract "simple" "$ROOT/contracts/simple/mandate-registry"
gate_contract "composites" "$ROOT/contracts/composites/mandate-registry"
gate_contract \
  "AP2 authorization extension" \
  "$ROOT/contracts/ap2-authorization/mandate-extension"
