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
 * - `JSONstat(obj)` → parse an in-memory JSON-stat object.
 */
export function JSONstat(input: 'version'): Promise<string>;
export function JSONstat(input: string, options?: RequestInit): Promise<JSONstatClass>;
export function JSONstat(input: object): Promise<JSONstatClass>;
