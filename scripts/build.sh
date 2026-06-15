#!/usr/bin/env bash
# Build the WASM package and apply the facade/entry-point customizations that
# wasm-pack cannot express itself.
#
# wasm-pack regenerates pkg/ from scratch on every build (and pkg/ is
# git-ignored), so the dual-entry configuration and the facade must be
# re-installed after each build. This script makes that reproducible:
#
#   1. wasm-pack build            → generates the raw glue + wasm + types
#   2. copy facade + types        → pkg/jsonstat.js, pkg/jsonstat.d.ts
#   3. patch pkg/package.json     → main/module/types/files/exports
#
# Usage:
#   ./scripts/build.sh                 # release build (default)
#   ./scripts/build.sh --dev           # debug build (no --release)
#   TARGET=node ./scripts/build.sh     # override --target (default: web)
set -euo pipefail

# ── Config ───────────────────────────────────────────────────────────────
# Repo root = directory containing this script's parent.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${TARGET:-web}"
PKG_DIR="${ROOT}/pkg"

# ── Parse flags ──────────────────────────────────────────────────────────
RELEASE_FLAG="--release"
if [[ "${1:-}" == "--dev" || "${1:-}" == "--debug" ]]; then
    RELEASE_FLAG=""
fi

# ── 1. Build ─────────────────────────────────────────────────────────────
echo "▶ wasm-pack build --target ${TARGET} ${RELEASE_FLAG}"
cd "${ROOT}"
wasm-pack build --target "${TARGET}" ${RELEASE_FLAG}

# ── 2. Copy the committed facade + its types into pkg/ ───────────────────
# The facade imports './jsonstat_wasm.js' (co-located), so it MUST sit next
# to the generated glue for the relative import to resolve on a CDN.
echo "▶ copying facade + types into ${PKG_DIR}/"
cp "${ROOT}/jsonstat.js"      "${PKG_DIR}/jsonstat.js"
cp "${ROOT}/jsonstat.d.ts"    "${PKG_DIR}/jsonstat.d.ts"

# ── 3. Patch pkg/package.json for the dual-entry layout ──────────────────
echo "▶ patching ${PKG_DIR}/package.json"
node "${ROOT}/scripts/patch-package-json.js" "${PKG_DIR}/package.json"

echo ""
echo "✓ pkg/ built and customized:"
echo "    main   → jsonstat.js   (high-level facade)"
echo "    /glue  → jsonstat_wasm.js (low-level glue)"
echo ""
echo "  Publish with:  wasm-pack publish"
echo "  Or local link: npm link ./pkg"
