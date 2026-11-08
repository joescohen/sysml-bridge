#!/usr/bin/env bash
# Fails when the Rust engine holds terms that belong to private or employer-derived work.
# The engine came from a private codebase; this check keeps the public copy clean.
# Positive control: scripts/check-public-content.sh --self-test plants a term and expects failure.
set -euo pipefail
cd "$(dirname "$0")/.."

PATTERN='saic|knowledge-os|\bkos\b|devin|fedramp|mdzip|magicgrid|sysml-studio|jsc61|/home/[a-z]+/|/mnt/c/Users|Joe \('
SCOPE=(crates Cargo.toml)

scan() { grep -rniE "$PATTERN" "$@" || true; }

if [[ "${1:-}" == "--self-test" ]]; then
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  printf '// planted: SAIC rule\n' > "$tmp/planted.rs"
  if [[ -z "$(scan "$tmp")" ]]; then
    echo "SELF-TEST FAILED: the planted term was not found" >&2
    exit 1
  fi
  echo "self-test ok: the planted term was found"
  exit 0
fi

hits="$(scan "${SCOPE[@]}")"
if [[ -n "$hits" ]]; then
  echo "Private-content terms found in the engine:" >&2
  echo "$hits" >&2
  exit 1
fi
echo "public-content check ok: no deny-listed terms in ${SCOPE[*]}"
