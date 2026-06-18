# Installation

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain)
- [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/) — builds the Rust crate and generates the JS/WASM glue
- A web server (for local development; WASM requires HTTP, not `file://`)

## Building from Source

1. **Clone the repository:**

   ```bash
   git clone https://github.com/jsonstat/wasm.git
   cd jsonstat-wasm
   ```

2. **Build the WebAssembly package:**

   ```bash
   ./scripts/build.sh
   ```

   This wraps `wasm-pack build` and then applies the dual-entry
   customization `wasm-pack` cannot express itself: it copies the high-level
   facade ([`jsonstat.js`](jsonstat.js:1)) and its type definitions
   ([`jsonstat.d.ts`](jsonstat.d.ts:1)) into `pkg/`, and patches
   `pkg/package.json` so `main` points at the facade with a `./glue` subpath
   for the raw glue. Use this instead of calling `wasm-pack build` directly,
   which would otherwise produce a single-entry package that excludes the
   facade.

   > Plain `wasm-pack build --target web` still works for iterating on the
   > Rust code, but it regenerates `pkg/package.json` to its default
   > single-entry shape (main → glue, no facade). Run `./scripts/build.sh`
   > before publishing or testing CDN imports.

   This produces a `pkg/` directory containing:

   | File | Description |
   |------|-------------|
   | `jsonstat.js` | **High-level facade** — `JSONstat()` with automatic init (main entry) |
   | `jsonstat.d.ts` | TypeScript types for the facade |
   | `jsonstat_wasm.js` | Low-level JS glue module (raw class + manual `init()`) |
   | `jsonstat_wasm_bg.wasm` | Compiled WebAssembly binary |
   | `jsonstat_wasm.d.ts` | TypeScript type definitions for the glue |

   Other `--target` options:

   | Target | Use case |
   |--------|----------|
   | `web` | Vanilla HTML/JS (`<script type="module">`) |
   | `bundler` | Bundlers like webpack, Vite, Rollup |
   | `node` | Server-side Node.js |

3. **Serve the demo page:**

   Any static HTTP server works. For example:

   ```bash
   # Python
   python3 -m http.server 8080

   # Node.js (npx, no install required)
   npx serve .

   # Rust
   cargo install miniserve && miniserve .
   ```

   Open [`http://localhost:8080`](http://localhost:8080) in your browser.

## Using in an Existing Web Project

### Vanilla HTML + ES Modules

Copy the `pkg/` directory into your project, then:

```html
<script type="module">
  import init, { JSONstat, init_panic_hook } from './pkg/jsonstat_wasm.js';

  async function main() {
    await init();
    init_panic_hook(); // Optional: improves error messages in the browser console

    const response = await fetch('https://example.com/data.json');
    const jsonStr = await response.text();
    const ds = new JSONstat(jsonStr);

    console.log(ds.label);
    console.log(ds.n);
  }

  main();
</script>
```

### Bundler (Vite, webpack, Rollup)

1. Install from npm (if published) or link locally:

   ```bash
   # From npm (when published)
   npm install jsonstat-wasm

   # Or link the local pkg/
   npm link ./path/to/jsonstat-wasm/pkg
   ```

2. Import in your JS/TS code:

   ```js
   import init, { JSONstat, init_panic_hook } from 'jsonstat-wasm';

   async function main() {
     await init();
     init_panic_hook();

     const ds = new JSONstat('{ "version": "2.0", "class": "dataset", ... }');
     console.log(ds.label);
   }

   main();
   ```

### CDN (no build step, no install)

`jsonstat-wasm` is mirrored by the standard npm-backed CDNs (jsDelivr, unpkg,
esm.sh). Two entry points are available:

| Import path | What you get |
|-------------|-------------|
| `…/jsonstat.js` | **High-level facade** — `JSONstat()` toolkit function with automatic one-time init (recommended) |
| `…/jsonstat_wasm.js` | **Low-level glue** — raw `JSONstat` class + `init()` you call yourself |

Always **pin the version** (here `0.2.1`, matching `Cargo.toml` / `package.json`).

#### High-level facade (recommended)

```html
<script type="module">
  import { JSONstat }
    from 'https://cdn.jsdelivr.net/npm/jsonstat-wasm@0.2.1/jsonstat.js';

  // No init() needed — the facade initializes the WASM module exactly once
  // on first import and gates every call behind that shared promise.
  const ds = await JSONstat('https://example.com/data.json');
  console.log(ds.label, ds.n);
</script>
```

#### Low-level glue

The glue resolves its `.wasm` binary relative to the JS module itself
(`new URL('jsonstat_wasm_bg.wasm', import.meta.url)`), so as long as you point
at the exact `.js` file the binary is fetched automatically:

```js
import init, { JSONstat, init_panic_hook }
  from 'https://cdn.jsdelivr.net/npm/jsonstat-wasm@0.2.1/jsonstat_wasm.js';

await init();          // .wasm resolved via import.meta.url ✅
init_panic_hook();
const ds = new JSONstat(jsonStr);
```

**Explicit URL (bulletproof):** some CDNs (notably esm.sh) rewrite/transform
modules, which can break the `import.meta.url` relative resolution. In that
case pass the binary URL directly to `init()`, which accepts a
`string` / `URL` / `Request`:

```js
import init, { JSONstat } from 'https://esm.sh/jsonstat-wasm@0.2.1/glue';

await init('https://esm.sh/jsonstat-wasm@0.2.1/jsonstat_wasm_bg.wasm');
const ds = new JSONstat(jsonStr);
```

> **Note:** `import.meta.url` resolution is reliable on **jsDelivr** and
> **unpkg**, which serve raw npm files and preserve the flat package layout.
> For **esm.sh** (or any bundler-style CDN), prefer the explicit `init(url)`
> form shown above.

## Using as a Rust Library

The crate is published as `jsonstat-wasm` on [crates.io](https://crates.io) (or can be referenced by path). The [`models`](src/models.rs) and [`query`](src/query.rs) modules are pure Rust with no WASM dependencies.

### Add the dependency

```toml
# Cargo.toml
[dependencies]
jsonstat-wasm = "0.1"
```

Or, for local development:

```toml
[dependencies]
jsonstat-wasm = { path = "../jsonstat-wasm" }
```

### Parse a JSON-stat document in Rust

```rust
use jsonstat_wasm::models::{JsonStatResponse, Dataset};

fn main() {
    let json_str = std::fs::read_to_string("data.json").unwrap();
    let response: JsonStatResponse = serde_json::from_str(&json_str).unwrap();

    match &response {
        JsonStatResponse::Dataset(ds) => {
            println!("Label: {:?}", ds.label);
            println!("Size:  {:?}", ds.size);
            if let Some(values) = &ds.value {
                println!("First value: {:?}", values.get_at(0));
            }
        }
        JsonStatResponse::Collection(c) => {
            println!("Collection: {:?}", c.label);
        }
        JsonStatResponse::Dimension(d) => {
            println!("Dimension: {:?}", d.label);
        }
    }
}
```

### Use the query utilities

```rust
use jsonstat_wasm::query;

fn main() {
    let sizes = vec![3, 2, 4];
    let indices = vec![1, 1, 2];

    let flat_index = query::calculate_index(&indices, &sizes);
    assert_eq!(flat_index, Some(14));

    let roundtrip = query::calculate_indices(14, &sizes);
    assert_eq!(roundtrip, Some(vec![1, 1, 2]));
}
```

## Running Tests

```bash
# Rust unit tests (native)
cargo test

# Rust unit tests (WASM target)
wasm-pack test --node
```

## Releasing & Publishing

Releases are tag-driven and fully automated. Pushing a `v<version>` tag triggers
[`.github/workflows/release.yml`](../.github/workflows/release.yml), which
rebuilds the package, verifies the tag matches `Cargo.toml`, runs the test and
type-check gates, **publishes the patched `pkg/` to npm**, and **creates the
GitHub Release**.

### One-time setup (npm Trusted Publishing)

The workflow authenticates to npm via **[Trusted Publishing](https://docs.npmjs.com/trusted-publishers)**
(OIDC) — there is **no `NPM_TOKEN` secret**. npm mints a short-lived credential
per run from the workflow's `id-token`, which also makes
[provenance](https://docs.npmjs.com/generating-provenance-statements) automatic.

Configure the trusted publisher once:

1. On npm, go to the **`jsonstat-wasm` package → Settings → Trusted Publishing**.
2. Add a **GitHub Actions** publisher with:
   - **Organization / user:** `jsonstat`
   - **Repository:** `wasm`
   - **Workflow filename:** `release.yml`
   - **Environment:** *(leave blank)*

That's it — no secret to store or rotate. The publishing account must already
own/maintain `jsonstat-wasm` to add the trusted publisher.

> The workflow upgrades npm (`npm install -g npm@latest`) before publishing
> because Trusted Publishing requires a recent npm CLI that the bundled Node 20
> npm predates.

### Cutting a release

1. Bump the version in [`Cargo.toml`](../Cargo.toml) (and update any pinned
   `@<version>` CDN references), then commit and push to `main`.
2. Run the release helper from a clean tree on `main`:

   ```bash
   ./scripts/release.sh             # tags v<version> and pushes it
   ./scripts/release.sh --dry-run   # preview without tagging
   ```

   The script validates the working tree, builds locally as a sanity check,
   then creates and pushes the annotated `v<version>` tag. The push is what
   triggers the publish workflow above — no manual `npm publish` needed.
