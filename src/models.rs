use std::collections::HashMap;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

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

impl JsonStatResponse {
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

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum DatasetValue {
    Array(Vec<Cell>),
    Map(HashMap<String, Cell>),
}

impl DatasetValue {
    pub fn get_at(&self, index: usize) -> Cell {
        match self {
            Self::Array(arr) => arr.get(index).cloned().unwrap_or(Cell::Null),
            Self::Map(map) => map.get(&index.to_string()).cloned().unwrap_or(Cell::Null),
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
                    CategoryIndex::Array(arr) => {
                        CategoryIndex::Array(
                            arr.iter()
                                .filter(|id| kept_cat_ids.contains(id))
                                .cloned()
                                .collect(),
                        )
                    }
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
