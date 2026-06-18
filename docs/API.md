# API Reference — JSON-stat WebAssembly

> This is the API reference for the **JSON-stat WebAssembly** library (`jsonstat-wasm`).
> For the original JSON-stat JavaScript Toolkit API, see [toolkit-api.md](toolkit-api.md).

## Overview

The library exposes a single class, `JSONstat`, which parses a [JSON-stat 2.0](https://json-stat.org/format/) string into a WebAssembly-managed object. It supports three response classes: **dataset**, **collection**, and **dimension**.

```js
import init, { JSONstat, init_panic_hook } from './pkg/jsonstat_wasm.js';

await init();
init_panic_hook(); // Optional: better error messages in console

const ds = new JSONstat(jsonStr);
```

---

## Methods

### By type

| Category | Methods |
|----------|---------|
| **Reading** | [`new JSONstat()`](#constructor) |
| **Traversing** | [`Dimension()`](#dimension), [`Category()`](#category), [`Data()`](#data), [`Item()`](#item) |
| **Value access** | [`Datum()`](#datum) |
| **Transforming** | [`Transform()`](#transform), [`Unflatten()`](#unflatten), [`Dice()`](#dice) |
| **Export / round-trip** | [`ToJSON()`](#tojson) |

### By hierarchy

- **`JSONstat`**
  - Properties: [`class`](#class), [`version`](#version), [`label`](#label), [`source`](#source), [`updated`](#updated), [`href`](#href), [`n`](#n), [`size`](#size), [`id`](#id), [`length`](#length), [`error`](#error), [`extension`](#extension), [`note`](#note), [`link`](#link), [`role`](#role), [`status`](#status), [`value`](#value)
  - **Collection responses**: [`Item()`](#item)
  - **Dataset responses**: [`Dimension()`](#dimension), [`Data()`](#data), [`Datum()`](#datum), [`Unflatten()`](#unflatten), [`Transform()`](#transform), [`Dice()`](#dice), [`ToJSON()`](#tojson)
- **`DimensionInstance`** (returned by [`Dimension(id)`](#dimension))
  - Properties: `class`, `id`, `label`, `length`, `role`, `categories`, `note`, `href`
  - Methods: [`Category()`](#category)

---

## Constructor

### `new JSONstat(jsonStr)`

Parses a JSON-stat string into a WebAssembly-managed object.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `jsonStr` | `string` | Yes | A valid JSON-stat 2.0 string |

**Returns:** A `JSONstat` instance.

**Throws:** An error if the string cannot be parsed as valid JSON-stat.

```js
const jsonStr = await (await fetch(url)).text();
const ds = new JSONstat(jsonStr);
```

---

## Properties

### `class`

Response class. String: `"dataset"`, `"collection"`, or `"dimension"`.

```js
console.log(ds.class); // "dataset"
```

### `version`

JSON-stat version string.

```js
console.log(ds.version); // "2.0"
```

### `label`

Human-readable label for the response. `string | null`.

```js
console.log(ds.label); // "Unemployment rate by sex and age"
```

### `source`

Data source description. `string | null`. Available for dataset and collection responses.

### `updated`

Last update timestamp string (ISO 8601). `string | null`. Available for dataset and collection responses.

### `href`

URL of the response. `string | null`.

### `n`

Total number of observations (product of dimension sizes). `number`. Zero for non-dataset responses.

```js
console.log(ds.n); // 432
```

### `size`

Array of dimension sizes. `number[]`. Empty array for non-dataset responses.

```js
console.log(ds.size); // [1, 36, 12]
```

### `id`

Array of dimension IDs. `string[]`. Empty array for non-dataset responses.

```js
console.log(ds.id); // ["concept", "geo", "time"]
```

### `length`

Number of dimensions (dataset), categories (dimension), or items (collection). `number`.

```js
console.log(ds.length); // 3 (3 dimensions)
```

### `error`

Error information from the response. `object | null`. Available for dataset responses.

### `extension`

Extension object with provider-specific metadata. `object | null`.

### `note`

Array of annotation strings. `string[] | null`.

### `link`

Link relations object. `object | null`.

### `role`

Role mapping object (e.g. `{ time: ["year"], geo: ["area"], metric: ["concept"] }`). `object | null`.

### `status`

Status information for dataset values. `array | object | null`. Available for dataset responses.

### `value`

All dataset values as a dense sequence. `Float64Array | any[] | null`.

> **Since v0.2.0 (minor breaking change):** when *every* value is a number, this
> returns a **`Float64Array`** (a contiguous `f64` buffer) rather than a plain
> `Array` of boxed numbers. Index access (`ds.value[i]`), `.length`, iteration
> and spread all behave identically; only `Array.isArray(ds.value)` is affected
> (use `Array.from(ds.value)` to normalize). Datasets containing any string or
> `null` value still return a plain `Array`. Sparse (map-based) values are
> always expanded to a dense sequence. Available for dataset responses.

---

## Methods — Traversing

### `Dimension(dimid?, instance?)`

> **Supported class:** `dataset` only. Throws `"Operation only supported for dataset class"` otherwise.

Gets dimension information from a dataset response.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `dimid` | `number`, `string`, `object`, or `undefined` | No | — | Dimension identifier |
| `instance` | `boolean` | No | `true` | When `false`, returns the category **labels** as a string array instead of the full dimension object |

**`dimid` forms:**

| Type | Behavior |
|------|----------|
| omitted / `undefined` | Returns an array of plain dimension info objects |
| `number` | Returns a **callable `DimensionInstance`** (access by index in the `id` array) |
| `string` | Returns a **callable `DimensionInstance`** (access by dimension ID) |
| `{ role: "time" }` | Returns an array of plain dimension info objects (filtered by role) |

When `instance` is `false`, `Dimension()` returns the dimension's category **labels** (falling back to IDs when no label exists) as a plain string array. For a role filter it returns an array-of-arrays (one label array per matching dimension). This is equivalent to `.Category().map(c => c.label)`.

> **Anatomy note:** A single-ID call (`Dimension("geo")` or `Dimension(0)`) returns a **callable `DimensionInstance`** object — it exposes the dimension properties below **and** a [`Category()`](#category) method for drilling into individual categories: `ds.Dimension("geo").Category("AT").label`. This mirrors the jsonstat-toolkit anatomy. The no-arg and role-filter forms return plain (non-callable) objects, matching the toolkit.

**Returns (single-ID):** A `DimensionInstance` with dimension properties **and** a `Category()` method:

| Property | Type | Description |
|----------|------|-------------|
| `class` | `string` | Always `"dimension"` |
| `id` | `string[]` | Category IDs in this dimension |
| `label` | `string` | Dimension label |
| `length` | `number` | Number of categories |
| `role` | `string` or `undefined` | Role (time, geo, metric, classification) |
| `categories` | `object[]` | Array of category detail objects |
| `note` | `string[]` or `undefined` | Dimension annotations |
| `href` | `string` or `undefined` | Dimension URL |

Each entry in `categories` has:

| Property | Type | Description |
|----------|------|-------------|
| `id` | `string` | Category ID |
| `label` | `string` or `undefined` | Category label |
| `index` | `number` | Position in the dimension |
| `unit` | `object` or `undefined` | Unit information |
| `coordinates` | `number[]` or `undefined` | Geo-coordinates (for geo dimensions) |
| `note` | `any` or `undefined` | Category annotations |

```js
// All dimensions (plain objects, not callable)
const allDims = ds.Dimension();
console.log(allDims[0].label);

// By index → DimensionInstance (callable)
const dim0 = ds.Dimension(0);

// By ID → DimensionInstance (callable)
const areaDim = ds.Dimension("area");
console.log(areaDim.id); // ["AT", "BE", "BG", ...]

// By role (plain objects, not callable)
const geoDims = ds.Dimension({ role: "geo" });

// Category labels only (instance: false)
const areaLabels = ds.Dimension("area", false);
console.log(areaLabels); // ["Austria", "Belgium", "Bulgaria", ...]

// Category labels for every dimension matching a role
const geoLabelArrays = ds.Dimension({ role: "geo" }, false);
// [["Austria", ...], ["Northern Europe", ...]]

// The returned instance is callable — drill into a category:
const auLabel = ds.Dimension("area").Category("AU").label; // "Australia"
```

---

### `Category(catid?)`

<span id="category"></span>

> **Supported class:** `dataset` only (indirectly). `Category()` is a method on the `DimensionInstance` returned by [`Dimension(id)`](#dimension), which itself requires a dataset response.

Gets category information from a [`DimensionInstance`](#dimension) (returned by `Dimension(id)`).

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `catid` | `number`, `string`, or `undefined` | No | Category identifier |

**`catid` forms:**

| Type | Behavior |
|------|----------|
| omitted / `undefined` | Returns an array of all categories for the dimension |
| `number` | Access by position index |
| `string` | Access by category ID |

**Returns:** An object (or array of objects) with category properties:

| Property | Type | Description |
|----------|------|-------------|
| `id` | `string` | Category ID |
| `label` | `string` or `undefined` | Category label |
| `index` | `number` | Position in the dimension |
| `unit` | `object` or `undefined` | Unit information |
| `coordinates` | `number[]` or `undefined` | Geo-coordinates |
| `note` | `any` or `undefined` | Category annotations |

```js
// All categories of the "area" dimension
const areaDim = ds.Dimension("area");
const allCats = areaDim.Category();

// By position
const cat0 = ds.Dimension("area").Category(0);

// By ID
const au = ds.Dimension("area").Category("AU");
console.log(au.label); // "Australia"
```

---

### `Data(dataid?, status?)`

> **Supported class:** `dataset` only. Throws `"Operation only supported for dataset class"` otherwise.

Gets data (value + status) from a dataset. Supports multiple lookup strategies.

**Parameters:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `dataid` | `number`, `number[]`, `object`, or `undefined` | No | — | Data selector |
| `status` | `boolean` | No | `true` | Whether to include status information |

**`dataid` forms:**

| Type | Behavior | Return |
|------|----------|--------|
| omitted / `undefined` | All observations | `{ value, status }[]` or `any[]` |
| `number` | Flat index in the value array | `{ value, status }` or `any` |
| `number[]` | Dimension indices (one per dimension) | `{ value, status }` or `any` |
| `object` | `{ dimId: catId }` mapping | `{ value, status }` or `any`, or an array (slice) when exactly one non-constant dimension is left unspecified |

When `status` is `false`, returns just the value(s) instead of `{ value, status }` objects.

**Partial queries.** Object queries support the Toolkit's partial-query semantics:

- **0 free dimensions** → a single value (or `{ value, status }`) is returned.
- **1 free (non-constant) dimension** → an **array** (a "slice") of results is returned, one entry per category of the free dimension, in dataset order.
- **More than 1 free dimension** → `null` is returned.

Constant (size-1) dimensions are always auto-filled. Query keys that are not real dimension IDs, and category IDs that don't exist, are ignored (the dimension simply becomes free).

```js
// All observations
const all = ds.Data();
// [ { value: 5.9, status: null }, { value: 5.4, status: null }, ... ]

// By flat index
const d0 = ds.Data(0);            // { value: 5.9, status: null }
const v0 = ds.Data(0, false);     // 5.9

// By dimension indices
const d = ds.Data([1, 0, 3]);     // { value: ..., status: ... }

// By dimension/category IDs (fully resolved)
const d = ds.Data({ concept: "UNR", area: "GR", year: "2014" });

// Partial query: one dimension ("year") left free → returns an array slice
const series = ds.Data({ concept: "UNR", area: "GR" }, false);
// [ 5.9, 5.4, 5.0, ... ]  — one entry per year.
// (v0.2.0: when all values are numbers this is a Float64Array; index access
//  is unchanged. Use Array.from(...) if a real Array is required.)
```

---

### `Item(itemid?)`

> **Supported class:** `collection` only. Throws `"Item() is only supported for collection class"` otherwise.

Gets item information from a collection response.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `itemid` | `number`, `object`, or `undefined` | No | Item index or filter |

**`itemid` forms:**

| Type | Behavior |
|------|----------|
| omitted / `undefined` | Returns an array of all items |
| `number` | Access by index |
| `{ class, embedded? }` | Filter items by class (and embedded flag); returns an **array** of matching items |

The object form filters items:

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `class` | `string` | Yes | Item class to match (e.g. `"dataset"`, `"collection"`, `"link"`) |
| `embedded` | `boolean` | No | Only valid when `class` is `"dataset"`. When `true`, matches only datasets with an inline `extension`; when `false`, matches datasets without one. Omitted matches both. |

**Returns:** An object (or array of objects) with item properties:

| Property | Type | Description |
|----------|------|-------------|
| `class` | `string` or `undefined` | Item class (e.g. `"dataset"`) |
| `href` | `string` or `undefined` | Item URL |
| `label` | `string` or `undefined` | Item label |
| `extension` | `any` or `undefined` | Extension metadata |

```js
const j = new JSONstat(collectionJsonStr);
const allItems = j.Item();
const first = j.Item(0);
console.log(first.label); // "Dataset 1"

// All items with class "dataset"
const datasets = j.Item({ class: "dataset" });
// Datasets that carry an inline extension payload
const embedded = j.Item({ class: "dataset", embedded: true });
```

---

## Methods — Value Access

### `Datum(query)`

> **Supported class:** `dataset` only. Throws `"Operation only supported for dataset class"` otherwise.

Gets a single value for a specific combination of dimension categories.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | `object` | Yes | `{ dimensionId: categoryId }` mapping |

Constant (size-1) dimensions can be omitted from the query — they are auto-filled.

**Returns:** `number | null` — the value, or `null` if the cell is empty.

```js
const val = ds.Datum({ area: "CA", year: "2021" });
console.log(val); // 38.0
```

---

## Methods — Transforming

### `Unflatten(callback)`

> **Supported class:** `dataset` only. Throws `"Operation only supported for dataset class"` otherwise.

Iterates over every cell in the dataset, calling the provided callback for each. Returns an accumulated array of callback results.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `callback` | `function` | Yes | Called for each cell |

**Callback signature:**

```js
function(coordinates, datapoint, n, row) { ... }
```

| Argument | Type | Description |
|----------|------|-------------|
| `coordinates` | `object` | `{ dimId: catId }` for the current cell |
| `datapoint` | `object` | `{ value: number|null, status: any }` for the current cell |
| `n` | `number` | Cell counter (0-based) |
| `row` | `array` | The accumulated results array so far |

If the callback returns `undefined`, the result is not added to the output array.

**Returns:** `any[]` — the accumulated array of callback return values.

```js
// Array of { coordinates, value } objects
const result = ds.Unflatten((coordinates, datapoint) => {
  return { coordinates, value: datapoint.value };
});

// CSV string
const csv = ds.Unflatten((coords, dp, n, row) => {
  const line = ds.id.map(d => coords[d]).join(',') + ',' + dp.value;
  return line;
}).join('\n');
```

---

### `Transform(opts?)`

> **Supported class:** `dataset` only. Throws `"Operation only supported for dataset class"` otherwise.

Converts the dataset into tabular form.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `opts` | `object` | No | Transformation options |

**Options:**

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `type` | `string` | `"array"` | Output format: `"array"`, `"arrobj"`, `"objarr"`, or `"object"` |
| `status` | `boolean` | `false` | Include status column. **Ignored when `by` is set** (pivot mode has no status column — matches jsonstat-toolkit) |
| `content` | `string` | `"label"` | Category display: `"label"` or `"id"` |
| `field` | `string` | *(varies)* | Column naming: `"label"` or `"id"` (default: `"label"` for `array`, `"id"` otherwise) |
| `vlabel` | `string` | `"Value"` | Label for the value column when `field` is `"label"`. **Ignored when `by` is set** (pivot mode transposes each `by` category into its own column, so there is no single value column to label) |
| `slabel` | `string` | `"Status"` | Label for the status column when `field` is `"label"` |
| `drop` | `string[]` | `[]` | Dimension IDs to exclude from the output |
| `by` | `string` | — | Dimension ID to pivot on (spreads its categories as columns). Not available for `"array"` or `"object"`. When set, `status` and `vlabel` are ignored (matches jsonstat-toolkit) |
| `meta` | `boolean` | `false` | When `true`, wraps the output as `{ meta, data }` (see below). Not available for `"object"` |
| `prefix` | `string` | `""` | Prepended to each column name created by `by` transposition. Ignored when `by` is not set. Not available for `"array"` or `"object"` |
| `comma` | `boolean` | `false` | When `true`, numeric values are emitted as strings with a comma as the decimal mark. Not available for `"object"` |

#### `"arrobj"` type

Returns an array of objects (one per observation):

```js
const table = ds.Transform({ type: "arrobj" });
// [
//   { area: "AT", year: "2010", value: 4.8 },
//   { area: "AT", year: "2011", value: 4.2 },
//   ...
// ]
```

With `by`:

```js
const table = ds.Transform({ type: "arrobj", by: "year" });
// [
//   { area: "AT", "2010": 4.8, "2011": 4.2 },
//   ...
// ]
```

> **Pivot limitations:** When `by` is set, the `status` option and the `vlabel` label are ignored. Pivot mode spreads the `by` dimension's categories across the columns of the output, so there is no single "value" column to label and no place to emit a status column (each observation's status would need its own column per `by` category). This matches the upstream `jsonstat-toolkit` behavior. Use a non-pivot `type` (`"arrobj"` or `"objarr"` without `by`) if you need `status` or a custom value-column label.

#### `"array"` type

Returns an array of arrays. The first row is the header:

```js
const table = ds.Transform({ type: "array" });
// [
//   ["area", "year", "Value"],
//   ["AT", "2010", 4.8],
//   ["AT", "2011", 4.2],
//   ...
// ]
```

#### `"objarr"` type

Returns a column-oriented object (each property is an array):

```js
const table = ds.Transform({ type: "objarr" });
// {
//   area: ["AT", "AT", "BE", ...],
//   year: ["2010", "2011", "2010", ...],
//   value: [4.8, 4.2, 7.6, ...]
// }
```

#### `"object"` type

Returns a [Google DataTable](https://developers.google.com/chart/interactive/docs/reference#DataTable) (`{ cols, rows }`). The value column type is inferred naïvely from the first observation: `"number"` if it is a number or `null`, `"string"` otherwise. Dimension columns are always `"string"`. The `meta`, `comma`, `by`, and `prefix` options are not available for this type.

```js
const table = ds.Transform({ type: "object" });
// {
//   cols: [
//     { id: "area", label: "area", type: "string" },
//     { id: "year", label: "year", type: "string" },
//     { id: "value", label: "Value", type: "number" }
//   ],
//   rows: [
//     { c: [ { v: "AT" }, { v: "2010" }, { v: 4.8 } ] },
//     ...
//   ]
// }
```

#### `meta` option

When `meta: true` (not available for `"object"`), the output is wrapped as an object with two properties: `meta` (dataset metadata, the resolved options, and per-dimension category information) and `data` (the same array/object that would be returned with `meta: false`):

```js
const { meta, data } = ds.Transform({ type: "arrobj", meta: true });
// meta: { type, label, source, updated, id, status, by, drop, prefix, comma, dimensions: { ... } }
// data: [ { area: "AT", year: "2010", value: 4.8 }, ... ]
```

The `meta.dimensions` object is keyed by dimension ID and is not affected by `by` or `drop`.

---

### `Dice(filter, opts?)`

Filters the dataset, keeping only the specified dimension categories. Returns a **new** `JSONstat` instance with the subset.

> **Supported class:** `dataset` only. Throws `"Operation only supported for dataset class"` otherwise.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `filter` | `object` or `array` | Yes | Category filter specification |
| `opts` | `object` | No | Options |

**Filter forms:**

Object (recommended):
```js
{ "dimensionId": ["categoryId1", "categoryId2"] }
```

Array of pairs:
```js
[ ["dimensionId", ["categoryId1", "categoryId2"]], ... ]
```

**Options:**

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `drop` | `boolean` | `false` | When `true`, the filter specifies categories to **remove** instead of keep |

**Returns:** A new `JSONstat` instance with the filtered subset.

```js
// Keep only Austria and Canada for years 2010-2011
const subset = ds.Dice({
  area: ["AT", "CA"],
  year: ["2010", "2011"]
});

// Drop (remove) specific categories instead
const subset2 = ds.Dice(
  { area: ["EU15", "OECD"] },
  { drop: true }
);
```

---

## Methods — Export / Round-trip

### `ToJSON()`

> **Supported class:** `dataset`, `collection`, and `dimension`. Serializes whatever response class the instance holds.

Serializes the current state back to a JSON-stat string.

Because a parsed `JSONstat` instance is an opaque WASM handle wrapping Rust
memory, `JSON.stringify(ds)` does **not** produce a valid JSON-stat document.
`ToJSON()` is the only path that re-serializes the full internal response —
most importantly the only way to export a transformed subset (e.g. the result
of [`Dice()`](#dice)) out of WASM and back into a usable JSON-stat string.

> **Note on the name:** This method is *not* the JavaScript [`toJSON()`
> protocol](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/JSON/stringify#tojson_behavior_of_object_values).
> The PascalCase `ToJSON` is a distinct property and is never invoked by
> `JSON.stringify()`. It returns a JSON-stat string directly.

**Returns:** `string` — a valid JSON-stat 2.0 string.

```js
const jsonStr = ds.ToJSON();
console.log(jsonStr);

// Export a transformed subset out of WASM:
const subset = ds.Dice({ area: ["AT", "BE"] });
const subsetStr = subset.ToJSON();
```

---

## Initialization

### `init_panic_hook()`

Installs a panic hook that forwards Rust panic messages to the browser's `console.error`. Call this once after `init()` for better debugging.

```js
import init, { JSONstat, init_panic_hook } from './pkg/jsonstat_wasm.js';

await init();
init_panic_hook();
```

---

## Error Handling

Most methods return a `Result` type. Errors are thrown as JavaScript `Error` objects with descriptive messages:

```js
try {
  const ds = new JSONstat("invalid json");
} catch (e) {
  console.error(e.message); // "Failed to parse JSON-stat: ..."
}
```

Common error cases:

| Scenario | Message |
|----------|---------|
| Invalid JSON | `"Failed to parse JSON-stat: ..."` |
| Unknown class | `"Unknown class: '...'"` |
| Wrong response class | `"Operation only supported for dataset class"` |
| Missing dimension | `"Dimension '...' not found"` |
| Missing category | `"Category '...' not found in dimension '...'"` |
| Invalid query | `"Query is missing category for non-constant dimension '...'"` |
