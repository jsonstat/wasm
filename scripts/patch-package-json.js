#!/usr/bin/env node
// Patches the wasm-pack-generated pkg/package.json to expose the facade as
// the package's main entry while keeping the raw glue reachable via a
// `./glue` subpath.
//
// Usage:  node scripts/patch-package-json.js [pkg/package.json]
//
// `wasm-pack build` regenerates pkg/package.json on every run, resetting it
// to a single-entry shape (main → jsonstat_wasm.js, no exports map). This
// script restores the dual-entry configuration so CDNs and bundlers resolve
// `jsonstat-wasm` to the high-level facade by default. It is idempotent:
// running it twice yields the same result.
//
// The edits applied:
//   - main   → "jsonstat.js"
//   - module → "jsonstat.js"
//   - types  → "jsonstat.d.ts"
//   - files  → prepends "jsonstat.js" and "jsonstat.d.ts"
//   - exports → installs the dual-entry map (facade + glue)
'use strict';

const fs = require('fs');
const path = require('path');

const target = process.argv[2] || path.join('pkg', 'package.json');
const pkg = JSON.parse(fs.readFileSync(target, 'utf8'));

// ── Entry-point fields ───────────────────────────────────────────────────
pkg.main = 'jsonstat.js';
pkg.module = 'jsonstat.js';
pkg.types = 'jsonstat.d.ts';

// ── files: ensure the facade + its types ship ────────────────────────────
const requiredFiles = ['jsonstat.js', 'jsonstat.d.ts'];
pkg.files = Array.isArray(pkg.files) ? pkg.files : [];
for (const f of requiredFiles) {
    if (!pkg.files.includes(f)) {
        pkg.files.unshift(f);
    }
}

// ── exports: dual-entry map (facade default, raw glue via ./glue) ────────
pkg.exports = {
    '.': {
        types: './jsonstat.d.ts',
        import: './jsonstat.js',
    },
    './glue': {
        types: './jsonstat_wasm.d.ts',
        import: './jsonstat_wasm.js',
    },
    // Allow direct CDN file references (jsDelivr/unpkg) without resolution
    // being intercepted by the "." condition.
    './jsonstat.js': './jsonstat.js',
    './jsonstat_wasm.js': './jsonstat_wasm.js',
    './jsonstat_wasm_bg.wasm': './jsonstat_wasm_bg.wasm',
    './package.json': './package.json',
};

fs.writeFileSync(target, JSON.stringify(pkg, null, 2) + '\n', 'utf8');
console.log(`✓ patched ${target} (main → jsonstat.js, dual exports map)`);
