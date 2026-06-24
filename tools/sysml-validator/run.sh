#!/usr/bin/env bash
# Wrapper that runs the local SysML v2 grammar validator using the repo .venv.
#
# Usage: tools/sysml-validator/run.sh path/to/model.sysml [more.sysml ...]
#
# The repo virtualenv lives at <repo>/.venv with one runtime dep
# (antlr4-python3-runtime, see requirements.txt). If it is missing this script
# bootstraps it automatically on first run, so `pnpm validate:sysml <file>`
# works on a fresh clone with nothing but Node + Python 3 installed.
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ -z "${REPO_ROOT}" ]; then
  # Fall back to two levels up from this script (tools/sysml-validator/ -> repo root).
  REPO_ROOT="$(cd "${HERE}/../.." && pwd)"
fi

VENV_PY="${REPO_ROOT}/.venv/bin/python"

if [ ! -x "${VENV_PY}" ]; then
  # Auto-bootstrap the venv on first use. Pick a python3 to seed it.
  PYTHON_BIN="$(command -v python3 || command -v python || true)"
  if [ -z "${PYTHON_BIN}" ]; then
    cat >&2 <<EOF
error: python3 not found on PATH — the SysML v2 validator needs Python 3.

Install Python 3, then re-run:
  tools/sysml-validator/run.sh path/to/model.sysml
EOF
    exit 2
  fi
  echo "sysml-validator: bootstrapping virtualenv at ${REPO_ROOT}/.venv (first run)…" >&2
  "${PYTHON_BIN}" -m venv "${REPO_ROOT}/.venv" >&2
  "${REPO_ROOT}/.venv/bin/pip" install --quiet --disable-pip-version-check \
    -r "${HERE}/requirements.txt" >&2
  echo "sysml-validator: virtualenv ready." >&2
fi

exec "${VENV_PY}" "${HERE}/validate_sysml.py" "$@"
