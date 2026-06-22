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
//   - main   → "jsonstat.js"   (minified facade — default entry)
//   - module → "jsonstat.js"
//   - types  → "jsonstat.d.ts"
//   - files  → prepends the minified defaults (jsonstat.js, jsonstat_wasm.js),
//              the readable non-minified source (jsonstat.max.js,
//              jsonstat_wasm.max.js), the source maps (*.js.map), and
//              jsonstat.d.ts so all artifacts ship to npm
//   - exports → installs the dual-entry map (facade + glue), plus passthrough
//              entries for the .max.js and .map files so strict resolvers
//              (and the sourceMappingURL comment) can reach them
//   - repository.url → canonicalized to git+https form (npm expects this)
'use strict';

const fs = require('fs');
const path = require('path');

const target = process.argv[2] || path.join('pkg', 'package.json');
const pkg = JSON.parse(fs.readFileSync(target, 'utf8'));

// ── Entry-point fields ───────────────────────────────────────────────────
pkg.main = 'jsonstat.js';
pkg.module = 'jsonstat.js';
pkg.types = 'jsonstat.d.ts';

// ── files: ensure the facade + glue and their readable/map artifacts ship ─
// Bare names are the minified defaults; the `.max.js` siblings are the
// readable non-minified source; the `.js.map` files are source maps (which
// reference the co-located `.max.js`). wasm-pack already lists the glue's
// .wasm/.d.ts/snippets, so only the JS artifacts need asserting here.
const requiredFiles = [
    'jsonstat.max.js',
    'jsonstat.js',
    'jsonstat.js.map',
    'jsonstat.d.ts',
    'jsonstat_wasm.max.js',
    'jsonstat_wasm.js',
    'jsonstat_wasm.js.map',
];
pkg.files = Array.isArray(pkg.files) ? pkg.files : [];
for (const f of requiredFiles) {
    if (!pkg.files.includes(f)) {
        pkg.files.unshift(f);
    }
}

// ── repository.url: canonicalize to the git+https form npm expects ───────
// `npm publish` warns (and silently auto-corrects) when the URL lacks the
// `git+https://` scheme and `.git` suffix. wasm-pack copies the URL straight
// from Cargo.toml (plain https://github.com/...), so normalize it here to
// keep publishes warning-free. Idempotent: an already-canonical URL is
// returned unchanged. Handles both the object and (fallback) string forms.
function canonicalRepoUrl(url) {
    if (typeof url !== 'string') return url;
    let u = url.trim();
    // Coerce plain https/http git hosting URLs to git+https.
    u = u.replace(/^https:\/\/github\.com\//, 'git+https://github.com/');
    // Ensure the .git suffix is present.
    if (/^git\+https:\/\/github\.com\//.test(u) && !u.endsWith('.git')) {
        u += '.git';
    }
    return u;
}
if (pkg.repository) {
    if (typeof pkg.repository === 'object' && pkg.repository.url) {
        pkg.repository.url = canonicalRepoUrl(pkg.repository.url);
    } else if (typeof pkg.repository === 'string') {
        pkg.repository = canonicalRepoUrl(pkg.repository);
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
    // Readable non-minified source + source maps, reachable for debugging
    // and by strict resolvers (Node ESM, bundlers honoring `exports`).
    './jsonstat.max.js': './jsonstat.max.js',
    './jsonstat.js.map': './jsonstat.js.map',
    './jsonstat_wasm.max.js': './jsonstat_wasm.max.js',
    './jsonstat_wasm.js.map': './jsonstat_wasm.js.map',
};

fs.writeFileSync(target, JSON.stringify(pkg, null, 2) + '\n', 'utf8');
console.log(`✓ patched ${target} (main → jsonstat.js, dual exports map)`);
