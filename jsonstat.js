// JSON-stat WASM public facade (committed source-of-truth).
//
// This file lives at the repository root (NOT under pkg/, which is
// git-ignored build output). The build script (scripts/build.sh) copies it
// into pkg/jsonstat.js alongside the generated glue so the two are
// co-located on npm and CDNs.
//
// It is the single external entry point for the toolkit-style API. It
// imports the generated WASM glue (./jsonstat_wasm.js) and exposes a plain
// `JSONstat(input, options)` function that mirrors the jsonstat-toolkit
// `JSONstat()` documented in toolkit-api.md:
//
//     const v = await JSONstat("version");        // → package version string
//     const ds = await JSONstat(url, options);     // fetch + parse
//     const ds = await JSONstat(obj);              // parse an object
//
// The WASM module is initialized exactly once on first import (ES module
// caching guarantees a single execution). Every call awaits that shared
// promise, so WASM is never used before init() has completed.
//
// CDN note: the import below is relative (`./jsonstat_wasm.js`), so the glue
// and this facade MUST be co-located. On npm-mirroring CDNs (jsDelivr,
// unpkg) the flat package layout keeps them side by side, and the glue
// resolves its `.wasm` via `new URL('jsonstat_wasm_bg.wasm', import.meta.url)`.
// That means a CDN consumer can simply:
//
//     import { JSONstat }
//       from 'https://cdn.jsdelivr.net/npm/jsonstat-wasm@0.3.0/jsonstat.js';
//
// and initialization is fully automatic.
//
// API deviation from the toolkit: because the package version is baked into
// the WASM binary (env!("CARGO_PKG_VERSION")), `JSONstat("version")` must
// await the init gate and therefore returns a Promise<string>, unlike the
// toolkit where it is synchronous.
//
// Note on `new`: the toolkit calls `JSONstat(...)` as a plain function
// (without `new`). This facade is a normal function. It wraps the WASM class
// internally, so consumers should never use `new JSONstat(...)` here.
import init, {
    JSONstat as _JSONstat,
    version as _version,
    init_panic_hook,
} from './jsonstat_wasm.js';

const ready = (async () => {
    await init();
    init_panic_hook();
})();

// Resolves to the package version string. The version is baked into the WASM
// binary via env!("CARGO_PKG_VERSION"), so `version()` is gated on `ready`
// alongside the other branches.
async function versionAsync() {
    await ready;
    return _version();
}

// ── Columnar Transform assembler ───────────────────────────────────────────
//
// `JSONstat#_transformColumns(opts)` emits a *columnar* payload instead of
// the row-based `[{dim:value, ..., Value:v}, ...]` produced by the serde
// path. This module turns that payload back into the row-object array using a
// V8-JIT'd `new Function` constructor, so per-row assembly is one object
// literal per row with no `Reflect.set` calls and no Map allocation.
//
// Payload shape (from Rust):
//   {
//     __columnar: true,
//     n: <row count>,
//     names: ["colA", "colB", ..., "Value"],
//     cols: [
//       { kind: "enum",   uniques: [...], indices: Uint32Array }, // dim col
//       { kind: "number", data: Float64Array },                    // numeric value
//       { kind: "cells",  data: Array },                           // mixed / status
//     ]
//   }
//
// `enum` columns carry `uniques` (distinct category strings, one per category
// position) and `indices` (row → category position, length N). We expand the
// enum into a plain JS array of strings ONCE, then read by index in the JIT'd
// loop — far cheaper than crossing the boundary N times for repeated strings.
function assembleColumnarTransform(payload) {
    const n = payload.n | 0;
    const names = payload.names;
    const cols = payload.cols;

    // Pre-materialize each column as a length-N dense array of JS values.
    const dataArrays = new Array(cols.length);
    for (let c = 0; c < cols.length; c++) {
        const col = cols[c];
        if (col.kind === 'number') {
            // Float64Array is already indexable in a hot loop. NaN → null to
            // match the serde path's treatment of absent sparse values.
            const src = col.data;
            const out = new Array(n);
            for (let i = 0; i < n; i++) {
                const v = src[i];
                out[i] = v !== v ? null : v; // NaN test
            }
            dataArrays[c] = out;
        } else if (col.kind === 'enum') {
            const uniques = col.uniques;
            const idx = col.indices;
            const out = new Array(n);
            for (let i = 0; i < n; i++) {
                out[i] = uniques[idx[i]];
            }
            dataArrays[c] = out;
        } else {
            // cells: plain Array already; pass through (length N).
            dataArrays[c] = col.data;
        }
    }

    // Build a JIT'd row assembler: the column names are baked into the
    // generated source as object-literal keys, and each column's value array
    // is read by index. This is the hot loop that previously cost a
    // serde_json::Value::Object + serde-wasm-bindgen re-walk per cell.
    const nc = cols.length;
    let src = '"use strict";return function(n,a){var out=new Array(n);';
    src += 'for(var i=0;i<n;i++){out[i]={';
    for (let c = 0; c < nc; c++) {
        if (c) src += ',';
        // Names from JSON-stat dimensions are arbitrary strings; bake them as
        // JSON-quoted keys (handles quotes / special chars safely).
        src += JSON.stringify(names[c]) + ':a[' + c + '][i]';
    }
    src += '};}return out;};';
    // eslint-disable-next-line no-new-func
    const assembler = new Function(src)();

    return assembler(n, dataArrays);
}

// Wrap a WASM-backed `_JSONstat` instance so `Transform(opts)` routes
// `arrobj` (without `by`/`meta`) through the columnar fast path, and every
// other option shape falls back to the serde-based Rust `Transform`.
//
// The wrapper is a Proxy so every other property access (value, n, size,
// Data, Datum, Dimension, Dice, …) is forwarded unchanged — we only
// intercept `Transform`.
function wrapDataset(instance) {
    // JS-side memoization for `value`. The WASM getter already caches the
    // underlying Float64Array in Rust (so the bulk f64 copy happens once),
    // but every call STILL crosses the JS↔WASM boundary to invoke the getter
    // and clone the JsValue (~30µs/call). The plain-JS toolkit stores `value`
    // as a real JS property, so repeated reads are zero-cost. To match that,
    // we cache the first read in a closure variable; every subsequent
    // `ds.value` is a pure JS property lookup with no boundary crossing.
    // This also keeps `ds.value === ds.value` (the documented v0.3.0 behavior).
    let valueCache;
    let valueCached = false;
    return new Proxy(instance, {
        get(target, prop, receiver) {
            if (prop === 'value') {
                if (!valueCached) {
                    valueCache = target.value;
                    valueCached = true;
                }
                return valueCache;
            }
            if (prop === 'Transform') {
                return function transformFacade(opts) {
                    const type = opts && opts.type;
                    const by = opts && opts.by;
                    const meta = opts && opts.meta;
                    // Columnar fast path: plain arrobj without by or meta.
                    if ((type === undefined || type === 'arrobj') && !by && !meta) {
                        const payload = target._transformColumns(opts || undefined);
                        // Defensive: if Rust ever declines the columnar path
                        // (it doesn't today, but the contract allows it),
                        // fall back to the serde Transform.
                        if (payload && payload.__columnar) {
                            return assembleColumnarTransform(payload);
                        }
                    }
                    return target.Transform(opts);
                };
            }
            const v = Reflect.get(target, prop, receiver);
            // Preserve method `this`-binding for any other function the
            // consumer invokes off the proxy.
            return typeof v === 'function' ? v.bind(target) : v;
        },
    });
}

/**
 * Creates a jsonstat instance from an external input.
 *
 * @param {string | object} input - "version", a URL string, or a JSON-stat
 *   object.
 * @param {RequestInit} [options] - fetch options, only used when `input` is a
 *   URL.
 * @returns {Promise<string> | Promise<_JSONstat>} A Promise resolving to the
 *   package version string (when `input === "version"`) or to a JSONstat
 *   instance.
 */
export function JSONstat(input, options) {
    // Toolkit entry point: return the package version.
    if (input === 'version') {
        return versionAsync();
    }

    // String input. It may be either a URL to fetch or an inline JSON-stat
    // document — the toolkit treats both as valid string inputs. We must
    // disambiguate before doing anything destructive: handing a multi-KB JSON
    // string to fetch() produces a percent-encoded giant URI that 414s (and a
    // single-token string like "true" would fetch a nonsense URL).
    //
    // Single-pass detection (v0.3.0): we peek the first non-whitespace char.
    //   '{' → object-shaped JSON-stat document. Route directly through the
    //         WASM constructor `new _JSONstat(str)`, which is a single
    //         pure-Rust serde_json traversal. The previous implementation did
    //         JSON.parse THEN fromObject — two full traversals (V8's native
    //         parse plus a serde-wasm-bindgen walk that paid a Reflect::get
    //         per array element). For large datasets the double walk was a
    //         2–6× regression vs the plain JS toolkit.
    //   anything else → URL to fetch (matches the toolkit, which treats any
    //         non-keyword string as a URL).
    //
    // We do NOT try to JSON.parse first: a leading '{' is a reliable object
    // signal, and a malformed object (e.g. "{not json") will surface as a
    // clean error from serde_json rather than a silent URL fetch. Numbers,
    // booleans, arrays, and bare strings parse to non-objects in JSON and are
    // not valid JSON-stat documents, so they correctly fall through to the
    // URL path — consistent with the toolkit.
    if (typeof input === 'string') {
        let firstChar = '';
        for (let i = 0; i < input.length; i++) {
            const ch = input[i];
            if (ch !== ' ' && ch !== '\t' && ch !== '\n' && ch !== '\r') {
                firstChar = ch;
                break;
            }
        }
        if (firstChar === '{') {
            // Single-pass: serde_json on the raw string, no JS-side parse.
            return (async () => {
                await ready;
                return wrapDataset(new _JSONstat(input));
            })();
        }
        // URL path: fetch the body, then serde_json it in one pass.
        return (async () => {
            await ready;
            const resp = await fetch(input, options);
            return wrapDataset(new _JSONstat(await resp.text()));
        })();
    }

    // JSON-stat object: pass straight into WASM. `fromObject` deserializes
    // the JS object directly into the Rust model via serde-wasm-bindgen,
    // avoiding the JSON.stringify + re-parse round-trip. V8's native
    // JSON.parse (which already produced this object) is faster than
    // serde_json, so we let the JS engine own the lexing and traverse the
    // object only once on the Rust side.
    return (async () => {
        await ready;
        return wrapDataset(_JSONstat.fromObject(input));
    })();
}
