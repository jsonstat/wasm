use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

// ── Null-tolerant role deserializer ───────────────────────────────────────
//
// jsonstat-toolkit normalizes `role` and may emit values of `null`
// (e.g. `role.classification = null`). serde-wasm-bindgen refuses to
// coerce `null` into `Vec<String>` and throws `Reflect.get called on
// non-object`. We treat any `null` value as an empty `Vec<String>` so
// the object fast path never trips on it.
fn deserialize_role<'de, D>(
    deserializer: D,
) -> Result<Option<HashMap<String, Vec<String>>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NullOrVec {
        Null,
        Vec(Vec<String>),
    }

    let opt: Option<HashMap<String, NullOrVec>> = Option::deserialize(deserializer)?;
    Ok(opt.map(|map| {
        map.into_iter()
            .map(|(k, v)| match v {
                NullOrVec::Vec(items) => (k, items),
                NullOrVec::Null => (k, Vec::new()),
            })
            .collect()
    }))
}

// ── JsonStatResponse with class-based discrimination ──────────────────────

#[derive(Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum JsonStatResponse {
    Dataset(Dataset),
    Dimension(DimensionResponse),
    Collection(Collection),
}

impl<'de> serde::Deserialize<'de> for JsonStatResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let class = value
            .get("class")
            .and_then(|v| v.as_str())
            .unwrap_or("dataset");

        match class {
            "dataset" => serde_json::from_value::<Dataset>(value)
                .map(JsonStatResponse::Dataset)
                .map_err(serde::de::Error::custom),
            "dimension" => serde_json::from_value::<DimensionResponse>(value)
                .map(JsonStatResponse::Dimension)
                .map_err(serde::de::Error::custom),
            "collection" => serde_json::from_value::<Collection>(value)
                .map(JsonStatResponse::Collection)
                .map_err(serde::de::Error::custom),
            other => Err(serde::de::Error::custom(format!(
                "Unknown class: '{}'",
                other
            ))),
        }
    }
}

/// Sentinel that brackets the detected class in [`ClassFinder`]'s signalling
/// error. Uses control characters that never appear in a JSON-stat `class`
/// value (`"dataset"`/`"dimension"`/`"collection"`), so extraction is
/// unambiguous even if a deserializer appends extra text to the message.
const CLASS_SENTINEL: &str = "\u{1}jsonstat_class\u{1}";

/// Visitor that reads only the top-level `class` field and **stops early**.
///
/// A `#[derive(Deserialize)]` probe struct must visit every top-level entry —
/// skipping unknown fields still lexes the entire document, including the
/// potentially huge `value` array. To truly short-circuit, this visitor signals
/// success by *returning an error* the moment it sees `"class"`: serde_json
/// then stops parsing immediately and never lexes the trailing fields (the
/// common layout has `class` before `value`). `from_json_str` decodes that
/// sentinel error back into the class string.
///
/// Keys are read as `Cow<str>` so simple keys borrow with zero allocation and
/// escaped keys still deserialize correctly. Non-`class` values are skipped via
/// `IgnoredAny` (structural skip, no typed materialization).
struct ClassFinder;

impl<'de> serde::de::Visitor<'de> for ClassFinder {
    /// Returned only when no `class` key exists; the found case exits via error.
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON-stat object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        use serde::de::Error as _;
        while let Some(key) = map.next_key::<Cow<str>>()? {
            if key == "class" {
                let class: String = map.next_value()?;
                return Err(A::Error::custom(format!(
                    "{CLASS_SENTINEL}{class}{CLASS_SENTINEL}"
                )));
            }
            map.next_value::<serde::de::IgnoredAny>()?;
        }
        Ok(())
    }
}

impl JsonStatResponse {
    /// Parse a JSON-stat document directly into the typed model.
    ///
    /// This avoids the double parse of the generic `Deserialize` impl (which
    /// first builds a full `serde_json::Value` tree, then re-walks it with
    /// `from_value`). Here we do one cheap, early-exiting structural scan to
    /// read `class`, then a single real deserialization straight into the
    /// concrete type.
    pub fn from_json_str(json_str: &str) -> Result<Self, serde_json::Error> {
        // Cheap class probe: short-circuits as soon as `class` is found via a
        // sentinel error. Missing `class`, a non-object, or malformed JSON all
        // fall back to "dataset"; the real parse below surfaces the precise
        // error in those cases.
        let class = {
            use serde::Deserializer as _;
            let mut de = serde_json::Deserializer::from_str(json_str);
            match de.deserialize_map(ClassFinder) {
                Ok(()) => "dataset".to_string(),
                Err(e) => e
                    .to_string()
                    .split(CLASS_SENTINEL)
                    .nth(1)
                    .map(str::to_string)
                    .unwrap_or_else(|| "dataset".to_string()),
            }
        };

        match class.as_str() {
            "dataset" => serde_json::from_str::<Dataset>(json_str).map(JsonStatResponse::Dataset),
            "dimension" => {
                serde_json::from_str::<DimensionResponse>(json_str).map(JsonStatResponse::Dimension)
            }
            "collection" => {
                serde_json::from_str::<Collection>(json_str).map(JsonStatResponse::Collection)
            }
            other => Err(serde::de::Error::custom(format!(
                "Unknown class: '{}'",
                other
            ))),
        }
    }

    pub fn version(&self) -> &str {
        match self {
            Self::Dataset(d) => &d.version,
            Self::Dimension(d) => &d.version,
            Self::Collection(c) => &c.version,
        }
    }

    pub fn class(&self) -> &str {
        match self {
            Self::Dataset(d) => d.class.as_deref().unwrap_or("dataset"),
            Self::Dimension(d) => d.class.as_deref().unwrap_or("dimension"),
            Self::Collection(c) => c.class.as_deref().unwrap_or("collection"),
        }
    }
}

// ── Dataset ───────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Dataset {
    pub version: String,
    pub class: Option<String>,
    pub label: Option<String>,
    pub source: Option<String>,
    pub updated: Option<String>,
    pub href: Option<String>,
    pub id: Option<Vec<String>>,
    pub size: Option<Vec<usize>>,
    pub dimension: Option<IndexMap<String, Dimension>>,
    pub value: Option<DatasetValue>,
    pub status: Option<serde_json::Value>,
    #[serde(deserialize_with = "deserialize_role", default)]
    pub role: Option<HashMap<String, Vec<String>>>,
    pub note: Option<Vec<String>>,
    pub link: Option<HashMap<String, Vec<LinkItem>>>,
    pub extension: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
}

// ── Cell ──────────────────────────────────────────────────────────────────

/// A single observation value. JSON-stat 2.0 allows values to be numbers,
/// strings or null.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum Cell {
    Number(serde_json::Number),
    String(String),
    Null,
}

impl Cell {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(n) => n.as_f64(),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn to_json_value(&self) -> serde_json::Value {
        match self {
            Self::Number(n) => serde_json::Value::Number(n.clone()),
            Self::String(s) => serde_json::Value::String(s.clone()),
            Self::Null => serde_json::Value::Null,
        }
    }
}

// ── DatasetValue ──────────────────────────────────────────────────────────

/// The `value` payload of a JSON-stat dataset.
///
/// Three storage variants, picked **at parse time** to minimize per-cell cost
/// on the hottest paths (parse, [`value`](crate::JSONstat::value), `Data()`,
/// `Transform()`):
///
/// - [`DatasetValue::Numbers`] — **every** value is numeric. Stored as a flat
///   `Vec<f64>`, the form that lets the `value` getter emit a single
///   contiguous `Float64Array` (and lets `Transform` emit a numeric column
///   with zero per-cell boxing). This is the overwhelmingly common case for
///   statistical datasets. Integer-valued entries (e.g. `331`) are stored as
///   their `f64`; the custom [`Serialize`] writes whole numbers back without a
///   trailing `.0` to preserve the original document's formatting.
/// - [`DatasetValue::Cells`] — a dense array mixing numbers, strings, and
///   nulls (e.g. `[1.5, "confidential", null]`). Falls back to the
///   enum-tagged [`Cell`] representation so non-numeric values survive.
/// - [`DatasetValue::Sparse`] — a JSON-stat 2.0 sparse object keyed by the
///   decimal flat index (`{"0": 10, "3": 40}`). Missing keys read as `null`.
///
/// # Precision note
/// Integer values larger than 2⁵³ (~9 quadrillion) lose precision when stored
/// as `f64` on the `Numbers` path, exactly as they would in any JavaScript
/// engine (where every number is a double). Datasets in this library's domain
/// (statistical observations) never reach that range; mixed integer/float
/// arrays are unaffected. This matches the `jsonstat-toolkit` behavior.
#[derive(Debug, Clone)]
pub enum DatasetValue {
    /// All-numeric dense array — the fast path.
    Numbers(Vec<f64>),
    /// Dense array mixing numbers/strings/nulls.
    Cells(Vec<Cell>),
    /// Sparse object keyed by decimal flat index (JSON-stat 2.0 form).
    Sparse(HashMap<String, Cell>),
}

impl DatasetValue {
    /// Returns the cell at `index` (row-major flat), or [`Cell::Null`] when out
    /// of range or (for sparse) absent.
    ///
    /// On the `Numbers` path this boxes the `f64` back into a [`Cell`]; prefer
    /// [`DatasetValue::get_f64`] in hot loops that only need the numeric value.
    pub fn get_at(&self, index: usize) -> Cell {
        match self {
            Self::Numbers(nums) => nums
                .get(index)
                .map(|&n| {
                    Cell::Number(
                        serde_json::Number::from_f64(n)
                            .unwrap_or_else(|| serde_json::Number::from(0)),
                    )
                })
                .unwrap_or(Cell::Null),
            Self::Cells(arr) => arr.get(index).cloned().unwrap_or(Cell::Null),
            Self::Sparse(map) => {
                // Allocation-free decimal key lookup (sparse value object).
                let mut buf = [0u8; 20];
                map.get(crate::query::usize_key(&mut buf, index))
                    .cloned()
                    .unwrap_or(Cell::Null)
            }
        }
    }

    /// Zero-clone numeric access for the `Numbers` fast path. Returns `Some(f64)`
    /// for a stored number, `None` for an absent (sparse) or non-numeric value.
    /// Avoids boxing a [`Cell`] on every cell read in the hot `Transform` /
    /// `Data()` loops.
    #[inline]
    pub fn get_f64(&self, index: usize) -> Option<f64> {
        match self {
            Self::Numbers(nums) => nums.get(index).copied(),
            Self::Cells(arr) => arr.get(index).and_then(|c| c.as_f64()),
            Self::Sparse(map) => {
                let mut buf = [0u8; 20];
                map.get(crate::query::usize_key(&mut buf, index))
                    .and_then(|c| c.as_f64())
            }
        }
    }

    /// Returns a borrow over the `Numbers` slice when this is the all-numeric
    /// fast path, else `None`. Lets `value`/`Transform` emit a `Float64Array`
    /// / numeric column with a single bulk copy and no per-cell iteration.
    #[inline]
    pub fn as_numbers(&self) -> Option<&[f64]> {
        match self {
            Self::Numbers(nums) => Some(nums.as_slice()),
            _ => None,
        }
    }

    /// Number of stored entries (dense length, or sparse key count).
    pub fn len(&self) -> usize {
        match self {
            Self::Numbers(nums) => nums.len(),
            Self::Cells(arr) => arr.len(),
            Self::Sparse(map) => map.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `true` when the storage is the all-numeric fast path.
    pub fn is_numbers(&self) -> bool {
        matches!(self, Self::Numbers(_))
    }
}

impl<'de> serde::Deserialize<'de> for DatasetValue {
    /// Single-pass deserialization that picks the storage variant at parse time:
    /// an all-numeric JSON array becomes [`DatasetValue::Numbers`] (no
    /// per-element `Cell` boxing), a mixed array becomes
    /// [`DatasetValue::Cells`], and a JSON-stat 2.0 sparse object becomes
    /// [`DatasetValue::Sparse`].
    ///
    /// Replaces the previous `#[serde(untagged)]` derive, which walked the
    /// value twice (once into a `serde_json::Value` probe, once into the chosen
    /// variant). The custom visitor walks once.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(DatasetValueVisitor)
    }
}

struct DatasetValueVisitor;

impl<'de> serde::de::Visitor<'de> for DatasetValueVisitor {
    type Value = DatasetValue;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON-stat value array or sparse value object")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        // Collect as Cell first (single walk), then promote to Numbers if every
        // cell is numeric. The promotion is a cheap conversion with no re-parse
        // — far cheaper than serde's untagged double-walk.
        let mut cells: Vec<Cell> = Vec::new();
        while let Some(cell) = seq.next_element::<Cell>()? {
            cells.push(cell);
        }
        if cells.iter().all(|c| matches!(c, Cell::Number(_))) {
            let nums: Vec<f64> = cells
                .iter()
                .map(|c| match c {
                    Cell::Number(n) => n.as_f64().unwrap_or(f64::NAN),
                    _ => unreachable!("guarded by the `all` check above"),
                })
                .collect();
            Ok(DatasetValue::Numbers(nums))
        } else {
            Ok(DatasetValue::Cells(cells))
        }
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut out: HashMap<String, Cell> = HashMap::new();
        while let Some((k, v)) = map.next_entry::<String, Cell>()? {
            out.insert(k, v);
        }
        Ok(DatasetValue::Sparse(out))
    }
}

impl serde::Serialize for DatasetValue {
    /// Serializes every variant back to its JSON-stat wire form:
    /// - [`DatasetValue::Numbers`] → a JSON **array** of numbers. Whole-valued
    ///   `f64`s (e.g. `331.0`) are emitted as integers (`331`) to preserve the
    ///   original document's formatting; `serde_json` would otherwise write
    ///   `331.0`.
    /// - [`DatasetValue::Cells`] → a JSON array (cells serialize to number/
    ///   string/null).
    /// - [`DatasetValue::Sparse`] → a JSON object keyed by decimal index
    ///   (preserving the JSON-stat 2.0 sparse form).
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        match self {
            DatasetValue::Numbers(nums) => {
                let mut seq = serializer.serialize_seq(Some(nums.len()))?;
                for &f in nums {
                    if f.is_finite()
                        && f.fract() == 0.0
                        && f >= i64::MIN as f64
                        && f <= i64::MAX as f64
                    {
                        seq.serialize_element(&(f as i64))?;
                    } else {
                        seq.serialize_element(&f)?;
                    }
                }
                seq.end()
            }
            DatasetValue::Cells(cells) => cells.serialize(serializer),
            DatasetValue::Sparse(map) => map.serialize(serializer),
        }
    }
}

// ── DimensionResponse ─────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DimensionResponse {
    pub version: String,
    pub class: Option<String>,
    pub label: Option<String>,
    pub href: Option<String>,
    pub category: Option<Category>,
    pub note: Option<Vec<String>>,
    pub link: Option<HashMap<String, Vec<LinkItem>>>,
    pub extension: Option<serde_json::Value>,
}

// ── Dimension ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Dimension {
    pub label: Option<String>,
    pub category: Option<Category>,
    pub href: Option<String>,
    pub note: Option<Vec<String>>,
    pub link: Option<HashMap<String, Vec<LinkItem>>>,
    pub extension: Option<serde_json::Value>,
}

impl Dimension {
    /// Returns a new Dimension with only the specified category IDs kept.
    /// Category indices are re-numbered starting from 0.
    pub fn filter_categories(&self, kept_cat_ids: &[String]) -> Dimension {
        let mut new_dim = self.clone();
        if let Some(ref mut category) = new_dim.category {
            // Filter index
            if let Some(ref index) = category.index {
                category.index = Some(match index {
                    CategoryIndex::Array(arr) => CategoryIndex::Array(
                        arr.iter()
                            .filter(|id| kept_cat_ids.contains(id))
                            .cloned()
                            .collect(),
                    ),
                    CategoryIndex::Map(map) => {
                        let mut kept: Vec<(&String, &usize)> = map
                            .iter()
                            .filter(|(k, _)| kept_cat_ids.contains(k))
                            .collect();
                        kept.sort_by_key(|(_, &v)| v);
                        let new_map: HashMap<String, usize> = kept
                            .into_iter()
                            .enumerate()
                            .map(|(new_idx, (k, _))| (k.clone(), new_idx))
                            .collect();
                        CategoryIndex::Map(new_map)
                    }
                });
            }

            // Filter label
            if let Some(ref labels) = category.label {
                let new_labels: HashMap<String, String> = labels
                    .iter()
                    .filter(|(k, _)| kept_cat_ids.contains(k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                category.label = if new_labels.is_empty() {
                    None
                } else {
                    Some(new_labels)
                };
            }

            // Filter unit
            if let Some(ref units) = category.unit {
                let new_units: HashMap<String, serde_json::Value> = units
                    .iter()
                    .filter(|(k, _)| kept_cat_ids.contains(k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                category.unit = if new_units.is_empty() {
                    None
                } else {
                    Some(new_units)
                };
            }

            // Filter coordinates
            if let Some(ref coords) = category.coordinates {
                let new_coords: HashMap<String, Vec<f64>> = coords
                    .iter()
                    .filter(|(k, _)| kept_cat_ids.contains(k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                category.coordinates = if new_coords.is_empty() {
                    None
                } else {
                    Some(new_coords)
                };
            }

            // Filter note
            if let Some(ref notes) = category.note {
                let new_notes: HashMap<String, serde_json::Value> = notes
                    .iter()
                    .filter(|(k, _)| kept_cat_ids.contains(k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                category.note = if new_notes.is_empty() {
                    None
                } else {
                    Some(new_notes)
                };
            }

            // Filter child
            if let Some(ref children) = category.child {
                let new_children: HashMap<String, Vec<String>> = children
                    .iter()
                    // Drop entries whose parent category isn't kept (avoids
                    // dangling references to removed categories as keys).
                    .filter(|(k, _)| kept_cat_ids.contains(*k))
                    .map(|(k, child_ids)| {
                        let kept: Vec<String> = child_ids
                            .iter()
                            // Drop child IDs that aren't kept (avoids dangling
                            // references to removed categories as values).
                            .filter(|cid| kept_cat_ids.contains(cid))
                            .cloned()
                            .collect();
                        (k.clone(), kept)
                    })
                    // Drop entries whose child list is now empty.
                    .filter(|(_, kept)| !kept.is_empty())
                    .collect();
                category.child = if new_children.is_empty() {
                    None
                } else {
                    Some(new_children)
                };
            }
        }
        new_dim
    }
}

// ── Category ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Category {
    pub index: Option<CategoryIndex>,
    pub label: Option<HashMap<String, String>>,
    pub child: Option<HashMap<String, Vec<String>>>,
    pub unit: Option<HashMap<String, serde_json::Value>>,
    pub coordinates: Option<HashMap<String, Vec<f64>>>,
    pub note: Option<HashMap<String, serde_json::Value>>,
}

// ── CategoryIndex ─────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum CategoryIndex {
    Array(Vec<String>),
    Map(HashMap<String, usize>),
}

impl CategoryIndex {
    pub fn len(&self) -> usize {
        match self {
            Self::Array(arr) => arr.len(),
            Self::Map(map) => map.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get_id_at(&self, index: usize) -> Option<String> {
        match self {
            Self::Array(arr) => arr.get(index).cloned(),
            Self::Map(map) => map
                .iter()
                .find(|(_, &v)| v == index)
                .map(|(k, _)| k.clone()),
        }
    }

    pub fn get_index_of(&self, id: &str) -> Option<usize> {
        match self {
            Self::Array(arr) => arr.iter().position(|x| x == id),
            Self::Map(map) => map.get(id).copied(),
        }
    }

    pub fn ids(&self) -> Vec<String> {
        match self {
            Self::Array(arr) => arr.clone(),
            Self::Map(map) => {
                let mut entries: Vec<_> = map.iter().collect();
                entries.sort_by_key(|(_, &v)| v);
                entries.into_iter().map(|(k, _)| k.clone()).collect()
            }
        }
    }
}

// ── Collection ────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Collection {
    pub version: String,
    pub class: Option<String>,
    pub label: Option<String>,
    pub href: Option<String>,
    pub updated: Option<String>,
    pub source: Option<String>,
    pub link: Option<HashMap<String, Vec<CollectionItem>>>,
    pub note: Option<Vec<String>>,
    pub extension: Option<serde_json::Value>,
}

// ── CollectionItem ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CollectionItem {
    pub class: Option<String>,
    pub href: Option<String>,
    pub label: Option<String>,
    pub extension: Option<serde_json::Value>,
}

// ── LinkItem ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LinkItem {
    pub href: Option<String>,
    #[serde(rename = "type")]
    pub link_type: Option<String>,
    pub label: Option<String>,
    pub class: Option<String>,
    pub extension: Option<serde_json::Value>,
}
