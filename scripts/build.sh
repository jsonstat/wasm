#!/usr/bin/env bash
# Build the WASM package and apply the facade/entry-point customizations that
# wasm-pack cannot express itself.
#
# wasm-pack regenerates pkg/ from scratch on every build (and pkg/ is
# git-ignored), so the dual-entry configuration and the facade must be
# re-installed after each build. This script makes that reproducible:
#
#   1. wasm-pack build            → generates the raw glue + wasm + types
#   2. stage + minify facade+glue → pkg/*.js (minified) + *.max.js (readable)
#                                   + *.js.map (source maps)
#   3. patch pkg/package.json     → main/module/types/files/exports
#
# Usage:
#   ./scripts/build.sh                 # release build (default)
#   ./scripts/build.sh --dev           # debug build (no --release)
#   TARGET=node ./scripts/build.sh     # node target (shorthand for nodejs)
set -euo pipefail

# ── Config ───────────────────────────────────────────────────────────────
# Repo root = directory containing this script's parent.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${TARGET:-web}"
# `wasm-pack --target` requires the canonical name `nodejs`; accept the shorter
# `node` shorthand used throughout this repo's docs and map it through.
if [[ "${TARGET}" == "node" ]]; then
    TARGET="nodejs"
fi
PKG_DIR="${ROOT}/pkg"

# Pinned esbuild version for the minify step (step 2 below). Pulled fresh via
# npx — no committed package.json dependency — matching the tsc pattern in CI.
ESBUILD_VERSION="0.25.0"

# ── Parse flags ──────────────────────────────────────────────────────────
RELEASE_FLAG="--release"
if [[ "${1:-}" == "--dev" || "${1:-}" == "--debug" ]]; then
    RELEASE_FLAG=""
fi

# ── 1. Build ─────────────────────────────────────────────────────────────
echo "▶ wasm-pack build --target ${TARGET} ${RELEASE_FLAG}"
cd "${ROOT}"
wasm-pack build --target "${TARGET}" ${RELEASE_FLAG}

# ── 2. Stage the facade + glue, and minify each ──────────────────────────
# Naming convention: the bare name (jsonstat.js, jsonstat_wasm.js) is the
# MINIFIED file and the default entry point (main/module/exports point at
# it). The readable, non-minified source is published alongside as a
# `.max.js` sibling for debugging, and each minified file ships with a
# `.js.map` whose `sources` reference that co-located `.max.js`.
#
# The facade imports './jsonstat_wasm.js', so the glue MUST be staged first.
#
# esbuild preserves each target's native module format (no --format flag):
# the web target's glue is ESM (locates its `.wasm` via `import.meta.url`),
# the nodejs target's glue is CommonJS (`module.exports` + `require`).
# Forcing --format=esm would convert the CJS glue to ESM and collapse its
# named exports into a single default export, breaking the local Node test
# harnesses (`.idea/test/*.mjs`), which import `{ JSONstat }` by name.
# --minify renames only locals and keeps the module's exported bindings
# (`JSONstat`, `version`, `init_panic_hook`, default `init`), so the facade's
# named imports and the `./glue` subpath keep resolving unchanged.

# 2a. Glue: wasm-pack emits pkg/jsonstat_wasm.js — keep it as the readable
# `.max.js`, then write the minified output back to the bare name.
echo "▶ staging + minifying glue → ${PKG_DIR}/jsonstat_wasm.js"
mv "${PKG_DIR}/jsonstat_wasm.js" "${PKG_DIR}/jsonstat_wasm.max.js"
npx --yes -p "esbuild@${ESBUILD_VERSION}" esbuild \
    "${PKG_DIR}/jsonstat_wasm.max.js" \
    --minify --sourcemap \
    --outfile="${PKG_DIR}/jsonstat_wasm.js"

# 2b. Facade: copy the committed readable source as the `.max.js`, then
# minify to the bare name. The types are not minified (not runtime code).
echo "▶ staging + minifying facade → ${PKG_DIR}/jsonstat.js"
cp "${ROOT}/jsonstat.js"      "${PKG_DIR}/jsonstat.max.js"
cp "${ROOT}/jsonstat.d.ts"    "${PKG_DIR}/jsonstat.d.ts"
npx --yes -p "esbuild@${ESBUILD_VERSION}" esbuild \
    "${PKG_DIR}/jsonstat.max.js" \
    --minify --sourcemap \
    --outfile="${PKG_DIR}/jsonstat.js"

# ── 3. Patch pkg/package.json for the dual-entry layout ──────────────────
echo "▶ patching ${PKG_DIR}/package.json"
node "${ROOT}/scripts/patch-package-json.js" "${PKG_DIR}/package.json"

echo ""
echo "✓ pkg/ built and customized:"
echo "    main   → jsonstat.js      (high-level facade, minified)"
echo "    /glue  → jsonstat_wasm.js (low-level glue, minified)"
echo "    source → *.max.js + *.js.map (readable source + maps, also shipped)"
echo ""
echo "  Publish with:  wasm-pack publish"
echo "  Or local link: npm link ./pkg"
