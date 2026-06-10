#!/usr/bin/env bash
# render.sh — render SysML v2 graphical views from a .sysml file to PDF (and
# optionally PNG), using the vendored decisym-viewer Rust renderer.
#
# Usage:
#   render.sh <file.sysml> <out-dir> [--spec views.json] [--png]
#
# Options:
#   --spec views.json   JSON file describing which views to render (see README
#                       for schema).  Without --spec, all 11 default ANGARS
#                       views are rendered.
#   --png               After a successful export run, rasterize each produced
#                       PDF to PNG via pdftoppm -r 150.  Requires poppler-utils
#                       (pdftoppm).
#
# Toolchain prerequisite:
#   Rust 1.96.0, pinned via tools/decisym-viewer/rust-toolchain.toml.
#   Install / update with: rustup toolchain install 1.96.0
#   rustup is available at: https://rustup.rs/
#
# Exit codes:
#   0  success
#   1  render error (export_figures reported failures or zero views produced)
#   2  prerequisite missing (cargo or pdftoppm not found, or input file missing)
set -euo pipefail

# ── Resolve repo root ────────────────────────────────────────────────────────
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ -z "${REPO_ROOT}" ]; then
    # Fall back to two levels up from this script (tools/decisym-viewer/ -> repo root).
    REPO_ROOT="$(cd "${HERE}/../.." && pwd)"
fi

VIEWER_DIR="${HERE}"
MANIFEST="${VIEWER_DIR}/Cargo.toml"
BINARY="${VIEWER_DIR}/target/release/export_figures"

# ── Parse arguments ──────────────────────────────────────────────────────────
if [ $# -lt 2 ]; then
    echo "usage: render.sh <file.sysml> <out-dir> [--spec views.json] [--png]" >&2
    exit 2
fi

INPUT_FILE="${1}"
OUTPUT_DIR="${2}"
shift 2

SPEC_FILE=""
DO_PNG=false

while [ $# -gt 0 ]; do
    case "$1" in
        --spec)
            shift
            if [ $# -eq 0 ]; then
                echo "error: --spec requires a path argument" >&2
                exit 2
            fi
            SPEC_FILE="${1}"
            ;;
        --png)
            DO_PNG=true
            ;;
        *)
            echo "error: unexpected argument: ${1}" >&2
            echo "usage: render.sh <file.sysml> <out-dir> [--spec views.json] [--png]" >&2
            exit 2
            ;;
    esac
    shift
done

# ── Validate inputs ──────────────────────────────────────────────────────────
if [ ! -f "${INPUT_FILE}" ]; then
    echo "error: input file not found: ${INPUT_FILE}" >&2
    echo "  Pass a valid .sysml file as the first argument." >&2
    exit 2
fi

# ── Check prerequisites ───────────────────────────────────────────────────────
if ! command -v cargo >/dev/null 2>&1; then
    cat >&2 <<EOF
error: cargo not found on PATH.

The decisym-viewer renderer requires Rust ${RUST_VERSION:-1.96.0} (edition 2024).
Install the Rust toolchain via rustup:

  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

Then install the pinned toolchain used by this project:

  rustup toolchain install 1.96.0

After installation, re-run:
  tools/decisym-viewer/render.sh <file.sysml> <out-dir>
EOF
    exit 2
fi

if [ "${DO_PNG}" = true ] && ! command -v pdftoppm >/dev/null 2>&1; then
    cat >&2 <<EOF
error: pdftoppm not found on PATH but --png was requested.

Install poppler-utils (provides pdftoppm):
  Ubuntu/Debian: sudo apt-get install -y poppler-utils
  macOS:         brew install poppler
  Fedora:        sudo dnf install -y poppler-utils

Then re-run with --png.
EOF
    exit 2
fi

# ── Build the binary if needed ───────────────────────────────────────────────
if [ ! -f "${BINARY}" ]; then
    echo "info: building export_figures (release)..." >&2
    cargo build --release --bin export_figures --manifest-path "${MANIFEST}"
fi

# ── Run the renderer ─────────────────────────────────────────────────────────
mkdir -p "${OUTPUT_DIR}"

EXPORT_ARGS=("${INPUT_FILE}" "${OUTPUT_DIR}")
if [ -n "${SPEC_FILE}" ]; then
    EXPORT_ARGS+=(--spec "${SPEC_FILE}")
fi

"${BINARY}" "${EXPORT_ARGS[@]}"
EXPORT_EXIT=$?

if [ "${EXPORT_EXIT}" -ne 0 ]; then
    exit "${EXPORT_EXIT}"
fi

# ── Rasterize to PNG if requested ─────────────────────────────────────────────
if [ "${DO_PNG}" = true ]; then
    shopt -s nullglob
    PDFS=("${OUTPUT_DIR}"/*.pdf)
    shopt -u nullglob
    if [ "${#PDFS[@]}" -eq 0 ]; then
        echo "warning: --png requested but no PDF files found in ${OUTPUT_DIR}" >&2
    fi
    for pdf in "${PDFS[@]}"; do
        stem="${pdf%.pdf}"
        pdftoppm -png -r 150 "${pdf}" "${stem}"
    done
fi
