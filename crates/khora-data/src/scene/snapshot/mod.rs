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

//! A world as a snapshot: the fastest file to load, bound to the schema that
//! wrote it.
//!
//! Values are positional — no names, no record tree, no name resolution on
//! load — and every component is listed with its schema fingerprint. A
//! snapshot whose fingerprints differ from the running registrations is
//! refused whole: it is the same build's cache of a stable save, never its
//! only copy.
//!
//! Payload: `version: u8`; the component table — a count, then each name
//! (length and UTF-8) and its fingerprint (`u64` LE); the entities — a
//! count, then each persistent id (`u64` LE); the pages — a count, then for
//! each its components (a count, then table indices), its rows (a count,
//! then persistent ids) and its values column after column, each value its
//! length then its bytes. Counts and lengths are LEB128 varints.

use std::any::TypeId;
use std::collections::HashMap;

use khora_core::ecs::PersistentId;
use khora_core::scene::SceneFile;

use super::apply::{failure, prepare_pages, LoadFailure, PagePlan, Prepared};
use super::capture::{capture_pages, world_in_tree_order, SaveError};
use super::component_registration::{ComponentRegistration, Kept};
use super::file::{check_format, encoding_of, scene_file};
use super::positional::write_varint;
use super::record::LoadReport;
use crate::ecs::World;

/// The id a scene header names the snapshot encoding by.
pub const SNAPSHOT_ENCODING_ID: &str = "KH_SNAPSHOT_V1";

/// The payload layout this module writes and reads.
const VERSION: u8 = 1;

/// Writes every entity of `world` and its saved components as a snapshot.
pub fn write_snapshot(world: &World) -> Result<SceneFile, SaveError> {
    let entities = world_in_tree_order(world);
    let (ids, pages) = capture_pages(
        world,
        &entities,
        None,
        Kept::Scene,
        |reg, column, row, references| {
            let mut bytes = Vec::new();
            (reg.column_to_snapshot)(column, row, &mut bytes, references)?;
            Ok(bytes)
        },
    )?;

    // Every component the pages hold, once, in the order first met — each
    // one a fingerprint can guard.
    let mut table: Vec<&'static ComponentRegistration> = Vec::new();
    let mut index_of: HashMap<TypeId, usize> = HashMap::new();
    for page in &pages {
        for reg in &page.components {
            if let std::collections::hash_map::Entry::Vacant(slot) = index_of.entry(reg.type_id) {
                if !(reg.schema_complete)() {
                    return Err(SaveError::Unguarded(reg.type_name.to_owned()));
                }
                slot.insert(table.len());
                table.push(*reg);
            }
        }
    }

    let mut payload = vec![VERSION];
    write_varint(&mut payload, table.len() as u64);
    for reg in &table {
        write_varint(&mut payload, reg.type_name.len() as u64);
        payload.extend_from_slice(reg.type_name.as_bytes());
        payload.extend_from_slice(&(reg.schema)().to_le_bytes());
    }
    write_varint(&mut payload, ids.len() as u64);
    for id in &ids {
        payload.extend_from_slice(&id.to_bits().to_le_bytes());
    }
    write_varint(&mut payload, pages.len() as u64);
    for page in &pages {
        write_varint(&mut payload, page.components.len() as u64);
        for reg in &page.components {
            write_varint(&mut payload, index_of[&reg.type_id] as u64);
        }
        write_varint(&mut payload, page.rows.len() as u64);
        for id in &page.rows {
            payload.extend_from_slice(&id.to_bits().to_le_bytes());
        }
        // Column by column, as the page stores them.
        for column in &page.columns {
            for value in column {
                write_varint(&mut payload, value.len() as u64);
                payload.extend_from_slice(value);
            }
        }
    }
    scene_file(SNAPSHOT_ENCODING_ID, payload).map_err(|error| SaveError::Encoding(error.0))
}

/// Reads and checks the snapshot `file` holds against `world`'s
/// registrations, reserving an id for each of its entities, without adding
/// anything — committed through the shared [`Prepared::commit`].
pub fn prepare_snapshot(world: &mut World, file: &SceneFile) -> Result<Prepared, LoadFailure> {
    let refused = |message: String| failure(message, LoadReport::default());
    let encoding = encoding_of(file);
    if encoding != SNAPSHOT_ENCODING_ID {
        return Err(refused(format!("a `{encoding}` file is not a snapshot")));
    }
    check_format(file).map_err(|error| refused(error.to_string()))?;
    let snapshot = Snapshot::read(&file.payload).map_err(refused)?;

    let plans = snapshot
        .pages
        .iter()
        .map(|page| PagePlan {
            rows: &page.rows,
            columns: page
                .components
                .iter()
                .enumerate()
                .map(|(column, &table)| (column, snapshot.table[table]))
                .collect(),
        })
        .collect();
    prepare_pages(
        world,
        &snapshot.entities,
        plans,
        LoadReport::default(),
        |reg, page, column, row, references| {
            (reg.stage_snapshot)(snapshot.pages[page].values[column][row], references)
        },
    )
}

/// A snapshot's payload, read and checked, its values not yet.
struct Snapshot<'p> {
    table: Vec<&'static ComponentRegistration>,
    entities: Vec<PersistentId>,
    pages: Vec<SnapshotPage<'p>>,
}

struct SnapshotPage<'p> {
    /// Indices into the component table.
    components: Vec<usize>,
    rows: Vec<PersistentId>,
    /// Each column's values, a byte slice per row.
    values: Vec<Vec<&'p [u8]>>,
}

impl<'p> Snapshot<'p> {
    fn read(payload: &'p [u8]) -> Result<Self, String> {
        let mut input = Input {
            bytes: payload,
            at: 0,
        };
        let version = input.byte()?;
        if version != VERSION {
            return Err(format!(
                "snapshot layout {version}, this engine reads {VERSION}"
            ));
        }

        let count = input.count(9)?;
        let mut table = Vec::with_capacity(count);
        for _ in 0..count {
            let len = input.count(1)?;
            let name = std::str::from_utf8(input.take(len)?)
                .map_err(|_| "a component name that is not UTF-8".to_owned())?;
            let fingerprint = input.u64_le()?;
            table.push(registration_for(name, fingerprint)?);
        }

        let count = input.count(8)?;
        let mut entities = Vec::with_capacity(count);
        for _ in 0..count {
            entities.push(PersistentId::from_bits(input.u64_le()?));
        }

        let count = input.count(3)?;
        let mut pages = Vec::with_capacity(count);
        for _ in 0..count {
            let columns = input.count(1)?;
            let mut components = Vec::with_capacity(columns);
            for _ in 0..columns {
                let index = usize::try_from(input.varint()?).unwrap_or(usize::MAX);
                if index >= table.len() {
                    return Err(format!("component {index} is not in the table"));
                }
                components.push(index);
            }
            let count = input.count(8)?;
            let mut rows = Vec::with_capacity(count);
            for _ in 0..count {
                rows.push(PersistentId::from_bits(input.u64_le()?));
            }
            // Every value takes at least its length byte.
            if columns
                .checked_mul(rows.len())
                .is_none_or(|values| values > input.left())
            {
                return Err(format!(
                    "a page of {} rows by {columns} columns the file has no room for",
                    rows.len()
                ));
            }
            let mut values = Vec::with_capacity(columns);
            for _ in 0..columns {
                let mut column = Vec::with_capacity(rows.len());
                for _ in 0..rows.len() {
                    let len = input.count(1)?;
                    column.push(input.take(len)?);
                }
                values.push(column);
            }
            pages.push(SnapshotPage {
                components,
                rows,
                values,
            });
        }
        if input.left() > 0 {
            return Err(format!("{} byte(s) after the last page", input.left()));
        }
        Ok(Self {
            table,
            entities,
            pages,
        })
    }
}

/// The registration a snapshot's component is read with: this build's own
/// name for it, its schema the one that wrote it, a component a save holds.
fn registration_for(
    name: &str,
    fingerprint: u64,
) -> Result<&'static ComponentRegistration, String> {
    let reg = inventory::iter::<ComponentRegistration>
        .into_iter()
        .find(|reg| reg.type_name == name)
        .ok_or_else(|| format!("unknown component type `{name}` in a snapshot"))?;
    if !reg.is_saved() {
        return Err(format!(
            "component `{name}` is derived or runtime state; a snapshot does not hold it"
        ));
    }
    if !(reg.schema_complete)() {
        return Err(format!(
            "component `{name}` has a schema no fingerprint can guard; a snapshot does not hold it"
        ));
    }
    let current = (reg.schema)();
    if current != fingerprint {
        return Err(format!(
            "component `{name}` was written by another schema \
             ({fingerprint:016x}, this build has {current:016x}): load its stable save"
        ));
    }
    Ok(reg)
}

/// A cursor over the payload. Every count is checked against the bytes left
/// before anything is sized from it: a count is the file's claim.
struct Input<'p> {
    bytes: &'p [u8],
    at: usize,
}

impl<'p> Input<'p> {
    fn left(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn byte(&mut self) -> Result<u8, String> {
        let byte = *self
            .bytes
            .get(self.at)
            .ok_or_else(|| "the snapshot ends early".to_owned())?;
        self.at += 1;
        Ok(byte)
    }

    fn take(&mut self, len: usize) -> Result<&'p [u8], String> {
        if len > self.left() {
            return Err("the snapshot ends early".to_owned());
        }
        let taken = &self.bytes[self.at..self.at + len];
        self.at += len;
        Ok(taken)
    }

    fn u64_le(&mut self) -> Result<u64, String> {
        let mut array = [0; 8];
        array.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(array))
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            let bits = u64::from(byte & 0x7f);
            if shift == 63 && bits > 1 {
                return Err("a number too large for 64 bits".to_owned());
            }
            value |= bits << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("a number too large for 64 bits".to_owned())
    }

    /// A count of things each taking at least `each` bytes.
    fn count(&mut self, each: usize) -> Result<usize, String> {
        let count = self.varint()?;
        let count = usize::try_from(count).map_err(|_| "a count too large".to_owned())?;
        if count
            .checked_mul(each)
            .is_none_or(|needed| needed > self.left())
        {
            return Err(format!("a count of {count} the snapshot has no room for"));
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests;
