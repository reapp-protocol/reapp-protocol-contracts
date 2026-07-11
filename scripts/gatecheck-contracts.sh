#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

for variant in simple composites; do
  contract="$ROOT/contracts/$variant/mandate-registry"
  echo "==> $variant: format"
  cargo fmt --manifest-path "$contract/Cargo.toml" --all -- --check
  echo "==> $variant: lint"
  cargo clippy --manifest-path "$contract/Cargo.toml" --all-targets -- -D warnings
  echo "==> $variant: tests"
  cargo test --manifest-path "$contract/Cargo.toml"
  echo "==> $variant: release WASM"
  cargo build --manifest-path "$contract/Cargo.toml" --target wasm32v1-none --release
done
