#!/usr/bin/env bash
# Wrapper that runs the local SysML v2 grammar validator using the repo .venv.
#
# Usage: tools/sysml-validator/run.sh path/to/model.sysml [more.sysml ...]
#
# Requires the repo virtualenv at <repo>/.venv with the runtime dep installed:
#   python -m venv .venv
#   .venv/bin/pip install -r tools/sysml-validator/requirements.txt
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ -z "${REPO_ROOT}" ]; then
  # Fall back to two levels up from this script (tools/sysml-validator/ -> repo root).
  REPO_ROOT="$(cd "${HERE}/../.." && pwd)"
fi

VENV_PY="${REPO_ROOT}/.venv/bin/python"

if [ ! -x "${VENV_PY}" ]; then
  cat >&2 <<EOF
error: repo virtualenv not found at ${VENV_PY}

Set it up once with:
  python -m venv "${REPO_ROOT}/.venv"
  "${REPO_ROOT}/.venv/bin/pip" install -r "${HERE}/requirements.txt"

Then re-run:
  tools/sysml-validator/run.sh path/to/model.sysml
EOF
  exit 2
fi

exec "${VENV_PY}" "${HERE}/validate_sysml.py" "$@"
