// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! A scene, written down page by page.
//!
//! CRPECS stores entities in pages — one signature, rows, a column per
//! component — and the page is its unit of iteration, compaction and
//! serialization. A scene record keeps that shape: each page record is a
//! signature, the entities of its rows, and one column of values per
//! component. Capturing reads a page's columns; loading builds a page's rows
//! in one go. What a page record holds is the WHAT — component names and
//! values by name — never the memory a column happens to occupy.

use khora_core::ecs::PersistentId;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::record::Record;

/// Everything a scene, a prefab or a save holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SceneRecord {
    /// Every entity, in the order they were captured — including any that
    /// has no component worth saving, which still exists.
    pub entities: Vec<PersistentId>,
    /// The pages their components are stored in.
    pub pages: Vec<PageRecord>,
}

/// One page: a signature, its rows, a column of values per component.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageRecord {
    /// The page's components, by type name.
    pub components: Vec<String>,
    /// The entity each row belongs to.
    pub rows: Vec<PersistentId>,
    /// One column per component, in the order of `components`; each holds
    /// one value per row.
    pub columns: Vec<Vec<Record>>,
}

/// Written as `{"entities": [..], "pages": [{"rows": [..], "columns": {"Name":
/// [value per row], ..}}, ..]}` — a page's components are the keys of its
/// columns, in signature order: a text save reads as the pages the scene is
/// stored in.
impl Serialize for SceneRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("entities", &Ids(&self.entities))?;
        map.serialize_entry("pages", &self.pages)?;
        map.end()
    }
}

impl Serialize for PageRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("rows", &Ids(&self.rows))?;
        map.serialize_entry("columns", &Columns(self))?;
        map.end()
    }
}

/// Identities as the 64-bit numbers a file holds.
struct Ids<'a>(&'a [PersistentId]);

impl Serialize for Ids<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|id| id.to_bits()))
    }
}

/// A page's columns, keyed by component name in signature order.
struct Columns<'a>(&'a PageRecord);

impl Serialize for Columns<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.components.len()))?;
        for (name, column) in self.0.components.iter().zip(&self.0.columns) {
            map.serialize_entry(name, column)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for SceneRecord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            entities: Vec<u64>,
            pages: Vec<RawPage>,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawPage {
            rows: Vec<u64>,
            columns: RawColumns,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(SceneRecord {
            entities: raw
                .entities
                .into_iter()
                .map(PersistentId::from_bits)
                .collect(),
            pages: raw
                .pages
                .into_iter()
                .map(|page| {
                    let (components, columns) = page.columns.0.into_iter().unzip();
                    PageRecord {
                        components,
                        rows: page.rows.into_iter().map(PersistentId::from_bits).collect(),
                        columns,
                    }
                })
                .collect(),
        })
    }
}

/// A page's columns read back in the order the file lists them.
struct RawColumns(Vec<(String, Vec<Record>)>);

impl<'de> Deserialize<'de> for RawColumns {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ColumnsVisitor;

        impl<'de> Visitor<'de> for ColumnsVisitor {
            type Value = RawColumns;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("columns keyed by component name")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RawColumns, A::Error> {
                let mut columns: Vec<(String, Vec<Record>)> =
                    Vec::with_capacity(map.size_hint().unwrap_or(0).min(256));
                while let Some((name, column)) = map.next_entry::<String, Vec<Record>>()? {
                    if columns.iter().any(|(known, _)| *known == name) {
                        return Err(de::Error::custom(format!(
                            "component `{name}` appears twice in one page"
                        )));
                    }
                    columns.push((name, column));
                }
                Ok(RawColumns(columns))
            }
        }

        deserializer.deserialize_map(ColumnsVisitor)
    }
}
