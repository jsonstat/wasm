// Type definitions for the JSON-stat WASM facade (`./jsonstat.js`).
//
// Committed source-of-truth; the build script copies this into
// pkg/jsonstat.d.ts alongside the generated type definitions.
//
// The facade wraps the raw WASM glue (`./jsonstat_wasm.js`) and exposes a
// single overloaded `JSONstat()` function plus a re-export of the underlying
// class for typing convenience.

import type { JSONstat as JSONstatClass } from './jsonstat_wasm.js';

export type { JSONstatClass };

/**
 * Toolkit entry point.
 *
 * - `JSONstat("version")` → package version string (Promise-wrapped because
 *   the version is baked into the WASM binary).
 * - `JSONstat(url, options?)` → fetch + parse a remote JSON-stat document.
 *   A `{`-leading `input` string is treated as an inline JSON-stat document and
 *   parsed in a single Rust pass (no double `JSON.parse`); any other string is
 *   fetched as a URL.
 * - `JSONstat(obj)` → parse an in-memory JSON-stat object.
 *
 * The returned `JSONstatClass` instance is wrapped in a Proxy that routes
 * `Transform({type:'arrobj'})` (without `by`/`meta`) through a columnar fast
 * path; all other `Transform` options use the serde fallback transparently.
 *
 * @see https://github.com/jsonstat/wasm/blob/main/docs/releases/v0.3.0.md
 *   for the v0.3.0 performance and behavior changes, including:
 *   - `ds.value` returns a `Float64Array` (not `Array`) on all-numeric datasets;
 *   - `ds.value` is cached (`ds.value === ds.value`), treat the buffer as
 *     read-only.
 */
export function JSONstat(input: 'version'): Promise<string>;
export function JSONstat(input: string, options?: RequestInit): Promise<JSONstatClass>;
export function JSONstat(input: object): Promise<JSONstatClass>;
