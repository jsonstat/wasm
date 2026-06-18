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
//       from 'https://cdn.jsdelivr.net/npm/jsonstat-wasm@0.2.0/jsonstat.js';
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
    // single-token string like "true" would fetch a nonsense URL). The check is
    // cheap (V8's native JSON.parse) and only runs on the string branch.
    //
    // Heuristic, matching the toolkit: try to parse the string as JSON. If it
    // parses to a plain object, route through fromObject (single serde-wasm-
    // bindgen walk over the already-parsed object). Otherwise treat the input
    // as a URL.
    //
    // URL path: parse via the WASM constructor (serde_json on the contiguous
    // body string) rather than `resp.json()` + `fromObject()`. For a fetched
    // body that is NOT yet parsed, serde_json is a single pure-Rust traversal
    // with no per-token JS-engine crossings, whereas `resp.json()` +
    // `fromObject()` would be TWO traversals (native JSON.parse, then a
    // serde-wasm-bindgen walk that pays a `Reflect::get` call per array
    // element) — a regression for large datasets. (simd-json was also evaluated
    // and rejected: it requires `target-feature=+simd128`, breaking older
    // browsers, and its scalar fallback is slower than serde_json.)
    if (typeof input === 'string') {
        let parsed;
        try {
            parsed = JSON.parse(input);
        } catch (_) {
            parsed = undefined;
        }
        // Only a plain object is a JSON-stat document. Numbers/strings/arrays
        // (e.g. "true", "42", "[1,2]") parse but are not valid inputs, so they
        // fall through to the URL path — consistent with the toolkit, which
        // would also reject them as documents.
        if (parsed !== undefined && typeof parsed === 'object' && parsed !== null && !Array.isArray(parsed)) {
            return (async () => {
                await ready;
                return _JSONstat.fromObject(parsed);
            })();
        }
        return (async () => {
            await ready;
            const resp = await fetch(input, options);
            return new _JSONstat(await resp.text());
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
        return _JSONstat.fromObject(input);
    })();
}
