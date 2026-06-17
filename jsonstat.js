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
//       from 'https://cdn.jsdelivr.net/npm/jsonstat-wasm@0.1.2/jsonstat.js';
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

    // URL string: fetch, then parse the response text.
    if (typeof input === 'string') {
        return (async () => {
            await ready;
            const resp = await fetch(input, options);
            return new _JSONstat(await resp.text());
        })();
    }

    // JSON-stat object: stringify (the WASM constructor only accepts a
    // string) and parse.
    return (async () => {
        await ready;
        return new _JSONstat(JSON.stringify(input));
    })();
}
