# jsonstat-wasm

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

An **experimental** [WebAssembly](https://webassembly.org/) port of the
[JSON-stat](https://json-stat.org/) parsing logic that powers the
[JSON-stat JavaScript Toolkit](https://github.com/jsonstat/toolkit).

It does **not** aim to be a full clone of the toolkit. Instead, it mimics the
**main subset of methods** that the toolkit exposes, but implemented in
[Rust](https://www.rust-lang.org/) and compiled to WASM. The idea is to explore
whether the toolkit's day-to-day API can be delivered with WASM-level speed and
a tiny footprint, while keeping usage familiar to anyone who already knows the
toolkit.

> ⚠️ This is a work in progress and an experiment. The API may change, and not
> every toolkit feature is available yet. See [What is implemented](#what-is-implemented)
> and the [API reference](./docs/API.md) for the current scope.

---

## Table of contents

- [What is JSON-stat?](#what-is-jsonstat)
- [What is implemented](#what-is-implemented)
- [Try it in a webpage (simple version)](#try-it-in-a-webpage-simple-version)
- [How it works](#how-it-works)
- [Performance](#performance)
- [Documentation](#documentation)
- [License](#license)

---

## What is JSON-stat?

[JSON-stat](https://json-stat.org/) is a simple, open format for statistical
data. Statistical offices around the world publish datasets (population, economy,
unemployment, etc.) as JSON-stat documents. Reading one of those documents in the
browser — turning it into something you can query and display — is exactly the
job the JSON-stat Toolkit (and this experiment) is built for.

## What is implemented

This package exposes the toolkit-style entry point and the core methods you use
most often:

- `JSONstat(input)` — create a dataset instance from a JSON-stat **string**,
  **object**, or **URL** (fetched for you).
- **Traversing:** `Dimension()`, `Category()`, `Data()`, `Item()`.
- **Transforming:** `Transform()`, `Unflatten()`, `Dice()` (subsetting/filtering).
- **Export / round-trip:** `ToJSON()`.

For the full, precise list of methods and properties, see the
[**API reference**](./docs/API.md).

---

## Try it in a webpage (simple version)

The easiest way to use `jsonstat-wasm` is to load it straight from a
[CDN](https://en.wikipedia.org/wiki/Content_delivery_network) — **no download,
no build step, no package manager**. You only need a plain `.html` file.

### 1. Create a file called `index.html`

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <title>jsonstat-wasm demo</title>
</head>
<body>
  <h1>jsonstat-wasm demo</h1>
  <pre id="output">Loading…</pre>

  <script type="module">
    // 1. Import JSONstat from a CDN (in production, pin to a proven
    //    specific version instead of @latest for safety).
    //    jsDelivr (default):
    import { JSONstat }
      from 'https://cdn.jsdelivr.net/npm/jsonstat-wasm@latest/jsonstat.js';
    //    …or unpkg (choose either one):
    // import { JSONstat }
    //   from 'https://unpkg.com/jsonstat-wasm@latest/jsonstat.js';

    // 2. Point at any JSON-stat dataset (here, the OECD sample).
    const url = 'https://json-stat.org/samples/oecd.json';

    // 3. Fetch + parse it in one call. JSONstat() returns a Promise.
    const ds = await JSONstat(url);

    // 4. Read some basic information about the dataset.
    const
      label = ds.label,                 // dataset title
      dims  = ds.id,                    // array of dimension IDs
      n     = ds.n                      // number of data values
    ;

    // 5. Look up a dimension and a category, just like the toolkit.
    const
      geoDim     = ds.Dimension('concept'), // a dimension by ID
      firstLabel = geoDim.Category(0).label // first category's label
    ;

    document.getElementById('output').textContent =
      `Dataset: ${label}\n` +
      `Dimensions: ${dims.join(', ')}\n` +
      `Number of values: ${n}\n` +
      `First category of "concept": ${firstLabel}`;
  </script>
</body>
</html>
```

### 2. Open it through a local web server

Browsers block `import` and `fetch()` when you open a file directly
(`file://...`), so serve the folder over HTTP. Any static server works:

```bash
# Python (already installed on most systems)
python3 -m http.server 8080

# …or with Node.js (no install needed)
npx serve .
```

Then visit [http://localhost:8080](http://localhost:8080) in your browser. You
should see the dataset title, its dimensions, and the number of data values.

### 3. That's it

From here you can explore the dataset with the methods in the
[API reference](./docs/API.md) — for example `ds.Data(0).value` to read the
first value, or `ds.Dice({ ... })` to create a filtered subset.

> Want to use it with npm, a bundler, or build it yourself from the Rust source?
> See the [**Installation guide**](./docs/INSTALL.md).

---

## How it works

The parsing and querying logic is written in Rust (see [`src/`](./src/)) and
compiled to a WebAssembly module. A thin JavaScript facade (`jsonstat.js`)
loads and initializes the WASM module automatically, then exposes a familiar
toolkit-style `JSONstat()` function. Because the heavy lifting happens in WASM,
parsing large datasets is fast, while the JavaScript you write stays simple.

---

## Performance

Since v0.3.0, `jsonstat-wasm` is engineered to **beat the plain JS toolkit on
the hot paths**, not just match it. On large datasets (~100k cells) versus
[`jsonstat-toolkit`](https://github.com/jsonstat/toolkit), measured in Chrome
148 (via [`.idea/test/bench.html`](./.idea/test/bench.html)) and Node 23 on
macOS (median of N runs, ratio < 1.0 = WASM wins):

| Phase | WASM vs JS toolkit | Notes |
|---|---|---|
| **`JSONstat(string)` parse** | **~1.6–2.1× faster** ✅ | single-pass Rust `serde_json` (the `fetch().then(r=>r.text())` path) |
| **`Transform({type:'arrobj'})`** | **~7–15× faster** ✅✅ | columnar fast path — the bigger the dataset, the bigger the win |
| `JSONstat(obj)` parse | **~2.3–4× slower** ❌ | irreducible: the JS engine reads its own heap in place; WASM must cross the boundary per property. Use the string path (`fetch`→`text()`) instead. |
| `ds.value` getter, `Data()` slice | tied (sub-millisecond) | `ds.value` is cached on both sides after first read |

> **When is WASM slower?** Only on `JSONstat(obj)` — passing an *already-parsed*
> JS object. The JS toolkit traverses V8's heap directly with no serialization,
> while WASM pays a per-property boundary crossing. This path cannot be made
> competitive without abandoning the WASM boundary entirely. The fix is to hand
> WASM the **text** instead: `JSONstat(await response.text())` is a 2× win,
> because a single Rust `serde_json` pass beats V8's `JSON.parse` + a JS walk.

### How the speed-ups work

- **Single-pass string parsing.** A `{`-leading string is handed straight to the
  Rust constructor — one `serde_json` traversal. The previous double-parse
  (V8's `JSON.parse` + a property-by-property boundary walk) is gone.
- **Columnar `Transform`.** For plain `arrobj` (no `by`/`meta`), Rust emits a
  column-oriented payload (`Float64Array` for numeric values, `Uint32Array`
  label indices for dimension columns) and a tiny JS assembler stitches the row
  objects together with a V8-JIT'd object literal. No per-cell `serde_json`
  tree, no per-cell map allocation. Other transform types (`array`, `object`,
  `objarr`, `arrobj` with `by`/`meta`) use the original serde path unchanged.
- **Zero-copy numeric values.** An all-numeric dataset is stored as a contiguous
  `Vec<f64>` and exposed as a `Float64Array`, so `ds.value` is one bulk copy.

### Behavior changes in v0.3.0 (minor, breaking-ish)

These are the trade-offs for the speed-ups. They are minor, but callers relying
on the exact v0.2.x shapes should be aware:

1. **`ds.value` returns a `Float64Array`, not an `Array`, when every value is a
   number.** Index access (`ds.value[i]`), `.length`, and iteration work
   identically; `Array.isArray(ds.value)` now returns `false` on all-numeric
   datasets. Use `Array.from(ds.value)` if you need a real `Array`. Datasets
   with strings/nulls still return a plain `Array`.
2. **`ds.value` is cached.** Repeated reads return the same `Float64Array`/
   `Array` instance (`ds.value === ds.value`), so the bulk copy happens once.
   Mutating the returned buffer will affect subsequent reads — treat it as
   read-only.

See [`docs/releases/v0.3.0.md`](./docs/releases/v0.3.0.md) for the full
change list.

---

## Documentation

- 📖 [**Installation guide**](./docs/INSTALL.md) — building from source, using
  with npm/bundlers, CDN usage, and the Rust library API.
- 📚 [**API reference**](./docs/API.md) — every method and property exposed by
  the `JSONstat` class.
- 🧪 [**Live examples**](https://jsonstat.com/examples/?lib=wasm) — runnable
  `jsonstat-wasm` examples on jsonstat.com.

---

## License

[MIT](./LICENSE) © Xavier Badosa
