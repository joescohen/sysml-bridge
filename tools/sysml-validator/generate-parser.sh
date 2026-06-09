#!/usr/bin/env bash
# Regenerate the committed Python parser in generated/ from the vendored grammar.
#
# This is a ONE-TIME / on-grammar-change step. The canonical, committed artifact
# is tools/sysml-validator/generated/ -- you do NOT need to run this to validate
# (see run.sh / README.md). You only need it after editing the .g4 grammar.
#
# Requirements (build time only):
#   - Java (JRE/JDK 11+). A private JRE auto-installed by the toolchain is
#     expected at ~/.jre -- this script prepends ~/.jre/bin to PATH if present.
#   - The ANTLR 4.13.2 complete JAR. Override its location with ANTLR_JAR=...,
#     otherwise common locations are probed.
#
# Source grammar (vendored, OMG/ANTLR, MIT):
#   docs/sysml-v2-reference/grammar/SysMLv2Lexer.g4
#   docs/sysml-v2-reference/grammar/SysMLv2Parser.g4
#
# Best-effort: if Java or the ANTLR jar are missing, it prints a clear hint and
# exits non-zero WITHOUT touching the committed generated/ artifact.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || cd "${HERE}/../.." && pwd)"

GRAMMAR_DIR="${REPO_ROOT}/docs/sysml-v2-reference/grammar"
OUT_DIR="${HERE}/generated"
ANTLR_VERSION="4.13.2"

# Prefer the private JRE if present.
if [ -d "${HOME}/.jre/bin" ]; then
  export PATH="${HOME}/.jre/bin:${PATH}"
fi

if ! command -v java >/dev/null 2>&1; then
  cat >&2 <<EOF
error: 'java' not found. This regen step needs a JRE/JDK (build time only).
A private JRE is expected at ~/.jre (auto-installed by the toolchain); ensure
~/.jre/bin/java exists, or install Java another way. Validation itself needs no
Java -- the committed generated/ parser is used at validate time.
EOF
  exit 1
fi

# Locate the ANTLR complete jar.
if [ -z "${ANTLR_JAR:-}" ]; then
  for cand in \
    "${HOME}/.local/lib/antlr-${ANTLR_VERSION}-complete.jar" \
    "/usr/local/lib/antlr-${ANTLR_VERSION}-complete.jar" \
    "/usr/local/lib/antlr-4.13.2-complete.jar" \
    "${HERE}/antlr-${ANTLR_VERSION}-complete.jar"; do
    if [ -f "${cand}" ]; then ANTLR_JAR="${cand}"; break; fi
  done
fi

if [ -z "${ANTLR_JAR:-}" ] || [ ! -f "${ANTLR_JAR}" ]; then
  cat >&2 <<EOF
error: ANTLR ${ANTLR_VERSION} complete jar not found.
Download it and set ANTLR_JAR, e.g.:
  curl -L -o /usr/local/lib/antlr-${ANTLR_VERSION}-complete.jar \\
    https://www.antlr.org/download/antlr-${ANTLR_VERSION}-complete.jar
  ANTLR_JAR=/usr/local/lib/antlr-${ANTLR_VERSION}-complete.jar tools/sysml-validator/generate-parser.sh
EOF
  exit 1
fi

if [ ! -f "${GRAMMAR_DIR}/SysMLv2Lexer.g4" ] || [ ! -f "${GRAMMAR_DIR}/SysMLv2Parser.g4" ]; then
  echo "error: grammar not found in ${GRAMMAR_DIR}" >&2
  exit 1
fi

echo "Using java: $(command -v java)"
echo "Using ANTLR jar: ${ANTLR_JAR}"
echo "Grammar dir: ${GRAMMAR_DIR}"
echo "Output dir:  ${OUT_DIR}"

mkdir -p "${OUT_DIR}"
# Generate the Python3 lexer + parser + listener into generated/.
java -jar "${ANTLR_JAR}" \
  -Dlanguage=Python3 \
  -o "${OUT_DIR}" \
  -lib "${GRAMMAR_DIR}" \
  "${GRAMMAR_DIR}/SysMLv2Lexer.g4" \
  "${GRAMMAR_DIR}/SysMLv2Parser.g4"

# ANTLR mirrors the input path under -o; flatten any nested output back into generated/.
find "${OUT_DIR}" -mindepth 2 -type f -name 'SysMLv2*' -exec mv -f {} "${OUT_DIR}/" \; 2>/dev/null || true
find "${OUT_DIR}" -mindepth 1 -type d -empty -delete 2>/dev/null || true

echo "Done. Regenerated parser in ${OUT_DIR}."
