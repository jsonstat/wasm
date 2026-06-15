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
    // 1. Import JSONstat from the jsDelivr CDN (always the latest version).
    import { JSONstat }
      from 'https://cdn.jsdelivr.net/npm/jsonstat-wasm@latest/jsonstat.js';

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

## Documentation

- 📖 [**Installation guide**](./docs/INSTALL.md) — building from source, using
  with npm/bundlers, CDN usage, and the Rust library API.
- 📚 [**API reference**](./docs/API.md) — every method and property exposed by
  the `JSONstat` class.

---

## License

[MIT](./LICENSE) © Xavier Badosa
