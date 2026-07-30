#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$repo_root"
cargo install --path apps/sixpack

echo
echo "sixpack installed"
echo "next: run 'sixpack create' for a guided starter or 'sixpack init' with schema.sixpack"
