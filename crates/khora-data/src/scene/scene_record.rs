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

//! A scene written down page by page, values by name, never a column's memory.
//!
//! See the book: concepts/serialization.md, "Pages, persistent ids, provenance".

use khora_core::ecs::PersistentId;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use khora_core::asset::AssetUUID;

use super::declared::DeclaredRecord;
use super::record::Record;
use super::save::SaveRecord;

/// Everything a scene, a prefab or a save holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SceneRecord {
    /// Every entity in capture order, including one with no component worth saving.
    pub entities: Vec<PersistentId>,
    /// The pages their components are stored in.
    pub pages: Vec<PageRecord>,
    /// The prefab instances kept as links; their entities are not in `pages`.
    pub instances: Vec<InstanceRecord>,
    /// The declarations, sorted by name, of the run-time-declared components `pages` hold.
    ///
    /// No registration knows such a component, so the record describes it.
    pub declared: Vec<DeclaredRecord>,
}

/// Written as `{"entities": [..], "pages": [{"rows": [..], "columns": {"Name": [..]}}]}`.
///
/// Columns are keyed by component name in signature order; `instances` and
/// `declared` appear only when not empty. See the book: reference/formats.md, "The scene record".
impl Serialize for SceneRecord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let linked = !self.instances.is_empty();
        let described = !self.declared.is_empty();
        let mut map = serializer.serialize_map(Some(2 + linked as usize + described as usize))?;
        map.serialize_entry("entities", &Ids(&self.entities))?;
        map.serialize_entry("pages", &self.pages)?;
        if linked {
            map.serialize_entry("instances", &self.instances)?;
        }
        if described {
            map.serialize_entry("declared", &self.declared)?;
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
            #[serde(default)]
            instances: Vec<InstanceRecord>,
            #[serde(default)]
            declared: Vec<DeclaredRecord>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(SceneRecord {
            entities: ids_from(raw.entities),
            pages: pages_from(raw.pages),
            instances: raw.instances,
            declared: raw.declared,
        })
    }
}

/// A prefab instance kept as a link: the prefab, its root's identity, its differences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceRecord {
    /// The instance root's identity; every entity of the instance is known
    /// by it and by its own id in the prefab.
    pub root: PersistentId,
    /// The prefab the instance was made from.
    pub prefab: AssetUUID,
    /// The instance's differences from its prefab, as expanded under `root`.
    pub delta: SaveRecord,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPage {
    rows: Vec<u64>,
    columns: RawColumns,
}

fn ids_from(raw: Vec<u64>) -> Vec<PersistentId> {
    raw.into_iter().map(PersistentId::from_bits).collect()
}

fn pages_from(raw: Vec<RawPage>) -> Vec<PageRecord> {
    raw.into_iter()
        .map(|page| {
            let (components, columns) = page.columns.0.into_iter().unzip();
            PageRecord {
                components,
                rows: ids_from(page.rows),
                columns,
            }
        })
        .collect()
}

/// Reads a record that cannot link to prefabs: a save's or an instance's differences.
///
/// Refusing `instances` there keeps a file from nesting links without end.
pub(super) fn plain_record<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<SceneRecord, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Raw {
        entities: Vec<u64>,
        pages: Vec<RawPage>,
        #[serde(default)]
        declared: Vec<DeclaredRecord>,
    }
    let raw = Raw::deserialize(deserializer)?;
    Ok(SceneRecord {
        entities: ids_from(raw.entities),
        pages: pages_from(raw.pages),
        instances: Vec::new(),
        declared: raw.declared,
    })
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
