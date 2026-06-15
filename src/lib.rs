pub mod models;
pub mod query;

use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use models::{
    Cell, JsonStatResponse, CollectionItem, Dataset, DatasetValue, Dimension,
};

// ── WASM Initialisation ───────────────────────────────────────────────────

#[wasm_bindgen]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

/// Returns the `jsonstat-wasm` package version (from `Cargo.toml`).
///
/// Mirrors the toolkit's `JSONstat("version")` entry point: callers use
/// `JSONstat("version")` on the JS facade, which returns this string
/// synchronously without instantiating any WASM-backed object.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ── JSONstat Wrapper ──────────────────────────────────────────────────────

#[wasm_bindgen]
pub struct JSONstat {
    response: JsonStatResponse,
}

// ── Private Helpers ───────────────────────────────────────────────────────

/// Convert a serializable value to a JsValue via JSON string round-trip.
/// This avoids serde_wasm_bindgen::to_value() which doesn't correctly
/// convert nested serde_json::Value objects (arrays/objects come through
/// as undefined in JS).
fn to_js_value<T: serde::Serialize>(val: &T) -> JsValue {
    match serde_json::to_string(val) {
        Ok(s) => js_sys::JSON::parse(&s).unwrap_or(JsValue::NULL),
        Err(_) => JsValue::NULL,
    }
}

/// Like `to_js_value` but returns a Result for methods that need error propagation.
fn to_js_value_result<T: serde::Serialize>(val: &T) -> Result<JsValue, JsValue> {
    let s = serde_json::to_string(val)
        .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))?;
    js_sys::JSON::parse(&s)
        .map_err(|e| JsValue::from_str(&format!("JSON parse error: {:?}", e)))
}

/// Extract a dataset reference from the response, or error.
fn require_dataset(resp: &JsonStatResponse) -> Result<&Dataset, JsValue> {
    match resp {
        JsonStatResponse::Dataset(d) => Ok(d),
        _ => Err(JsValue::from_str(
            "Operation only supported for dataset class",
        )),
    }
}

/// Look up a category index in a dimension's category index structure.
/// Falls back to the category label when 'index' is absent, which the
/// JSON-stat spec allows for single-category dimensions.
fn resolve_category_index(
    dim: &Dimension,
    cat_id: &str,
) -> Result<usize, String> {
    let category = dim
        .category
        .as_ref()
        .ok_or_else(|| "Dimension is missing 'category'".to_string())?;
    match category.index.as_ref() {
        Some(index) => index
            .get_index_of(cat_id)
            .ok_or_else(|| format!("Category '{}' not found", cat_id)),
        None => {
            let label = category.label.as_ref().ok_or_else(|| {
                "Category is missing both 'index' and 'label'".to_string()
            })?;
            if label.len() == 1 && label.contains_key(cat_id) {
                Ok(0)
            } else {
                Err(format!("Category '{}' not found", cat_id))
            }
        }
    }
}

/// Get the category ID at a given position in a dimension's category index.
/// Falls back to the category label when 'index' is absent.
fn category_id_at(dim: &Dimension, pos: usize) -> Option<String> {
    let category = dim.category.as_ref()?;
    match category.index.as_ref() {
        Some(index) => index.get_id_at(pos),
        None => {
            let label = category.label.as_ref()?;
            if label.len() == 1 && pos == 0 {
                label.keys().next().cloned()
            } else {
                None
            }
        }
    }
}

/// Get the category labels of a dimension, falling back to the category ID
/// when no label is present. This backs [`Dimension(id, false)`] — the
/// toolkit's "instance false" form that returns category labels.
///
/// [`Dimension(id, false)`]: JSONstat::dimension
fn category_labels_of(dim: &Dimension) -> Vec<String> {
    category_ids_of(dim)
        .iter()
        .map(|id| category_label_for(dim, id))
        .collect()
}

/// Get all category IDs of a dimension, falling back to the label keys when
/// 'index' is absent (single-category dimensions).
fn category_ids_of(dim: &Dimension) -> Vec<String> {
    let Some(category) = dim.category.as_ref() else {
        return vec![];
    };
    match category.index.as_ref() {
        Some(index) => index.ids(),
        None => category
            .label
            .as_ref()
            .filter(|l| l.len() == 1)
            .map(|l| l.keys().cloned().collect())
            .unwrap_or_default(),
    }
}

/// Get the category label for a given category ID in a dimension.
fn category_label_for(dim: &Dimension, cat_id: &str) -> String {
    dim.category
        .as_ref()
        .and_then(|c| c.label.as_ref())
        .and_then(|l| l.get(cat_id))
        .cloned()
        .unwrap_or_else(|| cat_id.to_string())
}

/// Get dimension label (or fall back to the dimension ID).
fn dimension_label_or(dim: &Dimension, fallback: &str) -> String {
    dim.label
        .clone()
        .unwrap_or_else(|| fallback.to_string())
}

/// Resolve the role (time/geo/metric/classification) of a dimension ID, if any.
/// Returns the first matching role name. Used by `Dimension()` to populate the
/// `role` property of returned dimension instances.
fn resolve_role(dataset: &Dataset, dim_id: &str) -> Option<String> {
    dataset.role.as_ref().and_then(|roles| {
        for (role_name, ids) in roles {
            if ids.contains(&dim_id.to_string()) {
                return Some(role_name.clone());
            }
        }
        None
    })
}

/// Build a category info object (id, label, index, unit, coordinates, note)
/// for a single category in a dimension. Shared by the `DimensionInstance`
/// getters/method and the `Dimension()` array forms so the category shape is
/// identical whether the dimension is accessed chained or in bulk.
fn build_cat_info(dim: &Dimension, cat_id: &str, pos: usize) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "id".to_string(),
        serde_json::Value::String(cat_id.to_string()),
    );
    if let Some(category) = dim.category.as_ref() {
        if let Some(label) = category.label.as_ref().and_then(|l| l.get(cat_id)) {
            obj.insert(
                "label".to_string(),
                serde_json::Value::String(label.clone()),
            );
        }
        if let Some(unit) = category.unit.as_ref().and_then(|u| u.get(cat_id)) {
            obj.insert("unit".to_string(), unit.clone());
        }
        if let Some(coords) = category.coordinates.as_ref().and_then(|c| c.get(cat_id)) {
            obj.insert(
                "coordinates".to_string(),
                serde_json::to_value(coords).unwrap_or(serde_json::Value::Null),
            );
        }
        if let Some(note) = category.note.as_ref().and_then(|n| n.get(cat_id)) {
            obj.insert("note".to_string(), note.clone());
        }
    }
    obj.insert("index".to_string(), serde_json::Value::Number(pos.into()));
    serde_json::Value::Object(obj)
}

/// Get a status value for a flat index from the dataset.
fn status_at(dataset: &Dataset, flat: usize) -> serde_json::Value {
    match &dataset.status {
        Some(serde_json::Value::Array(arr)) => {
            arr.get(flat).cloned().unwrap_or(serde_json::Value::Null)
        }
        Some(serde_json::Value::Object(map)) => map
            .get(&flat.to_string())
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        // A plain string applies to all values (JSON-stat 2.0)
        Some(serde_json::Value::String(s)) => serde_json::Value::String(s.clone()),
        _ => serde_json::Value::Null,
    }
}

/// Validate that a JS float is a valid non-negative integer index and
/// convert it to `usize`.
///
/// JS numbers always arrive as `f64` via [`JsValue::as_f64()`]. A naive
/// `num as usize` cast is unsafe:
/// - **fractions** silently truncate (`2.7 → 2`),
/// - **negatives saturate to `0`** (`-1.0 → 0`), yielding a *silent wrong
///   result* (e.g. returning element 0 instead of erroring),
/// - **values beyond `usize::MAX`** overflow / saturate.
///
/// This helper rejects all of the above so that `Data()`, `Dimension()`,
/// `Category()` and `Item()` behave consistently. Returns a `&'static str`
/// error (pure Rust, host-testable); call sites map it to a [`JsValue`] —
/// mirroring the [`resolve_flat_index`] convention.
fn index_from_f64(num: f64) -> Result<usize, &'static str> {
    if num < 0.0 || num.fract() != 0.0 || num > usize::MAX as f64 {
        return Err("Index must be a non-negative integer");
    }
    Ok(num as usize)
}

/// Resolve a flat-index query from a {dim_id: cat_id} map.
/// Auto-fills constant (size-1) dimensions when missing from the query.
/// Uses String errors for testability on non-wasm targets.
fn resolve_flat_index(
    dataset: &Dataset,
    query: &HashMap<String, String>,
) -> Result<usize, String> {
    let dim_ids = dataset
        .id
        .as_ref()
        .ok_or_else(|| "Dataset is missing 'id' array".to_string())?;
    let sizes = dataset
        .size
        .as_ref()
        .ok_or_else(|| "Dataset is missing 'size' array".to_string())?;
    let dimensions = dataset
        .dimension
        .as_ref()
        .ok_or_else(|| "Dataset is missing 'dimension' object".to_string())?;

    if dim_ids.len() != sizes.len() {
        return Err("Length of 'id' and 'size' arrays do not match".to_string());
    }

    let mut indices = Vec::with_capacity(dim_ids.len());

    for (i, dim_id) in dim_ids.iter().enumerate() {
        let cat_id = match query.get(dim_id) {
            Some(id) => id.clone(),
            None => {
                // Auto-fill constant (size-1) dimensions
                if sizes[i] == 1 {
                    let dim = dimensions.get(dim_id).ok_or_else(|| {
                        format!("Dataset is missing dimension object for '{}'", dim_id)
                    })?;
                    category_id_at(dim, 0).ok_or_else(|| {
                        format!("Dimension '{}' has no categories", dim_id)
                    })?
                } else {
                    return Err(format!(
                        "Query is missing category for non-constant dimension '{}'",
                        dim_id
                    ));
                }
            }
        };

        let dim = dimensions.get(dim_id).ok_or_else(|| {
            format!("Dataset is missing dimension object for '{}'", dim_id)
        })?;

        let cat_idx = resolve_category_index(dim, &cat_id)?;
        indices.push(cat_idx);
    }

    query::calculate_index(&indices, sizes).ok_or_else(|| {
        "Failed to calculate row-major order index".to_string()
    })
}

/// Outcome of resolving a [`Data()`](JSONstat::data) object query.
///
/// Mirrors the JSON-stat Toolkit semantics (toolkit-api.md, "Data()"):
///
/// - [`QueryResolution::Single`] — every non-constant dimension was pinned to a
///   valid category (constant dimensions may be omitted and are auto-filled).
/// - [`QueryResolution::Slice`] — exactly one non-constant dimension was left
///   free (missing or pointing at an unknown category). The toolkit returns an
///   array of value-status objects: one per category of the free dimension, in
///   category order.
/// - [`QueryResolution::Null`] — more than one non-constant dimension is free;
///   the toolkit returns `null`.
///
/// Unknown dimension IDs in the query are ignored entirely (they neither pin
/// nor free a real dimension). Constant (size-1) dimensions are never free:
/// when their category is missing/invalid they are simply auto-filled.
#[derive(Debug)]
enum QueryResolution {
    /// Fully-resolved single cell (flat, row-major index).
    Single(usize),
    /// One free dimension: iterate its category indices.
    Slice {
        free_dim_idx: usize,
        /// Resolved index for every dimension (the entry at `free_dim_idx`
        /// is overwritten in turn by each value of `free_cat_indices`).
        fixed: Vec<usize>,
        /// Candidate category indices for the free dimension, in order.
        free_cat_indices: Vec<usize>,
    },
    /// More than one free dimension → return JS `null`.
    Null,
}

/// Resolve a `{dim_id: cat_id}` object query following the Toolkit's partial
/// query rules. Pure-Rust (no `JsValue`) so it is host-testable.
fn resolve_query(
    dataset: &Dataset,
    query: &HashMap<String, String>,
) -> Result<QueryResolution, String> {
    let dim_ids = dataset
        .id
        .as_ref()
        .ok_or_else(|| "Dataset is missing 'id' array".to_string())?;
    let sizes = dataset
        .size
        .as_ref()
        .ok_or_else(|| "Dataset is missing 'size' array".to_string())?;
    let dimensions = dataset
        .dimension
        .as_ref()
        .ok_or_else(|| "Dataset is missing 'dimension' object".to_string())?;

    let mut fixed = vec![0usize; dim_ids.len()];
    let mut free: Vec<usize> = Vec::new();

    for (i, dim_id) in dim_ids.iter().enumerate() {
        let dim = dimensions.get(dim_id).ok_or_else(|| {
            format!("Dataset is missing dimension object for '{}'", dim_id)
        })?;
        let is_constant = sizes[i] == 1;

        match query.get(dim_id) {
            // A category was supplied for this (valid) dimension.
            Some(cat_id) => match resolve_category_index(dim, cat_id) {
                Ok(idx) => fixed[i] = idx,
                // Unknown category ID → the dimension's constraint is ignored:
                // constant dims are auto-filled, non-constant dims go free.
                Err(_) => {
                    if is_constant {
                        fixed[i] = 0;
                    } else {
                        free.push(i);
                    }
                }
            },
            // No entry for this dimension: constant dims auto-fill, others free.
            None => {
                if is_constant {
                    fixed[i] = 0;
                } else {
                    free.push(i);
                }
            }
        }
        // NOTE: query keys that are not real dimension IDs are simply never
        // visited by this loop, so they are ignored (Toolkit behavior).
    }

    match free.len() {
        0 => {
            let idx = query::calculate_index(&fixed, sizes)
                .ok_or_else(|| "Failed to calculate row-major order index".to_string())?;
            Ok(QueryResolution::Single(idx))
        }
        1 => {
            let fi = free[0];
            let free_cat_indices: Vec<usize> = (0..sizes[fi]).collect();
            Ok(QueryResolution::Slice {
                free_dim_idx: fi,
                fixed,
                free_cat_indices,
            })
        }
        _ => Ok(QueryResolution::Null),
    }
}

// ── Public API ────────────────────────────────────────────────────────────

#[wasm_bindgen]
impl JSONstat {
    /// Parses a JSON-stat string into a WebAssembly-managed object.
    #[wasm_bindgen(constructor)]
    pub fn new(json_str: &str) -> Result<JSONstat, JsValue> {
        let response: JsonStatResponse = serde_json::from_str(json_str)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse JSON-stat: {}", e)))?;
        Ok(JSONstat { response })
    }

    // ── Property Getters ──────────────────────────────────────────────

    /// Returns the JSON-stat version.
    #[wasm_bindgen(getter)]
    pub fn version(&self) -> String {
        self.response.version().to_string()
    }

    /// Returns the class (dataset, dimension, collection).
    #[wasm_bindgen(getter)]
    pub fn class(&self) -> String {
        self.response.class().to_string()
    }

    /// Returns the label, if present.
    #[wasm_bindgen(getter)]
    pub fn label(&self) -> Option<String> {
        match &self.response {
            JsonStatResponse::Dataset(d) => d.label.clone(),
            JsonStatResponse::Dimension(d) => d.label.clone(),
            JsonStatResponse::Collection(c) => c.label.clone(),
        }
    }

    /// Returns the source, if present.
    #[wasm_bindgen(getter)]
    pub fn source(&self) -> Option<String> {
        match &self.response {
            JsonStatResponse::Dataset(d) => d.source.clone(),
            JsonStatResponse::Collection(c) => c.source.clone(),
            _ => None,
        }
    }

    /// Returns the update date string, if present.
    #[wasm_bindgen(getter)]
    pub fn updated(&self) -> Option<String> {
        match &self.response {
            JsonStatResponse::Dataset(d) => d.updated.clone(),
            JsonStatResponse::Collection(c) => c.updated.clone(),
            _ => None,
        }
    }

    /// Returns the href, if present.
    #[wasm_bindgen(getter)]
    pub fn href(&self) -> Option<String> {
        match &self.response {
            JsonStatResponse::Dataset(d) => d.href.clone(),
            JsonStatResponse::Dimension(d) => d.href.clone(),
            JsonStatResponse::Collection(c) => c.href.clone(),
        }
    }

    /// Returns the number of values in the dataset (product of sizes).
    #[wasm_bindgen(getter)]
    pub fn n(&self) -> usize {
        match &self.response {
            JsonStatResponse::Dataset(d) => {
                d.size.as_ref().map(|s| s.iter().product()).unwrap_or(0)
            }
            _ => 0,
        }
    }

    /// Returns the dimension sizes array.
    #[wasm_bindgen(getter)]
    pub fn size(&self) -> Vec<usize> {
        match &self.response {
            JsonStatResponse::Dataset(d) => d.size.clone().unwrap_or_default(),
            _ => vec![],
        }
    }

    /// Returns the dimension IDs array.
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> Vec<String> {
        match &self.response {
            JsonStatResponse::Dataset(d) => d.id.clone().unwrap_or_default(),
            _ => vec![],
        }
    }

    /// Returns the number of dimensions (dataset), categories (dimension), or items (collection).
    #[wasm_bindgen(getter)]
    pub fn length(&self) -> usize {
        match &self.response {
            JsonStatResponse::Dataset(d) => {
                d.dimension.as_ref().map(|dims| dims.len()).unwrap_or(0)
            }
            JsonStatResponse::Dimension(d) => d
                .category
                .as_ref()
                .and_then(|c| c.index.as_ref())
                .map(|idx| idx.len())
                .unwrap_or(0),
            JsonStatResponse::Collection(c) => c
                .link
                .as_ref()
                .and_then(|l| l.get("item"))
                .map(|items| items.len())
                .unwrap_or(0),
        }
    }

    /// Returns the error information, if present.
    #[wasm_bindgen(getter)]
    pub fn error(&self) -> JsValue {
        match &self.response {
            JsonStatResponse::Dataset(d) => d
                .error
                .as_ref()
                .map(to_js_value)
                .unwrap_or(JsValue::NULL),
            _ => JsValue::NULL,
        }
    }

    /// Returns the extension object, if present.
    #[wasm_bindgen(getter)]
    pub fn extension(&self) -> JsValue {
        match &self.response {
            JsonStatResponse::Dataset(d) => d
                .extension
                .as_ref()
                .map(to_js_value)
                .unwrap_or(JsValue::NULL),
            JsonStatResponse::Dimension(d) => d
                .extension
                .as_ref()
                .map(to_js_value)
                .unwrap_or(JsValue::NULL),
            JsonStatResponse::Collection(c) => c
                .extension
                .as_ref()
                .map(to_js_value)
                .unwrap_or(JsValue::NULL),
        }
    }

    /// Returns notes, if present.
    #[wasm_bindgen(getter)]
    pub fn note(&self) -> JsValue {
        match &self.response {
            JsonStatResponse::Dataset(d) => d
                .note
                .as_ref()
                .map(to_js_value)
                .unwrap_or(JsValue::NULL),
            JsonStatResponse::Dimension(d) => d
                .note
                .as_ref()
                .map(to_js_value)
                .unwrap_or(JsValue::NULL),
            JsonStatResponse::Collection(c) => c
                .note
                .as_ref()
                .map(to_js_value)
                .unwrap_or(JsValue::NULL),
        }
    }

    /// Returns the link object, if present.
    #[wasm_bindgen(getter)]
    pub fn link(&self) -> JsValue {
        match &self.response {
            JsonStatResponse::Dataset(d) => d
                .link
                .as_ref()
                .map(to_js_value)
                .unwrap_or(JsValue::NULL),
            JsonStatResponse::Dimension(d) => d
                .link
                .as_ref()
                .map(to_js_value)
                .unwrap_or(JsValue::NULL),
            JsonStatResponse::Collection(c) => c
                .link
                .as_ref()
                .map(to_js_value)
                .unwrap_or(JsValue::NULL),
        }
    }

    /// Returns the role mapping, if present.
    #[wasm_bindgen(getter)]
    pub fn role(&self) -> JsValue {
        match &self.response {
            JsonStatResponse::Dataset(d) => d
                .role
                .as_ref()
                .map(to_js_value)
                .unwrap_or(JsValue::NULL),
            _ => JsValue::NULL,
        }
    }

    /// Returns the status array/object, if present.
    #[wasm_bindgen(getter)]
    pub fn status(&self) -> JsValue {
        match &self.response {
            JsonStatResponse::Dataset(d) => d
                .status
                .as_ref()
                .map(to_js_value)
                .unwrap_or(JsValue::NULL),
            _ => JsValue::NULL,
        }
    }

    /// Returns the values array (numbers, strings or nulls) as a dense array.
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> JsValue {
        let dataset = match require_dataset(&self.response) {
            Ok(d) => d,
            Err(_) => return JsValue::NULL,
        };
        match &dataset.value {
            Some(DatasetValue::Array(arr)) => to_js_value(arr),
            Some(value @ DatasetValue::Map(_)) => {
                // Convert sparse map to dense array
                let len: usize = dataset
                    .size
                    .as_ref()
                    .map(|s| s.iter().product())
                    .unwrap_or(0);
                let arr: Vec<Cell> = (0..len).map(|i| value.get_at(i)).collect();
                to_js_value(&arr)
            }
            None => JsValue::NULL,
        }
    }

    // ── Value Access ──────────────────────────────────────────────────

    /// Gets a value for a specific combination of dimension categories.
    /// Constant (size-1) dimensions can be omitted from the query.
    /// Returns a number, a string, or null (JSON-stat values may be strings).
    #[wasm_bindgen(js_name = "Datum")]
    pub fn datum(&self, query_js: JsValue) -> Result<JsValue, JsValue> {
        let query: HashMap<String, String> = serde_wasm_bindgen::from_value(query_js)
            .map_err(|e| JsValue::from_str(&format!("Invalid query format: {}", e)))?;

        let dataset = require_dataset(&self.response)?;
        let value = dataset
            .value
            .as_ref()
            .ok_or_else(|| JsValue::from_str("Dataset is missing 'value' property"))?;

        let flat_index = resolve_flat_index(dataset, &query)
            .map_err(|e| JsValue::from_str(&e))?;

        if let DatasetValue::Array(arr) = value {
            if flat_index >= arr.len() {
                return Err(JsValue::from_str(
                    "Calculated index is out of bounds for value array",
                ));
            }
        }

        to_js_value_result(&value.get_at(flat_index))
    }

    // ── Data() ────────────────────────────────────────────────────────

    /// Gets data information. Supports:
    /// - No arguments: array of {value, status} for all cells
    /// - Integer: {value, status} at that flat index
    /// - Array [i, j, k]: {value, status} at those dimension indices
    /// - Object {dim: cat}: {value, status} by dimension/category IDs
    ///
    /// When `status` is false, returns just the value(s) instead of objects.
    #[wasm_bindgen(js_name = "Data")]
    pub fn data(
        &self,
        dataid_js: JsValue,
        include_status: Option<bool>,
    ) -> Result<JsValue, JsValue> {
        let want_status = include_status.unwrap_or(true);
        let dataset = require_dataset(&self.response)?;
        let values = dataset
            .value
            .as_ref()
            .ok_or_else(|| JsValue::from_str("Dataset has no values"))?;

        // No argument → all data
        if dataid_js.is_undefined() || dataid_js.is_null() {
            let sizes = dataset.size.as_ref().ok_or_else(|| {
                JsValue::from_str("Dataset is missing 'size' array")
            })?;
            let total: usize = sizes.iter().product();
            let mut result = Vec::with_capacity(total);
            for i in 0..total {
                let val = values.get_at(i);
                if want_status {
                    let st = status_at(dataset, i);
                    result.push(serde_json::json!({
                        "value": val,
                        "status": st
                    }));
                } else {
                    result.push(serde_json::json!(val));
                }
            }
            return to_js_value_result(&result);
        }

        // Try as integer (flat index)
        if let Some(num) = dataid_js.as_f64() {
            let idx = index_from_f64(num).map_err(JsValue::from_str)?;
            let total: usize = dataset
                .size
                .as_ref()
                .map(|s| s.iter().product())
                .unwrap_or(0);
            if idx >= total {
                return Err(JsValue::from_str(&format!(
                    "Flat index {} out of bounds (dataset has {} values)",
                    idx, total
                )));
            }
            let val = values.get_at(idx);
            if want_status {
                let st = status_at(dataset, idx);
                let obj = serde_json::json!({
                    "value": val,
                    "status": st
                });
                return to_js_value_result(&obj);
            } else {
                return to_js_value_result(&val);
            }
        }

        // Try as array of dimension indices
        if js_sys::Array::is_array(&dataid_js) {
            let arr: Vec<usize> = serde_wasm_bindgen::from_value(dataid_js.clone())
                .map_err(|e| JsValue::from_str(&format!("Invalid array format: {}", e)))?;
            let sizes = dataset.size.as_ref().ok_or_else(|| {
                JsValue::from_str("Dataset is missing 'size' array")
            })?;
            let flat = query::calculate_index(&arr, sizes).ok_or_else(|| {
                JsValue::from_str("Invalid dimension indices")
            })?;
            let val = values.get_at(flat);
            if want_status {
                let st = status_at(dataset, flat);
                let obj = serde_json::json!({
                    "value": val,
                    "status": st
                });
                return to_js_value_result(&obj);
            } else {
                return to_js_value_result(&val);
            }
        }

        // Try as object {dim_id: cat_id} (Toolkit "Data()" partial queries).
        //
        // A non-constant dimension left unspecified — or pinned to an unknown
        // category — becomes a "free" dimension:
        //   * 0 free  → a single value-status object (or value).
        //   * 1 free  → an array (a "slice") across the free dimension, in
        //               category order.
        //   * >1 free → null.
        // Unknown dimension IDs in the query are ignored. Constant (size-1)
        // dimensions are always auto-filled and never go free.
        let query: HashMap<String, String> = serde_wasm_bindgen::from_value(dataid_js)
            .map_err(|e| JsValue::from_str(&format!("Invalid query format: {}", e)))?;
        let sizes = dataset.size.as_ref().ok_or_else(|| {
            JsValue::from_str("Dataset is missing 'size' array")
        })?;
        let resolution = resolve_query(dataset, &query)
            .map_err(|e| JsValue::from_str(&e))?;

        match resolution {
            QueryResolution::Null => Ok(JsValue::NULL),
            QueryResolution::Single(flat) => {
                let val = values.get_at(flat);
                if want_status {
                    let st = status_at(dataset, flat);
                    to_js_value_result(&serde_json::json!({ "value": val, "status": st }))
                } else {
                    to_js_value_result(&val)
                }
            }
            QueryResolution::Slice {
                free_dim_idx,
                mut fixed,
                free_cat_indices,
            } => {
                let mut out = Vec::with_capacity(free_cat_indices.len());
                for ci in free_cat_indices {
                    fixed[free_dim_idx] = ci;
                    let flat = query::calculate_index(&fixed, sizes).ok_or_else(|| {
                        JsValue::from_str("Failed to calculate row-major order index")
                    })?;
                    let val = values.get_at(flat);
                    if want_status {
                        let st = status_at(dataset, flat);
                        out.push(serde_json::json!({ "value": val, "status": st }));
                    } else {
                        out.push(serde_json::json!(val));
                    }
                }
                to_js_value_result(&out)
            }
        }
    }

    // ── Dimension() ───────────────────────────────────────────────────

    /// Gets dimension information. Supports:
    /// - No argument: array of all dimensions
    /// - Integer: dimension at that index
    /// - String: dimension with that ID
    /// - Object {role: "time"}: dimensions with that role
    ///
    /// `instance` (default `true`): when `false`, returns category label arrays
    /// instead of dimension-info objects (the toolkit "instance" form).
    /// - `Dimension(id, false)`  → `[label, label, ...]` for one dimension.
    /// - `Dimension({role}, false)` → `[[...], [...]]` (one array per dim).
    /// - `Dimension()` (no id) ignores `instance`.
    #[wasm_bindgen(js_name = "Dimension")]
    pub fn dimension(
        &self,
        dimid_js: JsValue,
        instance: Option<bool>,
    ) -> Result<JsValue, JsValue> {
        let dataset = require_dataset(&self.response)?;
        let dim_ids = dataset.id.as_ref().ok_or_else(|| {
            JsValue::from_str("Dataset is missing 'id' array")
        })?;
        let dimensions = dataset.dimension.as_ref().ok_or_else(|| {
            JsValue::from_str("Dataset is missing 'dimension' object")
        })?;

        let build_dim_info = |dim_id: &str, dim: &Dimension| -> serde_json::Value {
            let cat_ids = category_ids_of(dim);
            let role_val = resolve_role(dataset, dim_id);

            // Build categories array with detail (shared builder)
            let categories: Vec<serde_json::Value> = cat_ids
                .iter()
                .enumerate()
                .map(|(i, id)| build_cat_info(dim, id, i))
                .collect();

            let mut obj = serde_json::Map::new();
            obj.insert("class".to_string(), serde_json::Value::String("dimension".to_string()));
            obj.insert("id".to_string(), serde_json::to_value(&cat_ids).unwrap());
            obj.insert("label".to_string(), serde_json::Value::String(
                dim.label.clone().unwrap_or_else(|| dim_id.to_string()),
            ));
            obj.insert("length".to_string(), serde_json::Value::Number(cat_ids.len().into()));
            if let Some(r) = role_val {
                obj.insert("role".to_string(), serde_json::Value::String(r));
            }
            obj.insert("categories".to_string(), serde_json::Value::Array(categories));
            if let Some(ref note) = dim.note {
                obj.insert("note".to_string(), serde_json::to_value(note).unwrap());
            }
            if let Some(ref href) = dim.href {
                obj.insert("href".to_string(), serde_json::Value::String(href.clone()));
            }
            serde_json::Value::Object(obj)
        };

        // No argument → all dimensions
        if dimid_js.is_undefined() || dimid_js.is_null() {
            let all: Vec<serde_json::Value> = dim_ids
                .iter()
                .filter_map(|id| dimensions.get(id).map(|d| build_dim_info(id, d)))
                .collect();
            return to_js_value_result(&all);
        }

        let want_instance = instance.unwrap_or(true);

        // Try as integer index
        if let Some(num) = dimid_js.as_f64() {
            let idx = index_from_f64(num).map_err(JsValue::from_str)?;
            let dim_id = dim_ids.get(idx).ok_or_else(|| {
                JsValue::from_str(&format!("Dimension index {} out of bounds", idx))
            })?;
            let dim = dimensions.get(dim_id).ok_or_else(|| {
                JsValue::from_str(&format!("Dimension '{}' not found", dim_id))
            })?;
            if !want_instance {
                return to_js_value_result(&category_labels_of(dim));
            }
            // Return a callable DimensionInstance (clones dimension metadata +
            // resolved role). Owned data → no borrow hazard, auto-dropped on GC.
            let inst = DimensionInstance {
                dim_id: dim_id.clone(),
                dim: dim.clone(),
                role: resolve_role(dataset, dim_id),
            };
            return Ok(JsValue::from(inst));
        }

        // Try as string ID
        if let Some(s) = dimid_js.as_string() {
            let dim = dimensions.get(&s).ok_or_else(|| {
                JsValue::from_str(&format!("Dimension '{}' not found", s))
            })?;
            if !want_instance {
                return to_js_value_result(&category_labels_of(dim));
            }
            let inst = DimensionInstance {
                dim_id: s.clone(),
                dim: dim.clone(),
                role: resolve_role(dataset, &s),
            };
            return Ok(JsValue::from(inst));
        }

        // Try as {role: "time"} object
        let role_query: HashMap<String, String> = serde_wasm_bindgen::from_value(dimid_js)
            .map_err(|e| JsValue::from_str(&format!("Invalid dimension query: {}", e)))?;
        if let Some(role_name) = role_query.get("role") {
            let role_ids: Vec<String> = dataset
                .role
                .as_ref()
                .and_then(|roles| roles.get(role_name))
                .cloned()
                .unwrap_or_default();
            if !want_instance {
                // Array of arrays: one category-label array per role dimension.
                let labels: Vec<Vec<String>> = role_ids
                    .iter()
                    .filter_map(|id| dimensions.get(id).map(category_labels_of))
                    .collect();
                return to_js_value_result(&labels);
            }
            let role_dims: Vec<serde_json::Value> = role_ids
                .iter()
                .filter_map(|id| {
                    dimensions.get(id).map(|d| build_dim_info(id, d))
                })
                .collect();
            return to_js_value_result(&role_dims);
        }

        Err(JsValue::from_str("Invalid dimension identifier"))
    }

       // NOTE: Category() is no longer a dataset-scoped method. It now lives
       // on the DimensionInstance returned by Dimension(id) — i.e. the toolkit
       // anatomy `ds.Dimension(id).Category(catid)`. See `DimensionInstance`
       // below. Category() resolution reuses the free `build_cat_info` helper.

    // ── Item() ────────────────────────────────────────────────────────

    /// Gets item information from a collection. Supports:
    /// - No argument: array of all items
    /// - Integer: item at that index
    /// - Object `{class, embedded?}`: filter items by class (and, for
    ///   `class: "dataset"`, by whether they are embedded — have inline
    ///   extension data — when `embedded` is a boolean).
    #[wasm_bindgen(js_name = "Item")]
    pub fn item(&self, itemid_js: JsValue) -> Result<JsValue, JsValue> {
        let collection = match &self.response {
            JsonStatResponse::Collection(c) => c,
            _ => {
                return Err(JsValue::from_str(
                    "Item() is only supported for collection class",
                ))
            }
        };

        let items = collection
            .link
            .as_ref()
            .and_then(|l| l.get("item"))
            .ok_or_else(|| JsValue::from_str("Collection has no items"))?;

        let build_item = |item: &CollectionItem| -> serde_json::Value {
            let mut obj = serde_json::Map::new();
            if let Some(ref class) = item.class {
                obj.insert("class".to_string(), serde_json::Value::String(class.clone()));
            }
            if let Some(ref href) = item.href {
                obj.insert("href".to_string(), serde_json::Value::String(href.clone()));
            }
            if let Some(ref label) = item.label {
                obj.insert("label".to_string(), serde_json::Value::String(label.clone()));
            }
            if let Some(ref ext) = item.extension {
                obj.insert("extension".to_string(), ext.clone());
            }
            serde_json::Value::Object(obj)
        };

        // No argument → all items
        if itemid_js.is_undefined() || itemid_js.is_null() {
            let all: Vec<serde_json::Value> =
                items.iter().map(&build_item).collect();
            return to_js_value_result(&all);
        }

        // Integer index
        if let Some(num) = itemid_js.as_f64() {
            let idx = index_from_f64(num).map_err(JsValue::from_str)?;
            let item = items.get(idx).ok_or_else(|| {
                JsValue::from_str(&format!("Item index {} out of bounds", idx))
            })?;
            return to_js_value_result(&build_item(item));
        }

        // Object filter `{class, embedded?}` (toolkit Item() filter form).
        if itemid_js.is_object() {
            let filter: ItemFilter = serde_wasm_bindgen::from_value(itemid_js)
                .map_err(|e| JsValue::from_str(&format!("Invalid item filter: {}", e)))?;
            let class = filter.class.as_deref().unwrap_or("");
            // An invalid class value yields an empty array (toolkit behavior).
            let is_valid_class = matches!(class, "dataset" | "collection" | "dimension" | "bundle");
            if !is_valid_class {
                let empty: Vec<serde_json::Value> = Vec::new();
                return to_js_value_result(&empty);
            }
            let embedded = filter.embedded;
            let matched: Vec<serde_json::Value> = items
                .iter()
                .filter(|it| {
                    // Class must match.
                    let class_match = it.class.as_deref() == Some(class);
                    if !class_match {
                        return false;
                    }
                    // `embedded` only applies to "dataset" items.
                    if class != "dataset" {
                        return true;
                    }
                    // An item is "embedded" when it carries inline data via
                    // its `extension`. (We follow the toolkit's practical
                    // definition: an embedded dataset item has its payload in
                    // the collection rather than just an href pointer.)
                    let has_payload = it.extension.is_some();
                    match embedded {
                        Some(true) => has_payload,
                        Some(false) => !has_payload,
                        None => true,
                    }
                })
                .map(build_item)
                .collect();
            return to_js_value_result(&matched);
        }

        Err(JsValue::from_str("Invalid item identifier"))
    }

    // ── Unflatten() ───────────────────────────────────────────────────

    /// Iterates over all cells calling the provided JS callback with
    /// (coordinates, datapoint, n, row) for each cell.
    /// Returns the accumulated array of callback results.
    #[wasm_bindgen(js_name = "Unflatten")]
    pub fn unflatten(&self, callback_js: JsValue) -> Result<JsValue, JsValue> {
        let dataset = require_dataset(&self.response)?;
        let dim_ids = dataset
            .id
            .as_ref()
            .ok_or_else(|| JsValue::from_str("Dataset is missing 'id' array"))?;
        let sizes = dataset
            .size
            .as_ref()
            .ok_or_else(|| JsValue::from_str("Dataset is missing 'size' array"))?;
        let dimensions = dataset.dimension.as_ref().ok_or_else(|| {
            JsValue::from_str("Dataset is missing 'dimension' object")
        })?;
        let values = dataset
            .value
            .as_ref()
            .ok_or_else(|| JsValue::from_str("Dataset has no values"))?;

        let callback = callback_js
            .dyn_into::<js_sys::Function>()
            .map_err(|_| JsValue::from_str("Unflatten requires a callback function"))?;

        // Precompute category IDs per dimension (avoids per-cell lookups)
        let dim_cat_ids: Vec<Vec<String>> = dim_ids
            .iter()
            .map(|dim_id| {
                dimensions
                    .get(dim_id)
                    .map(category_ids_of)
                    .unwrap_or_default()
            })
            .collect();

        let row = js_sys::Array::new();

        for (n, indices) in query::index_iter(sizes).enumerate() {
            // Build coordinates object
            let mut coords = serde_json::Map::new();
            for (dim_idx, cat_pos) in indices.iter().enumerate() {
                if let Some(cat_id) = dim_cat_ids[dim_idx].get(*cat_pos) {
                    coords.insert(
                        dim_ids[dim_idx].clone(),
                        serde_json::Value::String(cat_id.clone()),
                    );
                }
            }

            // Build datapoint object (row-major iteration → flat index == n)
            let val = values.get_at(n);
            let st = status_at(dataset, n);
            let mut datapoint = serde_json::Map::new();
            datapoint.insert("value".to_string(), serde_json::json!(val));
            datapoint.insert("status".to_string(), st);

            let coords_js = to_js_value(&serde_json::Value::Object(coords));
            let dp_js = to_js_value(&serde_json::Value::Object(datapoint));
            // Cast through f64 (exact for all practical datasets; values
            // beyond 2^53 would lose precision, but `as u32` wrapped at 4B).
            let n_js = JsValue::from(n);

            let result = callback.call4(&JsValue::NULL, &coords_js, &dp_js, &n_js, &row)?;

            if !result.is_undefined() {
                row.push(&result);
            }
        }

        Ok(row.into())
    }

    // ── Transform() ───────────────────────────────────────────────────

    /// Converts the dataset into tabular form.
    ///
    /// Options (passed as a JS object):
    /// - type: "arrobj" (default) | "array" | "objarr"
    /// - status: boolean (default false)
    /// - content: "id" | "label" (default "label")
    /// - field: "id" | "label" (default depends on type)
    /// - vlabel: string (default "Value")
    /// - slabel: string (default "Status")
    /// - drop: array of dimension IDs to exclude
    /// - by: dimension ID to pivot on (arrobj/objarr only)
    #[wasm_bindgen(js_name = "Transform")]
    pub fn transform(&self, opts_js: Option<JsValue>) -> Result<JsValue, JsValue> {
        let dataset = require_dataset(&self.response)?;
        let dim_ids = dataset
            .id
            .as_ref()
            .ok_or_else(|| JsValue::from_str("Dataset is missing 'id' array"))?;
        let sizes = dataset
            .size
            .as_ref()
            .ok_or_else(|| JsValue::from_str("Dataset is missing 'size' array"))?;
        let dimensions = dataset.dimension.as_ref().ok_or_else(|| {
            JsValue::from_str("Dataset is missing 'dimension' object")
        })?;
        let values = dataset
            .value
            .as_ref()
            .ok_or_else(|| JsValue::from_str("Dataset has no values"))?;

        // Parse options
        let opts: TransformOpts = match opts_js {
            Some(js) => serde_wasm_bindgen::from_value(js)
                .map_err(|e| JsValue::from_str(&format!("Invalid transform options: {}", e)))?,
            None => TransformOpts::default(),
        };

        let type_ = opts.type_.as_deref().unwrap_or("array");
        let include_status = opts.status.unwrap_or(false);
        let content = opts.content.as_deref().unwrap_or("label");
        let field = opts.field.as_deref();
        let vlabel = opts.vlabel.as_deref().unwrap_or("Value");
        let slabel = opts.slabel.as_deref().unwrap_or("Status");
        let drop = opts.drop.unwrap_or_default();
        let by = opts.by.as_deref();
        let want_meta = opts.meta.unwrap_or(false);
        let comma = opts.comma.unwrap_or(false);
        let prefix = opts.prefix.as_deref().unwrap_or("");

        // Resolve field default based on type
        let field_resolved = match field {
            Some(f) => f.to_string(),
            None => {
                if type_ == "array" {
                    "label".to_string()
                } else {
                    "id".to_string()
                }
            }
        };

        // Determine which dimensions to include (as (dim_index, dim_id_string) pairs)
        let included_dim_indices: Vec<usize> = dim_ids
            .iter()
            .enumerate()
            .filter(|(_, id)| !drop.contains(id))
            .map(|(i, _)| i)
            .collect();

        // Check if by dimension is valid (by is not available for array/object)
        let by_dim_idx = if type_ == "arrobj" || type_ == "objarr" {
            by.and_then(|b| dim_ids.iter().position(|d| d == b))
        } else {
            None
        };
        let effective_by = by_dim_idx.and(by);

        let data = match type_ {
            "arrobj" => {
                if let Some(bdi) = by_dim_idx {
                    transform_arrobj_by(
                        dataset,
                        dim_ids,
                        sizes,
                        dimensions,
                        values,
                        &included_dim_indices,
                        bdi,
                        content,
                        &field_resolved,
                        vlabel,
                        comma,
                        prefix,
                    )
                } else {
                    transform_arrobj(
                        dataset,
                        dim_ids,
                        sizes,
                        dimensions,
                        values,
                        &included_dim_indices,
                        include_status,
                        content,
                        &field_resolved,
                        vlabel,
                        slabel,
                        comma,
                    )
                }
            }
            "array" => transform_array(
                dataset,
                dim_ids,
                sizes,
                dimensions,
                values,
                &included_dim_indices,
                include_status,
                content,
                &field_resolved,
                vlabel,
                slabel,
                comma,
            ),
            "objarr" => {
                if let Some(bdi) = by_dim_idx {
                    transform_objarr_by(
                        dataset,
                        dim_ids,
                        sizes,
                        dimensions,
                        values,
                        &included_dim_indices,
                        bdi,
                        content,
                        &field_resolved,
                        vlabel,
                        comma,
                        prefix,
                    )
                } else {
                    transform_objarr(
                        dataset,
                        dim_ids,
                        sizes,
                        dimensions,
                        values,
                        &included_dim_indices,
                        include_status,
                        content,
                        &field_resolved,
                        vlabel,
                        slabel,
                        comma,
                    )
                }
            }
            "object" => transform_object(
                dataset,
                dim_ids,
                sizes,
                dimensions,
                values,
                &included_dim_indices,
                include_status,
                content,
                &field_resolved,
                vlabel,
                slabel,
            ),
            _ => {
                return Err(JsValue::from_str(&format!(
                    "Unsupported transform type: '{}'",
                    type_
                )))
            }
        }?;

        // meta is not available for the "object" type
        let result = if want_meta && type_ != "object" {
            let meta = build_meta(
                dataset,
                dim_ids,
                type_,
                include_status,
                effective_by,
                &drop,
                prefix,
                comma,
            );
            serde_json::json!({ "meta": meta, "data": data })
        } else {
            data
        };

        to_js_value_result(&result)
    }

    // ── Dice() ────────────────────────────────────────────────────────

    /// Filters a dataset keeping only the specified dimension categories.
    /// Returns a new JSONstat with the subset.
    ///
    /// Filter can be an object {dim_id: [cat_ids]} or an array of [dim_id, [cat_ids]].
    /// Options: { drop: bool } — when true, filter specifies categories to remove.
    #[wasm_bindgen(js_name = "Dice")]
    pub fn dice(&self, filter_js: JsValue, opts_js: Option<JsValue>) -> Result<JSONstat, JsValue> {
        let dataset = require_dataset(&self.response)?;

        // Parse options
        let mut invert = false;
        if let Some(js) = opts_js {
            let opts: HashMap<String, serde_json::Value> =
                serde_wasm_bindgen::from_value(js)
                    .map_err(|e| JsValue::from_str(&format!("Invalid dice options: {}", e)))?;
            if let Some(serde_json::Value::Bool(b)) = opts.get("drop") {
                invert = *b;
            }
        }

        // Parse filter
        let filter: HashMap<String, Vec<String>> = parse_dice_filter(filter_js)?;

        let new_dataset = dice_dataset(dataset, &filter, invert)
            .map_err(|e| JsValue::from_str(&e))?;

        Ok(JSONstat {
            response: JsonStatResponse::Dataset(new_dataset),
        })
    }

    // ── ToJSON() ──────────────────────────────────────────────────────

    /// Serializes the current state back to a JSON-stat string.
    #[wasm_bindgen(js_name = "ToJSON")]
    pub fn to_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.response)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))
    }
}

// ── DimensionInstance (returned by Dimension(id)) ───────────────────────────

/// A callable dimension instance returned by `Dimension(id)`.
///
/// Mirrors the jsonstat-toolkit anatomy: `ds.Dimension("geo")` returns an
/// object that exposes the dimension's properties (`id`, `label`, `length`,
/// `role`, `categories`, …) **and** a `Category(catid)` method for drilling
/// into a single category.
///
/// The instance owns a clone of the underlying [`Dimension`] metadata plus the
/// resolved role string. It does **not** borrow from the parent `JSONstat`, so
/// there is no lifetime hazard, no reference cycle, and no leak: wasm-bindgen
/// drops the cloned data when JS garbage-collects the wrapper (same mechanism
/// `Dice()` relies on to return a typed `JSONstat`).
#[wasm_bindgen]
pub struct DimensionInstance {
    dim_id: String,
    dim: Dimension,
    role: Option<String>,
}

#[wasm_bindgen]
impl DimensionInstance {
    /// Always `"dimension"` (matches the toolkit `class` property).
    #[wasm_bindgen(getter)]
    pub fn class(&self) -> String {
        "dimension".to_string()
    }

    /// Category IDs in this dimension, in order.
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> Vec<String> {
        category_ids_of(&self.dim)
    }

    /// Dimension label (falls back to the dimension ID).
    #[wasm_bindgen(getter)]
    pub fn label(&self) -> String {
        self.dim
            .label
            .clone()
            .unwrap_or_else(|| self.dim_id.clone())
    }

    /// Number of categories.
    #[wasm_bindgen(getter)]
    pub fn length(&self) -> usize {
        category_ids_of(&self.dim).len()
    }

    /// Role (time/geo/metric/classification), if any.
    #[wasm_bindgen(getter)]
    pub fn role(&self) -> Option<String> {
        self.role.clone()
    }

    /// Array of category detail objects (`id`, `label`, `index`, `unit`,
    /// `coordinates`, `note`), in category order.
    #[wasm_bindgen(getter)]
    pub fn categories(&self) -> Result<JsValue, JsValue> {
        let cats: Vec<serde_json::Value> = category_ids_of(&self.dim)
            .iter()
            .enumerate()
            .map(|(i, id)| build_cat_info(&self.dim, id, i))
            .collect();
        to_js_value_result(&cats)
    }

    /// Dimension annotations, if present.
    #[wasm_bindgen(getter)]
    pub fn note(&self) -> JsValue {
        match &self.dim.note {
            Some(note) => to_js_value(note),
            None => JsValue::UNDEFINED,
        }
    }

    /// Dimension URL, if present.
    #[wasm_bindgen(getter)]
    pub fn href(&self) -> Option<String> {
        self.dim.href.clone()
    }

    // ── Category() ────────────────────────────────────────────────────

    /// Gets category information. Supports:
    /// - No catid: array of all categories for the dimension
    /// - Integer: category at that index
    /// - String: category with that ID
    #[wasm_bindgen(js_name = "Category")]
    pub fn category(&self, catid_js: JsValue) -> Result<JsValue, JsValue> {
        let all_ids = category_ids_of(&self.dim);

        // No argument → all categories
        if catid_js.is_undefined() || catid_js.is_null() {
            let cats: Vec<serde_json::Value> = all_ids
                .iter()
                .enumerate()
                .map(|(i, id)| build_cat_info(&self.dim, id, i))
                .collect();
            return to_js_value_result(&cats);
        }

        // Try as integer index
        if let Some(num) = catid_js.as_f64() {
            let pos = index_from_f64(num).map_err(JsValue::from_str)?;
            let cat_id = all_ids.get(pos).ok_or_else(|| {
                JsValue::from_str(&format!("Category index {} out of bounds", pos))
            })?;
            return to_js_value_result(&build_cat_info(&self.dim, cat_id, pos));
        }

        // Try as string ID
        if let Some(s) = catid_js.as_string() {
            let pos = all_ids.iter().position(|id| id == &s).ok_or_else(|| {
                JsValue::from_str(&format!(
                    "Category '{}' not found in dimension '{}'",
                    s, self.dim_id
                ))
            })?;
            return to_js_value_result(&build_cat_info(&self.dim, &s, pos));
        }

        Err(JsValue::from_str("Invalid category identifier"))
    }
}

// ── Item Filter (deserialized from JS) ─────────────────────────────────────

/// Deserialized `{class, embedded?}` filter for [`Item()`].
///
/// [`Item()`]: JSONstat::item
#[derive(Default, serde::Deserialize)]
struct ItemFilter {
    class: Option<String>,
    embedded: Option<bool>,
}

// ── Transform Options (deserialized from JS) ──────────────────────────────

#[derive(Default)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransformOpts {
    #[serde(rename = "type")]
    type_: Option<String>,
    status: Option<bool>,
    content: Option<String>,
    field: Option<String>,
    vlabel: Option<String>,
    slabel: Option<String>,
    drop: Option<Vec<String>>,
    by: Option<String>,
    meta: Option<bool>,
    prefix: Option<String>,
    comma: Option<bool>,
}

// ── Dice Core ──────────────────────────────────────────────────────────────

/// Pure-Rust core of Dice(): returns a new Dataset keeping (or dropping,
/// when `invert` is true) the specified dimension categories.
///
/// Kept categories always preserve the dataset's original category order,
/// so values and category indices stay aligned regardless of the order in
/// which the filter lists categories. Unknown dimension or category IDs in
/// the filter are reported as errors.
fn dice_dataset(
    dataset: &Dataset,
    filter: &HashMap<String, Vec<String>>,
    invert: bool,
) -> Result<Dataset, String> {
    let values = dataset
        .value
        .as_ref()
        .ok_or_else(|| "Dataset has no values".to_string())?;
    let dim_ids = dataset
        .id
        .as_ref()
        .ok_or_else(|| "Dataset is missing 'id' array".to_string())?;
    let sizes = dataset
        .size
        .as_ref()
        .ok_or_else(|| "Dataset is missing 'size' array".to_string())?;
    let dimensions = dataset
        .dimension
        .as_ref()
        .ok_or_else(|| "Dataset is missing 'dimension' object".to_string())?;

    // Validate that every filter dimension exists
    for filter_dim in filter.keys() {
        if !dim_ids.contains(filter_dim) {
            return Err(format!("Dimension '{}' not found", filter_dim));
        }
    }

    // For each dimension, determine which category indices to keep
    // (always in the dataset's original category order)
    let mut keep_indices: Vec<Vec<usize>> = Vec::with_capacity(dim_ids.len());

    for (i, dim_id) in dim_ids.iter().enumerate() {
        let dim = dimensions
            .get(dim_id)
            .ok_or_else(|| format!("Dimension '{}' not found", dim_id))?;
        let all_cat_ids = category_ids_of(dim);
        if all_cat_ids.is_empty() {
            return Err(format!("Dimension '{}' has no categories", dim_id));
        }

        if let Some(filtered_cats) = filter.get(dim_id) {
            // Validate that every filter category exists
            for cat_id in filtered_cats {
                if !all_cat_ids.contains(cat_id) {
                    return Err(format!(
                        "Category '{}' not found in dimension '{}'",
                        cat_id, dim_id
                    ));
                }
            }
            let kept: Vec<usize> = all_cat_ids
                .iter()
                .enumerate()
                .filter(|(_, id)| filtered_cats.contains(id) != invert)
                .map(|(pos, _)| pos)
                .collect();
            keep_indices.push(kept);
        } else {
            // No filter for this dimension: keep all
            keep_indices.push((0..sizes[i]).collect());
        }
    }

    // Build the new value array by iterating over kept combinations
    let all_combos = query::all_combinations(&keep_indices);
    let mut new_values = Vec::with_capacity(all_combos.len());
    let mut new_status = Vec::new();
    let has_status = dataset.status.is_some();
    if has_status {
        new_status.reserve(all_combos.len());
    }

    for combo in &all_combos {
        let flat = query::calculate_index(combo, sizes)
            .ok_or_else(|| "Failed to calculate row-major order index".to_string())?;
        new_values.push(values.get_at(flat));
        if has_status {
            new_status.push(status_at(dataset, flat));
        }
    }

    // Build new sizes
    let new_sizes: Vec<usize> = keep_indices.iter().map(|k| k.len()).collect();

    // Build new dimensions (filter categories)
    let mut new_dimensions = dimensions.clone();
    for (i, dim_id) in dim_ids.iter().enumerate() {
        if let Some(dim) = new_dimensions.get_mut(dim_id) {
            let kept_cat_ids: Vec<String> = keep_indices[i]
                .iter()
                .filter_map(|&pos| category_id_at(dim, pos))
                .collect();
            *dim = dim.filter_categories(&kept_cat_ids);
        }
    }

    // Build new dataset
    let mut new_dataset = dataset.clone();
    new_dataset.size = Some(new_sizes);
    new_dataset.dimension = Some(new_dimensions);
    new_dataset.value = Some(DatasetValue::Array(new_values));
    if has_status {
        new_dataset.status = Some(serde_json::Value::Array(new_status));
    }

    Ok(new_dataset)
}

// ── Dice Filter Parsing ───────────────────────────────────────────────────

fn parse_dice_filter(
    filter_js: JsValue,
) -> Result<HashMap<String, Vec<String>>, JsValue> {
    if filter_js.is_null() || filter_js.is_undefined() {
        return Ok(HashMap::new());
    }

    // Try as object {dim_id: [cat_ids]}
    if let Ok(map) = serde_wasm_bindgen::from_value::<HashMap<String, Vec<String>>>(filter_js.clone()) {
        return Ok(map);
    }

    // Try as array of [dim_id, [cat_ids]]
    if js_sys::Array::is_array(&filter_js) {
        let arr: Vec<Vec<serde_json::Value>> = serde_wasm_bindgen::from_value(filter_js)
            .map_err(|e| JsValue::from_str(&format!("Invalid filter format: {}", e)))?;
        let mut map = HashMap::new();
        for pair in arr {
            if pair.len() == 2 {
                if let (Some(serde_json::Value::String(dim_id)), Some(serde_json::Value::Array(cats))) =
                    (pair.first(), pair.get(1))
                {
                    let cat_ids: Vec<String> = cats
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                    map.insert(dim_id.clone(), cat_ids);
                }
            }
        }
        return Ok(map);
    }

    Err(JsValue::from_str("Invalid filter format"))
}

// ── Transform Implementations ─────────────────────────────────────────────

/// A precomputed output column for one dimension: its column name and the
/// cell value (id or label) for each category position.
struct DimColumn {
    dim_idx: usize,
    name: String,
    cells: Vec<serde_json::Value>,
}

/// Precompute column names and per-category cell values for the included
/// dimensions, avoiding repeated per-cell lookups in the transform loops.
fn precompute_columns(
    dim_ids: &[String],
    dimensions: &indexmap::IndexMap<String, Dimension>,
    included_dim_indices: &[usize],
    content: &str,
    field: &str,
) -> Vec<DimColumn> {
    included_dim_indices
        .iter()
        .map(|&dim_idx| {
            let dim_id = &dim_ids[dim_idx];
            let name = get_column_name(dim_id, field, dimensions);
            let cells = dimensions
                .get(dim_id)
                .map(|dim| {
                    category_ids_of(dim)
                        .iter()
                        .map(|cat_id| {
                            if content == "label" {
                                serde_json::Value::String(category_label_for(dim, cat_id))
                            } else {
                                serde_json::Value::String(cat_id.clone())
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            DimColumn { dim_idx, name, cells }
        })
        .collect()
}

fn column_cell(col: &DimColumn, cat_pos: usize) -> serde_json::Value {
    col.cells
        .get(cat_pos)
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

fn get_column_name(
    dim_id: &str,
    field: &str,
    dimensions: &indexmap::IndexMap<String, Dimension>,
) -> String {
    if field == "label" {
        dimensions
            .get(dim_id)
            .map(|d| dimension_label_or(d, dim_id))
            .unwrap_or_else(|| dim_id.to_string())
    } else {
        dim_id.to_string()
    }
}

/// Convert a [`Cell`] into a JSON value, optionally applying comma decimal
/// formatting (numbers become strings with `","` as the decimal mark).
fn format_cell(cell: &Cell, comma: bool) -> serde_json::Value {
    if comma {
        match cell {
            Cell::Number(n) => serde_json::Value::String(n.to_string().replace('.', ",")),
            _ => cell.to_json_value(),
        }
    } else {
        cell.to_json_value()
    }
}

#[allow(clippy::too_many_arguments)]
fn transform_arrobj(
    dataset: &Dataset,
    dim_ids: &[String],
    sizes: &[usize],
    dimensions: &indexmap::IndexMap<String, Dimension>,
    values: &DatasetValue,
    included_dim_indices: &[usize],
    include_status: bool,
    content: &str,
    field: &str,
    vlabel: &str,
    slabel: &str,
    comma: bool,
) -> Result<serde_json::Value, JsValue> {
    let columns = precompute_columns(dim_ids, dimensions, included_dim_indices, content, field);
    let val_key = get_value_key(field, vlabel);
    let status_key = get_status_key(field, slabel);

    let total: usize = sizes.iter().product();
    let mut result = Vec::with_capacity(total);
    for (flat, indices) in query::index_iter(sizes).enumerate() {
        let mut obj = serde_json::Map::new();
        for col in &columns {
            obj.insert(col.name.clone(), column_cell(col, indices[col.dim_idx]));
        }
        let val = values.get_at(flat);
        obj.insert(val_key.clone(), format_cell(&val, comma));
        if include_status {
            let st = status_at(dataset, flat);
            obj.insert(status_key.clone(), st);
        }
        result.push(serde_json::Value::Object(obj));
    }

    Ok(serde_json::Value::Array(result))
}

#[allow(clippy::too_many_arguments)]
fn transform_arrobj_by(
    _dataset: &Dataset,
    dim_ids: &[String],
    sizes: &[usize],
    dimensions: &indexmap::IndexMap<String, Dimension>,
    values: &DatasetValue,
    included_dim_indices: &[usize],
    by_dim_idx: usize,
    content: &str,
    field: &str,
    _vlabel: &str,
    comma: bool,
    prefix: &str,
) -> Result<serde_json::Value, JsValue> {
    // Get by-dimension categories and column names
    let by_dim_id = &dim_ids[by_dim_idx];
    let by_dim = dimensions.get(by_dim_id).ok_or_else(|| {
        JsValue::from_str(&format!("Dimension '{}' not found", by_dim_id))
    })?;
    let by_cat_ids = category_ids_of(by_dim);
    let by_col_names: Vec<String> = by_cat_ids
        .iter()
        .map(|id| {
            let raw = if content == "label" {
                category_label_for(by_dim, id)
            } else {
                id.clone()
            };
            format!("{}{}", prefix, raw)
        })
        .collect();

    // Non-by dimensions to include
    let non_by_dim_indices: Vec<usize> = included_dim_indices
        .iter()
        .filter(|&&idx| idx != by_dim_idx)
        .copied()
        .collect();
    let columns = precompute_columns(dim_ids, dimensions, &non_by_dim_indices, content, field);

    // Generate all index combos for non-by dimensions
    let non_by_sizes: Vec<usize> = non_by_dim_indices.iter().map(|&idx| sizes[idx]).collect();

    let mut result = Vec::new();

    for non_by_combo in query::index_iter(&non_by_sizes) {
        let mut obj = serde_json::Map::new();

        // Map non_by_combo back to full indices (placeholder for by-dim)
        let mut base_indices = vec![0usize; dim_ids.len()];
        for (combo_idx, &dim_idx) in non_by_dim_indices.iter().enumerate() {
            base_indices[dim_idx] = non_by_combo[combo_idx];
        }

        // Fill non-by dimension values
        for (col, &cat_pos) in columns.iter().zip(non_by_combo.iter()) {
            obj.insert(col.name.clone(), column_cell(col, cat_pos));
        }

        // Fill by-dimension categories as columns
        for (by_cat_pos, col_name) in by_col_names.iter().enumerate() {
            let mut full_indices = base_indices.clone();
            full_indices[by_dim_idx] = by_cat_pos;
            let flat = query::calculate_index(&full_indices, sizes).unwrap_or(0);
            let val = values.get_at(flat);
            obj.insert(col_name.clone(), format_cell(&val, comma));
        }

        result.push(serde_json::Value::Object(obj));
    }

    Ok(serde_json::Value::Array(result))
}

#[allow(clippy::too_many_arguments)]
fn transform_array(
    dataset: &Dataset,
    dim_ids: &[String],
    sizes: &[usize],
    dimensions: &indexmap::IndexMap<String, Dimension>,
    values: &DatasetValue,
    included_dim_indices: &[usize],
    include_status: bool,
    content: &str,
    field: &str,
    vlabel: &str,
    slabel: &str,
    comma: bool,
) -> Result<serde_json::Value, JsValue> {
    let columns = precompute_columns(dim_ids, dimensions, included_dim_indices, content, field);
    let val_key = get_value_key(field, vlabel);
    let status_key = get_status_key(field, slabel);

    let total: usize = sizes.iter().product();
    let mut result = Vec::with_capacity(total + 1);

    // Header row
    let mut header = Vec::new();
    for col in &columns {
        header.push(serde_json::Value::String(col.name.clone()));
    }
    header.push(serde_json::Value::String(val_key));
    if include_status {
        header.push(serde_json::Value::String(status_key));
    }
    result.push(serde_json::Value::Array(header));

    // Data rows
    for (flat, indices) in query::index_iter(sizes).enumerate() {
        let mut row = Vec::new();
        for col in &columns {
            row.push(column_cell(col, indices[col.dim_idx]));
        }
        let val = values.get_at(flat);
        row.push(format_cell(&val, comma));
        if include_status {
            let st = status_at(dataset, flat);
            row.push(st);
        }
        result.push(serde_json::Value::Array(row));
    }

    Ok(serde_json::Value::Array(result))
}

#[allow(clippy::too_many_arguments)]
fn transform_objarr(
    dataset: &Dataset,
    dim_ids: &[String],
    sizes: &[usize],
    dimensions: &indexmap::IndexMap<String, Dimension>,
    values: &DatasetValue,
    included_dim_indices: &[usize],
    include_status: bool,
    content: &str,
    field: &str,
    vlabel: &str,
    slabel: &str,
    comma: bool,
) -> Result<serde_json::Value, JsValue> {
    let columns = precompute_columns(dim_ids, dimensions, included_dim_indices, content, field);
    let total: usize = sizes.iter().product();
    let val_key = get_value_key(field, vlabel);
    let status_key = get_status_key(field, slabel);

    let mut obj = serde_json::Map::new();

    // Initialize columns
    for col in &columns {
        obj.insert(
            col.name.clone(),
            serde_json::Value::Array(Vec::with_capacity(total)),
        );
    }
    obj.insert(
        val_key.clone(),
        serde_json::Value::Array(Vec::with_capacity(total)),
    );
    if include_status {
        obj.insert(
            status_key.clone(),
            serde_json::Value::Array(Vec::with_capacity(total)),
        );
    }

    // Fill data
    for (flat, indices) in query::index_iter(sizes).enumerate() {
        for col in &columns {
            let cell_val = column_cell(col, indices[col.dim_idx]);
            if let Some(serde_json::Value::Array(arr)) = obj.get_mut(&col.name) {
                arr.push(cell_val);
            }
        }

        let val = values.get_at(flat);
        if let Some(serde_json::Value::Array(arr)) = obj.get_mut(&val_key) {
            arr.push(format_cell(&val, comma));
        }

        if include_status {
            let st = status_at(dataset, flat);
            if let Some(serde_json::Value::Array(arr)) = obj.get_mut(&status_key) {
                arr.push(st);
            }
        }
    }

    Ok(serde_json::Value::Object(obj))
}

#[allow(clippy::too_many_arguments)]
fn transform_objarr_by(
    _dataset: &Dataset,
    dim_ids: &[String],
    sizes: &[usize],
    dimensions: &indexmap::IndexMap<String, Dimension>,
    values: &DatasetValue,
    included_dim_indices: &[usize],
    by_dim_idx: usize,
    content: &str,
    field: &str,
    _vlabel: &str,
    comma: bool,
    prefix: &str,
) -> Result<serde_json::Value, JsValue> {
    let by_dim_id = &dim_ids[by_dim_idx];
    let by_dim = dimensions.get(by_dim_id).ok_or_else(|| {
        JsValue::from_str(&format!("Dimension '{}' not found", by_dim_id))
    })?;
    let by_cat_ids = category_ids_of(by_dim);
    let by_col_names: Vec<String> = by_cat_ids
        .iter()
        .map(|id| {
            let raw = if content == "label" {
                category_label_for(by_dim, id)
            } else {
                id.clone()
            };
            format!("{}{}", prefix, raw)
        })
        .collect();

    let non_by_dim_indices: Vec<usize> = included_dim_indices
        .iter()
        .filter(|&&idx| idx != by_dim_idx)
        .copied()
        .collect();
    let columns = precompute_columns(dim_ids, dimensions, &non_by_dim_indices, content, field);

    let non_by_sizes: Vec<usize> = non_by_dim_indices.iter().map(|&idx| sizes[idx]).collect();
    let total: usize = non_by_sizes.iter().product();

    let mut obj = serde_json::Map::new();

    // Initialize columns for non-by dims
    for col in &columns {
        obj.insert(
            col.name.clone(),
            serde_json::Value::Array(Vec::with_capacity(total)),
        );
    }

    // Initialize columns for by-dim categories
    for col_name in &by_col_names {
        obj.insert(
            col_name.clone(),
            serde_json::Value::Array(Vec::with_capacity(total)),
        );
    }

    // Fill data
    for non_by_combo in query::index_iter(&non_by_sizes) {
        let mut base_indices = vec![0usize; dim_ids.len()];
        for (combo_idx, &dim_idx) in non_by_dim_indices.iter().enumerate() {
            base_indices[dim_idx] = non_by_combo[combo_idx];
        }

        // Non-by dimension values
        for (col, &cat_pos) in columns.iter().zip(non_by_combo.iter()) {
            let cell_val = column_cell(col, cat_pos);
            if let Some(serde_json::Value::Array(arr)) = obj.get_mut(&col.name) {
                arr.push(cell_val);
            }
        }

        // By-dimension category values
        for (by_cat_pos, col_name) in by_col_names.iter().enumerate() {
            let mut full_indices = base_indices.clone();
            full_indices[by_dim_idx] = by_cat_pos;
            let flat = query::calculate_index(&full_indices, sizes).unwrap_or(0);
            let val = values.get_at(flat);
            if let Some(serde_json::Value::Array(arr)) = obj.get_mut(col_name) {
                arr.push(format_cell(&val, comma));
            }
        }
    }

    Ok(serde_json::Value::Object(obj))
}

/// Google DataTable "object" type. Columns are inferred from the first data
/// value: if it is a number (or null), the column type is "number"; otherwise
/// "string". Dimension/category columns are always typed as "string".
#[allow(clippy::too_many_arguments)]
fn transform_object(
    dataset: &Dataset,
    dim_ids: &[String],
    sizes: &[usize],
    dimensions: &indexmap::IndexMap<String, Dimension>,
    values: &DatasetValue,
    included_dim_indices: &[usize],
    include_status: bool,
    content: &str,
    field: &str,
    vlabel: &str,
    slabel: &str,
) -> Result<serde_json::Value, JsValue> {
    let columns = precompute_columns(dim_ids, dimensions, included_dim_indices, content, field);
    let val_key = get_value_key(field, vlabel);
    let status_key = get_status_key(field, slabel);

    // Naïve type inference from the first value (as per toolkit spec)
    let first_val = values.get_at(0);
    let value_type = if first_val.is_null() || first_val.as_f64().is_some() {
        "number"
    } else {
        "string"
    };

    // Build cols
    let mut cols = Vec::new();
    for col in &columns {
        cols.push(serde_json::json!({
            "id": get_dim_id(&col.name, field, dimensions, dim_ids, included_dim_indices),
            "label": col.name,
            "type": "string"
        }));
    }
    cols.push(serde_json::json!({
        "id": "value",
        "label": val_key,
        "type": value_type
    }));
    if include_status {
        cols.push(serde_json::json!({
            "id": "status",
            "label": status_key,
            "type": "string"
        }));
    }

    // Build rows
    let total: usize = sizes.iter().product();
    let mut rows = Vec::with_capacity(total);
    for (flat, indices) in query::index_iter(sizes).enumerate() {
        let mut c = Vec::new();
        for col in &columns {
            c.push(serde_json::json!({ "v": column_cell(col, indices[col.dim_idx]) }));
        }
        let val = values.get_at(flat);
        c.push(serde_json::json!({ "v": val.to_json_value() }));
        if include_status {
            let st = status_at(dataset, flat);
            c.push(serde_json::json!({ "v": st }));
        }
        rows.push(serde_json::json!({ "c": c }));
    }

    Ok(serde_json::json!({ "cols": cols, "rows": rows }))
}

/// Resolve the dimension ID for a column name (inverse of `get_column_name`).
fn get_dim_id(
    name: &str,
    field: &str,
    dimensions: &indexmap::IndexMap<String, Dimension>,
    dim_ids: &[String],
    included_dim_indices: &[usize],
) -> String {
    if field == "label" {
        for &idx in included_dim_indices {
            let dim_id = &dim_ids[idx];
            if let Some(dim) = dimensions.get(dim_id) {
                if dimension_label_or(dim, dim_id) == name {
                    return dim_id.clone();
                }
            }
        }
        name.to_string()
    } else {
        name.to_string()
    }
}

/// Build the "meta" object for Transform() when `meta: true`.
#[allow(clippy::too_many_arguments)]
fn build_meta(
    dataset: &Dataset,
    dim_ids: &[String],
    type_: &str,
    status: bool,
    by: Option<&str>,
    drop: &[String],
    prefix: &str,
    comma: bool,
) -> serde_json::Value {
    let label = dataset.label.clone().unwrap_or_default();
    let source = dataset.source.clone().unwrap_or_default();
    let updated = dataset.updated.clone().unwrap_or_default();
    let dimensions = dataset.dimension.as_ref();

    let mut dims_obj = serde_json::Map::new();
    if let Some(dims) = dimensions {
        for dim_id in dim_ids {
            if let Some(dim) = dims.get(dim_id) {
                let dim_label = dimension_label_or(dim, dim_id);
                let role = dataset
                    .role
                    .as_ref()
                    .and_then(|r| {
                        r.iter().find_map(|(role_name, ids)| {
                            if ids.iter().any(|id| id == dim_id) {
                                Some(role_name.clone())
                            } else {
                                None
                            }
                        })
                    })
                    .unwrap_or_else(|| "classification".to_string());

                let cat_ids = category_ids_of(dim);
                let cat_labels: Vec<String> = cat_ids
                    .iter()
                    .map(|id| category_label_for(dim, id))
                    .collect();

                dims_obj.insert(
                    dim_id.clone(),
                    serde_json::json!({
                        "label": dim_label,
                        "role": role,
                        "categories": {
                            "id": cat_ids,
                            "label": cat_labels
                        }
                    }),
                );
            }
        }
    }

    serde_json::json!({
        "type": type_,
        "label": label,
        "source": source,
        "updated": updated,
        "id": dim_ids,
        "status": status,
        "by": by.unwrap_or(""),
        "drop": drop,
        "prefix": prefix,
        "comma": comma,
        "dimensions": dims_obj
    })
}

fn get_value_key(field: &str, vlabel: &str) -> String {
    if field == "label" {
        vlabel.to_string()
    } else {
        "value".to_string()
    }
}

fn get_status_key(field: &str, slabel: &str) -> String {
    if field == "label" {
        slabel.to_string()
    } else {
        "status".to_string()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_dataset_json() -> &'static str {
        r#"{
            "version": "2.0",
            "class": "dataset",
            "label": "Population",
            "id": ["concept", "area", "year"],
            "size": [1, 2, 2],
            "dimension": {
                "concept": {
                    "label": "concept",
                    "category": {
                        "index": ["pop"],
                        "label": {"pop": "Population"}
                    }
                },
                "area": {
                    "label": "area",
                    "category": {
                        "index": ["US", "CA"],
                        "label": {"US": "United States", "CA": "Canada"}
                    }
                },
                "year": {
                    "label": "year",
                    "category": {
                        "index": ["2020", "2021"]
                    }
                }
            },
            "value": [331, 332, 38, 39]
        }"#
    }

    fn sample_collection_json() -> &'static str {
        r#"{
            "version": "2.0",
            "class": "collection",
            "label": "Test Collection",
            "link": {
                "item": [
                    {"class": "dataset", "href": "https://example.com/ds1", "label": "Dataset 1"},
                    {"class": "dataset", "href": "https://example.com/ds2", "label": "Dataset 2"}
                ]
            }
        }"#
    }

    fn sample_dimension_json() -> &'static str {
        r#"{
            "version": "2.0",
            "class": "dimension",
            "label": "Geography",
            "category": {
                "index": {"US": 0, "CA": 1, "MX": 2},
                "label": {"US": "United States", "CA": "Canada", "MX": "Mexico"}
            }
        }"#
    }

    #[test]
    fn test_parse_dataset() {
        let json = sample_dataset_json();
        let response: JsonStatResponse = serde_json::from_str(json).unwrap();
        assert!(matches!(response, JsonStatResponse::Dataset(_)));
        assert_eq!(response.class(), "dataset");
        assert_eq!(response.version(), "2.0");
    }

    #[test]
    fn test_parse_collection() {
        let json = sample_collection_json();
        let response: JsonStatResponse = serde_json::from_str(json).unwrap();
        assert!(matches!(response, JsonStatResponse::Collection(_)));
        assert_eq!(response.class(), "collection");
    }

    #[test]
    fn test_parse_dimension_response() {
        let json = sample_dimension_json();
        let response: JsonStatResponse = serde_json::from_str(json).unwrap();
        assert!(matches!(response, JsonStatResponse::Dimension(_)));
        assert_eq!(response.class(), "dimension");
    }

    #[test]
    fn test_class_discrimination() {
        // A collection should NOT be parsed as a dataset
        let json = sample_collection_json();
        let response: JsonStatResponse = serde_json::from_str(json).unwrap();
        match &response {
            JsonStatResponse::Dataset(_) => panic!("Collection parsed as Dataset!"),
            JsonStatResponse::Collection(c) => {
                assert_eq!(c.label.as_deref(), Some("Test Collection"));
            }
            _ => {}
        }
    }

    #[test]
    fn test_auto_fill_constant_dimension() {
        let json = sample_dataset_json();
        let response: JsonStatResponse = serde_json::from_str(json).unwrap();
        let dataset = match &response {
            JsonStatResponse::Dataset(d) => d,
            _ => panic!("Expected dataset"),
        };

        // Query without the constant dimension "concept" (size 1)
        let mut query = HashMap::new();
        query.insert("area".to_string(), "CA".to_string());
        query.insert("year".to_string(), "2021".to_string());

        let flat = resolve_flat_index(dataset, &query).unwrap();
        assert_eq!(flat, 3); // [0, 1, 1] in sizes [1, 2, 2]

        let values = dataset.value.as_ref().unwrap();
        assert_eq!(values.get_at(flat).as_f64(), Some(39.0));
    }

    #[test]
    fn test_auto_fill_rejects_missing_non_constant() {
        let json = sample_dataset_json();
        let response: JsonStatResponse = serde_json::from_str(json).unwrap();
        let dataset = match &response {
            JsonStatResponse::Dataset(d) => d,
            _ => panic!("Expected dataset"),
        };

        // Query missing a non-constant dimension "area" (size 2)
        let mut query = HashMap::new();
        query.insert("year".to_string(), "2021".to_string());

        let result = resolve_flat_index(dataset, &query);
        assert!(result.is_err());
    }

    #[test]
    fn test_sparse_map_values() {
        let json = r#"{
            "version": "2.0",
            "class": "dataset",
            "label": "Sparse",
            "id": ["x", "y"],
            "size": [2, 2],
            "dimension": {
                "x": {"category": {"index": ["a", "b"]}},
                "y": {"category": {"index": ["c", "d"]}}
            },
            "value": {"0": 10.0, "3": 40.0}
        }"#;
        let response: JsonStatResponse = serde_json::from_str(json).unwrap();
        let dataset = match &response {
            JsonStatResponse::Dataset(d) => d,
            _ => panic!("Expected dataset"),
        };
        let values = dataset.value.as_ref().unwrap();
        assert_eq!(values.get_at(0).as_f64(), Some(10.0));
        assert!(values.get_at(1).is_null());
        assert!(values.get_at(2).is_null());
        assert_eq!(values.get_at(3).as_f64(), Some(40.0));
    }

    #[test]
    fn test_category_index_map() {
        let json = r#"{
            "version": "2.0",
            "class": "dataset",
            "id": ["area"],
            "size": [3],
            "dimension": {
                "area": {
                    "category": {
                        "index": {"US": 0, "CA": 1, "MX": 2},
                        "label": {"US": "United States", "CA": "Canada", "MX": "Mexico"}
                    }
                }
            },
            "value": [1, 2, 3]
        }"#;
        let response: JsonStatResponse = serde_json::from_str(json).unwrap();
        let dataset = match &response {
            JsonStatResponse::Dataset(d) => d,
            _ => panic!("Expected dataset"),
        };
        let mut query = HashMap::new();
        query.insert("area".to_string(), "CA".to_string());
        let flat = resolve_flat_index(dataset, &query).unwrap();
        assert_eq!(flat, 1);

        let values = dataset.value.as_ref().unwrap();
        assert_eq!(values.get_at(flat).as_f64(), Some(2.0));
    }

    #[test]
    fn test_with_status() {
        let json = r#"{
            "version": "2.0",
            "class": "dataset",
            "id": ["x"],
            "size": [3],
            "dimension": {
                "x": {"category": {"index": ["a", "b", "c"]}}
            },
            "value": [1.0, 2.0, 3.0],
            "status": ["ok", "est", "ok"]
        }"#;
        let response: JsonStatResponse = serde_json::from_str(json).unwrap();
        let dataset = match &response {
            JsonStatResponse::Dataset(d) => d,
            _ => panic!("Expected dataset"),
        };
        assert_eq!(status_at(dataset, 0), serde_json::Value::String("ok".to_string()));
        assert_eq!(status_at(dataset, 1), serde_json::Value::String("est".to_string()));
        assert_eq!(status_at(dataset, 2), serde_json::Value::String("ok".to_string()));
    }

    #[test]
    fn test_string_values() {
        let json = r#"{
            "version": "2.0",
            "class": "dataset",
            "id": ["x"],
            "size": [3],
            "dimension": {
                "x": {"category": {"index": ["a", "b", "c"]}}
            },
            "value": [1.5, "confidential", null]
        }"#;
        let response: JsonStatResponse = serde_json::from_str(json).unwrap();
        let dataset = match &response {
            JsonStatResponse::Dataset(d) => d,
            _ => panic!("Expected dataset"),
        };
        let values = dataset.value.as_ref().unwrap();
        assert_eq!(values.get_at(0).as_f64(), Some(1.5));
        assert_eq!(values.get_at(1), Cell::String("confidential".to_string()));
        assert!(values.get_at(2).is_null());
    }

    #[test]
    fn test_status_string() {
        let json = r#"{
            "version": "2.0",
            "class": "dataset",
            "id": ["x"],
            "size": [2],
            "dimension": {
                "x": {"category": {"index": ["a", "b"]}}
            },
            "value": [1.0, 2.0],
            "status": "e"
        }"#;
        let response: JsonStatResponse = serde_json::from_str(json).unwrap();
        let dataset = match &response {
            JsonStatResponse::Dataset(d) => d,
            _ => panic!("Expected dataset"),
        };
        // A plain string status applies to all values
        assert_eq!(status_at(dataset, 0), serde_json::Value::String("e".to_string()));
        assert_eq!(status_at(dataset, 1), serde_json::Value::String("e".to_string()));
    }

    #[test]
    fn test_label_only_constant_dimension() {
        // Per the spec, 'index' is optional for single-category dimensions
        let json = r#"{
            "version": "2.0",
            "class": "dataset",
            "id": ["concept", "year"],
            "size": [1, 2],
            "dimension": {
                "concept": {
                    "category": {"label": {"pop": "Population"}}
                },
                "year": {
                    "category": {"index": ["2020", "2021"]}
                }
            },
            "value": [10, 20]
        }"#;
        let response: JsonStatResponse = serde_json::from_str(json).unwrap();
        let dataset = match &response {
            JsonStatResponse::Dataset(d) => d,
            _ => panic!("Expected dataset"),
        };

        // Auto-fill the constant dimension when missing from the query
        let mut query = HashMap::new();
        query.insert("year".to_string(), "2021".to_string());
        let flat = resolve_flat_index(dataset, &query).unwrap();
        assert_eq!(flat, 1);

        // Explicit query against the label-only category also works
        let mut query2 = HashMap::new();
        query2.insert("concept".to_string(), "pop".to_string());
        query2.insert("year".to_string(), "2020".to_string());
        assert_eq!(resolve_flat_index(dataset, &query2).unwrap(), 0);
    }

    fn dataset_from(json: &str) -> Dataset {
        match serde_json::from_str::<JsonStatResponse>(json).unwrap() {
            JsonStatResponse::Dataset(d) => d,
            _ => panic!("Expected dataset"),
        }
    }

    #[test]
    fn test_dice_preserves_original_order() {
        let dataset = dataset_from(sample_dataset_json());

        // Filter lists categories in reverse order; the result must keep the
        // dataset's original category order (US before CA) so values stay
        // aligned with the category index.
        let mut filter = HashMap::new();
        filter.insert("area".to_string(), vec!["CA".to_string(), "US".to_string()]);

        let diced = dice_dataset(&dataset, &filter, false).unwrap();
        let dims = diced.dimension.as_ref().unwrap();
        let area_ids = category_ids_of(dims.get("area").unwrap());
        assert_eq!(area_ids, vec!["US", "CA"]);

        let values = diced.value.as_ref().unwrap();
        // Original values [331, 332, 38, 39] must be unchanged
        assert_eq!(values.get_at(0).as_f64(), Some(331.0));
        assert_eq!(values.get_at(1).as_f64(), Some(332.0));
        assert_eq!(values.get_at(2).as_f64(), Some(38.0));
        assert_eq!(values.get_at(3).as_f64(), Some(39.0));
    }

    #[test]
    fn test_dice_keep_subset() {
        let dataset = dataset_from(sample_dataset_json());
        let mut filter = HashMap::new();
        filter.insert("area".to_string(), vec!["CA".to_string()]);

        let diced = dice_dataset(&dataset, &filter, false).unwrap();
        assert_eq!(diced.size.as_ref().unwrap(), &vec![1, 1, 2]);
        let values = diced.value.as_ref().unwrap();
        assert_eq!(values.get_at(0).as_f64(), Some(38.0));
        assert_eq!(values.get_at(1).as_f64(), Some(39.0));
    }

    #[test]
    fn test_dice_drop_mode() {
        let dataset = dataset_from(sample_dataset_json());
        let mut filter = HashMap::new();
        filter.insert("area".to_string(), vec!["US".to_string()]);

        let diced = dice_dataset(&dataset, &filter, true).unwrap();
        let dims = diced.dimension.as_ref().unwrap();
        let area_ids = category_ids_of(dims.get("area").unwrap());
        assert_eq!(area_ids, vec!["CA"]);
        let values = diced.value.as_ref().unwrap();
        assert_eq!(values.get_at(0).as_f64(), Some(38.0));
        assert_eq!(values.get_at(1).as_f64(), Some(39.0));
    }

    #[test]
    fn test_dice_unknown_ids_error() {
        let dataset = dataset_from(sample_dataset_json());

        // Unknown category
        let mut filter = HashMap::new();
        filter.insert("area".to_string(), vec!["XX".to_string()]);
        assert!(dice_dataset(&dataset, &filter, false).is_err());

        // Unknown dimension
        let mut filter2 = HashMap::new();
        filter2.insert("nope".to_string(), vec!["US".to_string()]);
        assert!(dice_dataset(&dataset, &filter2, false).is_err());
    }

    #[test]
    fn test_dice_filter() {
        let json = sample_dataset_json();
        let response: JsonStatResponse = serde_json::from_str(json).unwrap();
        let dataset = match &response {
            JsonStatResponse::Dataset(d) => d.clone(),
            _ => panic!("Expected dataset"),
        };

        // Simulate dice filtering: keep only area "CA"
        let dimensions = dataset.dimension.as_ref().unwrap();
        let dim = dimensions.get("area").unwrap();
        let filtered = dim.filter_categories(&["CA".to_string()]);

        let filtered_ids = filtered
            .category
            .as_ref()
            .and_then(|c| c.index.as_ref())
            .map(|idx| idx.ids())
            .unwrap_or_default();
        assert_eq!(filtered_ids, vec!["CA"]);
    }

    #[test]
    fn test_n_property() {
        let json = sample_dataset_json();
        let response: JsonStatResponse = serde_json::from_str(json).unwrap();
        let dataset = match &response {
            JsonStatResponse::Dataset(d) => d,
            _ => panic!("Expected dataset"),
        };
        // sizes: [1, 2, 2] → n = 4
        assert_eq!(dataset.size.as_ref().map(|s| s.iter().product::<usize>()), Some(4));
    }

    #[test]
    fn test_index_from_f64_valid() {
        assert_eq!(index_from_f64(0.0).unwrap(), 0);
        assert_eq!(index_from_f64(3.0).unwrap(), 3);
        assert_eq!(index_from_f64(1_000_000.0).unwrap(), 1_000_000);
    }

    #[test]
    fn test_index_from_f64_rejects_invalid() {
        // Negatives previously saturated to 0 → silent wrong result.
        assert!(index_from_f64(-1.0).is_err());
        assert!(index_from_f64(-100.0).is_err());
        // Fractions previously truncated silently (e.g. 2.7 → 2).
        assert!(index_from_f64(2.7).is_err());
        assert!(index_from_f64(99999.7).is_err());
        // Values beyond usize::MAX would overflow.
        assert!(index_from_f64(1e30).is_err());
        // Non-finite values.
        assert!(index_from_f64(f64::NAN).is_err());
        assert!(index_from_f64(f64::INFINITY).is_err());
        assert!(index_from_f64(f64::NEG_INFINITY).is_err());
    }

    #[test]
    fn test_filter_categories_filters_child() {
        use crate::models::{Category, CategoryIndex, Dimension};
        use std::collections::HashMap;

        // Dimension with 3 categories and a `child` relationship graph.
        let dim = Dimension {
            label: Some("geo".to_string()),
            category: Some(Category {
                index: Some(CategoryIndex::Array(vec![
                    "US".to_string(),
                    "CA".to_string(),
                    "MX".to_string(),
                ])),
                child: Some({
                    let mut m = HashMap::new();
                    // US → {CA, MX}; after filtering out MX, US → {CA} only.
                    m.insert("US".to_string(), vec!["CA".to_string(), "MX".to_string()]);
                    // MX → {US}; after filtering out MX entirely, this entry must be dropped.
                    m.insert("MX".to_string(), vec!["US".to_string()]);
                    m
                }),
                label: None,
                unit: None,
                coordinates: None,
                note: None,
            }),
            note: None,
            href: None,
            link: None,
            extension: None,
        };

        // Keep only US and CA.
        let filtered = dim.filter_categories(&["US".to_string(), "CA".to_string()]);
        let category = filtered.category.as_ref().unwrap();
        let child = category.child.as_ref().expect("child should remain");

        // US → {CA} (MX removed from the child list).
        assert_eq!(child.get("US"), Some(&vec!["CA".to_string()]));
        // MX → dropped entirely (key removed, no dangling reference).
        assert!(child.get("MX").is_none());
    }

    #[test]
    fn test_filter_categories_child_all_removed_drops_field() {
        use crate::models::{Category, CategoryIndex, Dimension};
        use std::collections::HashMap;

        // If every child relationship references only removed categories,
        // the `child` field must be dropped to None (no empty map lingering).
        let dim = Dimension {
            label: Some("geo".to_string()),
            category: Some(Category {
                index: Some(CategoryIndex::Array(vec![
                    "US".to_string(),
                    "CA".to_string(),
                ])),
                child: Some({
                    let mut m = HashMap::new();
                    // US → {CA}; keep only US → child list becomes empty.
                    m.insert("US".to_string(), vec!["CA".to_string()]);
                    m
                }),
                label: None,
                unit: None,
                coordinates: None,
                note: None,
            }),
            note: None,
            href: None,
            link: None,
            extension: None,
        };

        let filtered = dim.filter_categories(&["US".to_string()]);
        let category = filtered.category.as_ref().unwrap();
        assert!(
            category.child.is_none(),
            "child should be None when all child lists are emptied"
        );
    }

    #[test]
    fn test_dimension_map_preserves_insertion_order() {
        // Regression for report #16: to_json() must emit dimensions in the
        // order they appear in the source document. With IndexMap (replacing
        // HashMap), serialization order == insertion order == parse order.
        use crate::models::Dataset;
        use indexmap::IndexMap;

        let json = r#"{
            "version": "2.0",
            "class": "dataset",
            "id": ["zebra", "alpha", "mike"],
            "size": [1, 1, 1],
            "dimension": {
                "zebra": {"category": {"index": ["z"]}},
                "alpha": {"category": {"index": ["a"]}},
                "mike":   {"category": {"index": ["m"]}}
            },
            "value": [1.0]
        }"#;
        let response: JsonStatResponse = serde_json::from_str(json).unwrap();
        let dataset = match response {
            JsonStatResponse::Dataset(d) => d,
            _ => panic!("Expected dataset"),
        };

        let dimensions: &IndexMap<String, _> = dataset.dimension.as_ref().unwrap();
        let keys: Vec<&str> = dimensions.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec!["zebra", "alpha", "mike"],
            "dimension keys must preserve document order, not be alphabetized/shuffled"
        );

        // Round-trip: serialization must also preserve this order.
        let roundtrip = serde_json::to_string(
            &Dataset {
                dimension: Some(dimensions.clone()),
                ..serde_json::from_str::<Dataset>(json).unwrap()
            },
        )
        .unwrap();
        let zebra_pos = roundtrip.find("\"zebra\"").unwrap();
        let alpha_pos = roundtrip.find("\"alpha\"").unwrap();
        let mike_pos = roundtrip.find("\"mike\"").unwrap();
        assert!(zebra_pos < alpha_pos && alpha_pos < mike_pos,
            "serialization order must match insertion order");
    }

    // ── Data() partial queries ───────────────────────────────────────────

    #[test]
    fn test_resolve_query_full() {
        let dataset = dataset_from(sample_dataset_json());
        let mut query = HashMap::new();
        query.insert("concept".to_string(), "pop".to_string());
        query.insert("area".to_string(), "US".to_string());
        query.insert("year".to_string(), "2020".to_string());
        match resolve_query(&dataset, &query).unwrap() {
            QueryResolution::Single(idx) => assert_eq!(idx, 0),
            other => panic!("Expected Single, got {:?}", other),
        }
    }

    #[test]
    fn test_resolve_query_slice_one_free_dim() {
        // Leave "year" (size 2) unspecified → Slice with 2 categories.
        let dataset = dataset_from(sample_dataset_json());
        let mut query = HashMap::new();
        query.insert("area".to_string(), "US".to_string());
        match resolve_query(&dataset, &query).unwrap() {
            QueryResolution::Slice {
                free_cat_indices, ..
            } => assert_eq!(free_cat_indices, vec![0, 1]),
            other => panic!("Expected Slice, got {:?}", other),
        }
    }

    #[test]
    fn test_resolve_query_null_two_free_dims() {
        // Leave both "area" and "year" unspecified → Null.
        let dataset = dataset_from(sample_dataset_json());
        let query: HashMap<String, String> = HashMap::new();
        match resolve_query(&dataset, &query).unwrap() {
            QueryResolution::Null => {}
            other => panic!("Expected Null, got {:?}", other),
        }
    }

    #[test]
    fn test_resolve_query_ignores_unknown_dim_keys() {
        // "nonexistent" is not a real dimension; must be ignored.
        let dataset = dataset_from(sample_dataset_json());
        let mut query = HashMap::new();
        query.insert("area".to_string(), "US".to_string());
        query.insert("year".to_string(), "2020".to_string());
        query.insert("nonexistent".to_string(), "foo".to_string());
        match resolve_query(&dataset, &query).unwrap() {
            QueryResolution::Single(idx) => assert_eq!(idx, 0),
            other => panic!("Expected Single, got {:?}", other),
        }
    }

    #[test]
    fn test_resolve_query_unknown_cat_makes_dim_free() {
        // "ZZ" is not a valid category for "area" → that dim goes free.
        let dataset = dataset_from(sample_dataset_json());
        let mut query = HashMap::new();
        query.insert("area".to_string(), "ZZ".to_string());
        query.insert("year".to_string(), "2020".to_string());
        match resolve_query(&dataset, &query).unwrap() {
            QueryResolution::Slice { free_cat_indices, .. } => {
                assert_eq!(free_cat_indices, vec![0, 1])
            }
            other => panic!("Expected Slice, got {:?}", other),
        }
    }

    #[test]
    fn test_resolve_query_constant_dim_autofilled() {
        // Omit the constant dimension "concept" entirely.
        let dataset = dataset_from(sample_dataset_json());
        let mut query = HashMap::new();
        query.insert("area".to_string(), "CA".to_string());
        query.insert("year".to_string(), "2021".to_string());
        match resolve_query(&dataset, &query).unwrap() {
            QueryResolution::Single(idx) => assert_eq!(idx, 3), // [0,1,1]
            other => panic!("Expected Single, got {:?}", other),
        }
    }

    // ── Dimension() instance=false (label array) ─────────────────────────

    #[test]
    fn test_category_labels_of() {
        let dataset = dataset_from(sample_dataset_json());
        let dims = dataset.dimension.as_ref().unwrap();
        let area = dims.get("area").unwrap();
        let labels = category_labels_of(area);
        assert_eq!(labels, vec!["United States", "Canada"]);
    }

    #[test]
    fn test_category_labels_of_falls_back_to_ids() {
        // No "label" property → falls back to category IDs.
        let json = r#"{
            "version": "2.0", "class": "dataset",
            "id": ["x"], "size": [2],
            "dimension": {"x": {"category": {"index": ["a", "b"]}}},
            "value": [1, 2]
        }"#;
        let dataset = dataset_from(json);
        let dims = dataset.dimension.as_ref().unwrap();
        let x = dims.get("x").unwrap();
        assert_eq!(category_labels_of(x), vec!["a", "b"]);
    }

    // ── Transform() object type + meta/prefix/comma ──────────────────────

    fn transform_dataset() -> Dataset {
        dataset_from(sample_dataset_json())
    }

    #[test]
    fn test_transform_object_number_type() {
        let d = transform_dataset();
        let dims = d.dimension.as_ref().unwrap();
        let sizes = d.size.as_ref().unwrap();
        let dim_ids = d.id.as_ref().unwrap();
        let values = d.value.as_ref().unwrap();
        let included: Vec<usize> = (0..dim_ids.len()).collect();

        let result = transform_object(
            &d, dim_ids, sizes, dims, values, &included, false, "label", "id",
            "Value", "Status",
        )
        .unwrap();

        let cols = result.get("cols").unwrap().as_array().unwrap();
        // First value (1.0) is a number → value column is "number".
        let val_col = cols.last().unwrap();
        assert_eq!(val_col.get("type").unwrap(), "number");
        let rows = result.get("rows").unwrap().as_array().unwrap();
        assert_eq!(rows.len(), 4); // 1*2*2
    }

    #[test]
    fn test_transform_object_string_type() {
        let json = r#"{
            "version": "2.0", "class": "dataset",
            "id": ["x"], "size": [2],
            "dimension": {"x": {"category": {"index": ["a", "b"]}}},
            "value": ["foo", "bar"]
        }"#;
        let d = dataset_from(json);
        let dims = d.dimension.as_ref().unwrap();
        let sizes = d.size.as_ref().unwrap();
        let dim_ids = d.id.as_ref().unwrap();
        let values = d.value.as_ref().unwrap();
        let included: Vec<usize> = (0..dim_ids.len()).collect();

        let result = transform_object(
            &d, dim_ids, sizes, dims, values, &included, false, "label", "id",
            "Value", "Status",
        )
        .unwrap();

        let cols = result.get("cols").unwrap().as_array().unwrap();
        // First value "foo" is a string → value column is "string".
        assert_eq!(cols.last().unwrap().get("type").unwrap(), "string");
    }

    #[test]
    fn test_format_cell_comma() {
        let n = Cell::Number(serde_json::Number::from_f64(1.5).unwrap());
        assert_eq!(format_cell(&n, false), serde_json::json!(1.5));
        assert_eq!(format_cell(&n, true), serde_json::json!("1,5"));
        // Non-numbers are unaffected by comma.
        assert_eq!(format_cell(&Cell::Null, true), serde_json::Value::Null);
        assert_eq!(
            format_cell(&Cell::String("x".to_string()), true),
            serde_json::json!("x")
        );
    }

    #[test]
    fn test_transform_arrobj_comma() {
        let d = transform_dataset();
        let dims = d.dimension.as_ref().unwrap();
        let sizes = d.size.as_ref().unwrap();
        let dim_ids = d.id.as_ref().unwrap();
        let values = d.value.as_ref().unwrap();
        let included: Vec<usize> = (0..dim_ids.len()).collect();

        let result = transform_arrobj(
            &d, dim_ids, sizes, dims, values, &included, false, "label", "id",
            "Value", "Status", true,
        )
        .unwrap();
        let first = result.as_array().unwrap().first().unwrap();
        // comma:true turns the numeric value into a comma-decimal string.
        let val = first.get("value").unwrap();
        assert!(val.is_string(), "value must be a string when comma is true");
    }

    #[test]
    fn test_transform_arrobj_by_prefix() {
        let d = transform_dataset();
        let dims = d.dimension.as_ref().unwrap();
        let sizes = d.size.as_ref().unwrap();
        let dim_ids = d.id.as_ref().unwrap();
        let values = d.value.as_ref().unwrap();
        let included: Vec<usize> = (0..dim_ids.len()).collect();

        let result = transform_arrobj_by(
            &d, dim_ids, sizes, dims, values, &included, 1, "id", "id", "", false,
            "y_",
        )
        .unwrap();
        let first = result.as_array().unwrap().first().unwrap().as_object().unwrap();
        // The by-dimension (area, index 1) categories get prefixed column names.
        let has_prefixed = first.keys().any(|k| k.starts_with("y_"));
        assert!(has_prefixed, "expected prefixed column names");
    }

    #[test]
    fn test_build_meta_structure() {
        let d = transform_dataset();
        let dim_ids = d.id.as_ref().unwrap();
        let meta = build_meta(&d, dim_ids, "arrobj", false, Some("area"), &["year".to_string()], "", false);
        assert_eq!(meta.get("type").unwrap(), "arrobj");
        assert_eq!(meta.get("status").unwrap(), false);
        assert_eq!(meta.get("by").unwrap(), "area");
        assert_eq!(meta.get("drop").unwrap(), &serde_json::json!(["year"]));
        assert_eq!(meta.get("comma").unwrap(), false);
        let dims = meta.get("dimensions").unwrap().as_object().unwrap();
        assert!(dims.contains_key("area"));
        assert!(dims.contains_key("concept"));
        assert!(dims.contains_key("year"));
    }
}
