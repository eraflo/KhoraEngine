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

//! A game save: how a running world differs from the scene it started from,
//! merged three-way into the scene as it stands at load time.
//!
//! See the book: concepts/saves.md, "Loading a save: a three-way merge".

use std::collections::{HashMap, HashSet};

use khora_core::asset::AssetUUID;
use khora_core::ecs::PersistentId;
use khora_core::scene::{SceneFile, SAVE_MAGIC_BYTES};
use serde::{Deserialize, Serialize};

use super::apply::{prepare_kept, prepare_kept_with, Identity, LoadFailure, Prepared};
use super::capture::{capture_game, capture_world, SaveError};
use super::component_registration::{registration_named, Kept};
use super::declared::{declarations_of, described, register_all, DeclaredRecord};
use super::encoding::{encoding_named, EncodingError, SceneEncoding};
use super::entity_refs::{parent_in, referenced};
use super::file::{check_save_format, encoding_of, file_with, SceneFileReadError};
use super::record::{diff, patch, same, LoadReport, Record, ReportEntry, ReportKind};
use super::scene_record::{PageRecord, SceneRecord};
use crate::ecs::World;

mod read;
mod reports;
pub(crate) use read::scene_as_read;
use read::{as_read, Read, Unreadable};
pub use reports::about_what_loads;

/// What a game save holds, against the scene it was taken from.
///
/// `changes` and `before` are page-shaped scene records; an entity `changes`
/// does not list is the scene's, untouched.
/// See the book: concepts/saves.md, "What a save holds".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRecord {
    /// The scene the save was taken against.
    pub base: AssetUUID,
    /// Every entity of the saved world, tree by tree, each parent's children in order.
    ///
    /// The hierarchy is rebuilt from it, so siblings keep the order play left them in.
    pub order: Vec<PersistentId>,
    /// The scene's entities the game destroyed.
    pub destroyed: Vec<PersistentId>,
    /// The entities the game made; every other entity `changes` lists is the scene's.
    pub created: Vec<PersistentId>,
    /// The components the game removed from the scene's entities, by name.
    ///
    /// Named, not inferred from what a row lacks: a component the author adds
    /// to the scene after the save is not one the game removed.
    pub removed: Vec<RemovedComponents>,
    /// Every entity that differs from the scene, with the differing components whole.
    ///
    /// Those are the components the game changed or added on the scene's
    /// entities, and every component of the entities it made.
    #[serde(deserialize_with = "super::scene_record::plain_record")]
    pub changes: SceneRecord,
    /// The scene's values, when the save was taken, of the components the game changed.
    ///
    /// A load compares the game's values with these, field by field.
    #[serde(deserialize_with = "super::scene_record::plain_record")]
    pub before: SceneRecord,
}

/// The components the game removed from one of the scene's entities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovedComponents {
    /// The scene's entity.
    pub entity: PersistentId,
    /// The components the game removed from it, by name.
    pub components: Vec<String>,
}

/// An entity's components in a record, by name, in its page's order.
pub(super) type Components<'r> = Vec<(&'r str, &'r Record)>;

/// Returns every entity of `record` that has components, with its components.
pub(super) fn components_of(record: &SceneRecord) -> HashMap<PersistentId, Components<'_>> {
    let mut entities: HashMap<PersistentId, Components<'_>> = HashMap::new();
    for page in &record.pages {
        for (row, id) in page.rows.iter().enumerate() {
            let components = entities.entry(*id).or_default();
            for (name, column) in page.components.iter().zip(&page.columns) {
                if let Some(value) = column.get(row) {
                    components.push((name.as_str(), value));
                }
            }
        }
    }
    entities
}

fn component<'r>(components: &Components<'r>, name: &str) -> Option<&'r Record> {
    components
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, value)| *value)
}

pub(super) fn owned(components: &Components<'_>) -> Vec<(String, Record)> {
    components
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).clone()))
        .collect()
}

/// Pages being built, one per signature, in the order they are first met.
#[derive(Default)]
pub(super) struct Pages {
    pub(super) pages: Vec<PageRecord>,
    by_signature: HashMap<Vec<String>, usize>,
}

impl Pages {
    pub(super) fn push(&mut self, id: PersistentId, mut components: Vec<(String, Record)>) {
        if components.is_empty() {
            return;
        }
        // Sorted, as a capture sorts a page's components: one signature, one
        // page, whichever order the components were gathered in.
        components.sort_by(|(a, _), (b, _)| a.cmp(b));
        let signature: Vec<String> = components.iter().map(|(name, _)| name.clone()).collect();
        let pages = &mut self.pages;
        let index = *self
            .by_signature
            .entry(signature)
            .or_insert_with_key(|signature| {
                pages.push(PageRecord {
                    components: signature.clone(),
                    rows: Vec::new(),
                    columns: vec![Vec::new(); signature.len()],
                });
                pages.len() - 1
            });
        let page = &mut self.pages[index];
        page.rows.push(id);
        for (column, (_, value)) in page.columns.iter_mut().zip(components) {
            column.push(value);
        }
    }
}

/// Returns how `world` differs from `base`, the scene known as `base_id` it started from.
///
/// # Errors
/// `base` cannot be read, or the world cannot be captured.
pub fn capture_save(
    world: &World,
    base_id: AssetUUID,
    base: &SceneRecord,
) -> Result<SaveRecord, SaveError> {
    // Read as the running world declares its components: a component only
    // redeclared since the scene was saved is not a change the game made.
    // Nothing is loaded from it, so the reading's report is not needed.
    let (base, _) = scene_as_read(base, &declarations_of(world))
        .map_err(|failure| SaveError::Base(failure.message))?;
    let now = capture_game(world)?;
    Ok(differences(base_id, &base, &now))
}

/// Returns how `now` differs from `base`, two records of the components a scene keeps.
///
/// Each is first read as this build reads it, among the entities its values refer to.
pub(super) fn delta_between(
    base_id: AssetUUID,
    base: &SceneRecord,
    now: &SceneRecord,
) -> Result<SaveRecord, SaveError> {
    let read = |record: &SceneRecord| {
        let mut among = record.entities.clone();
        let listed: HashSet<PersistentId> = among.iter().copied().collect();
        among.extend(
            referenced(record)
                .into_iter()
                .filter(|id| !listed.contains(id)),
        );
        // Read only to compare, as above.
        as_read(record, among, Kept::Scene, &now.declared)
            .map(|(canonical, _)| SceneRecord {
                entities: record.entities.clone(),
                ..canonical
            })
            .map_err(|failure| SaveError::Base(failure.message))
    };
    Ok(differences(base_id, &read(base)?, &read(now)?))
}

/// Returns what a save of `now` against `base` holds.
fn differences(base_id: AssetUUID, base: &SceneRecord, now: &SceneRecord) -> SaveRecord {
    let base_components = components_of(base);
    let now_components = components_of(now);
    let in_base: HashSet<PersistentId> = base.entities.iter().copied().collect();
    let alive: HashSet<PersistentId> = now.entities.iter().copied().collect();

    let mut entities = Vec::new();
    let mut created = Vec::new();
    let mut removed = Vec::new();
    let mut changes = Pages::default();
    let mut before = Pages::default();
    let mut before_entities = Vec::new();
    let nothing = Components::new();
    for id in &now.entities {
        let current = now_components.get(id).unwrap_or(&nothing);
        if !in_base.contains(id) {
            created.push(*id);
            entities.push(*id);
            changes.push(*id, owned(current));
            continue;
        }
        let scene = base_components.get(id).unwrap_or(&nothing);
        let gone: Vec<String> = scene
            .iter()
            .filter(|(name, _)| component(current, name).is_none())
            .map(|(name, _)| (*name).to_owned())
            .collect();
        let mut differing = Vec::new();
        let mut was = Vec::new();
        for (name, value) in current {
            match component(scene, name) {
                Some(old) if same(old, value) => {}
                Some(old) => {
                    differing.push(((*name).to_owned(), (*value).clone()));
                    was.push(((*name).to_owned(), old.clone()));
                }
                None => differing.push(((*name).to_owned(), (*value).clone())),
            }
        }
        if differing.is_empty() && gone.is_empty() {
            continue;
        }
        if !gone.is_empty() {
            removed.push(RemovedComponents {
                entity: *id,
                components: gone,
            });
        }
        entities.push(*id);
        changes.push(*id, differing);
        if !was.is_empty() {
            before_entities.push(*id);
            before.push(*id, was);
        }
    }

    SaveRecord {
        base: base_id,
        order: now.entities.clone(),
        destroyed: base
            .entities
            .iter()
            .filter(|id| !alive.contains(id))
            .copied()
            .collect(),
        created,
        removed,
        changes: SceneRecord {
            entities,
            instances: Vec::new(),
            declared: described(&changes.pages, &now.declared),
            pages: changes.pages,
        },
        before: SceneRecord {
            entities: before_entities,
            instances: Vec::new(),
            declared: described(&before.pages, &base.declared),
            pages: before.pages,
        },
    }
}

/// What a merge reads, per entity: the scene's current components, the
/// save's, and the scene's at the save.
struct Sides<'r> {
    scene: HashMap<PersistentId, Components<'r>>,
    changes: HashMap<PersistentId, Components<'r>>,
    before: HashMap<PersistentId, Components<'r>>,
}

impl<'r> Sides<'r> {
    fn of(read: &'r Read) -> Self {
        Sides {
            scene: components_of(&read.base),
            changes: components_of(&read.changes),
            before: components_of(&read.before),
        }
    }

    /// Returns one of the scene's entities as the save leaves it, merged three-way.
    ///
    /// A component the author removed since stays removed, reported as `RemovedFromScene`.
    /// See the book: concepts/saves.md, "Loading a save: a three-way merge".
    fn merged(
        &self,
        id: PersistentId,
        removed: &[String],
        report: &mut Vec<ReportEntry>,
    ) -> Vec<(String, Record)> {
        let nothing = Components::new();
        let current = self.scene.get(&id).unwrap_or(&nothing);
        let now = self.changes.get(&id).unwrap_or(&nothing);
        let was = self.before.get(&id).unwrap_or(&nothing);

        let mut components: Vec<(String, Record)> = current
            .iter()
            .filter(|(name, _)| !removed.iter().any(|gone| gone == name))
            .filter(|(name, _)| component(now, name).is_none())
            .map(|(name, value)| ((*name).to_owned(), (*value).clone()))
            .collect();
        for (name, value) in now {
            match (component(was, name), component(current, name)) {
                (Some(old), Some(today)) => {
                    components.push(((*name).to_owned(), patch(today, &diff(old, value))));
                }
                (Some(_), None) => report.push(ReportEntry {
                    entity: Some(id),
                    component: Some((*name).to_owned()),
                    path: String::new(),
                    kind: ReportKind::RemovedFromScene,
                }),
                (None, _) => components.push(((*name).to_owned(), (*value).clone())),
            }
        }
        components
    }
}

/// Returns the world `save` describes, merged into `base`, in the order the save left it.
///
/// A scene or a save that cannot be read is handed back as the record that
/// could not, so loading the result fails the way loading it would.
pub fn compose(base: &SceneRecord, save: &SaveRecord) -> SceneRecord {
    match composed(base, save) {
        Ok((record, _)) => record,
        Err(unreadable) => unreadable.0,
    }
}

/// Does [`compose`], and returns what it left out.
///
/// That is what the save changed that the scene does not have: the author's removal wins.
///
/// # Errors
/// The scene or the save cannot be read.
pub fn compose_reporting(
    base: &SceneRecord,
    save: &SaveRecord,
) -> Result<(SceneRecord, Vec<ReportEntry>), LoadFailure> {
    composed(base, save).map_err(|unreadable| unreadable.1)
}

fn composed(
    base: &SceneRecord,
    save: &SaveRecord,
) -> Result<(SceneRecord, Vec<ReportEntry>), Unreadable> {
    let read = Read::new(base, save)?;
    let sides = Sides::of(&read);
    let mut report = Vec::new();
    let entities = read.order(save, &mut report);
    let nothing = Components::new();

    let mut merged: Vec<(PersistentId, Vec<(String, Record)>)> = entities
        .iter()
        .map(|id| {
            let components = if read.created.contains(id) {
                owned(sides.changes.get(id).unwrap_or(&nothing))
            } else if read.diverged(*id) {
                sides.merged(*id, read.removed_from(*id), &mut report)
            } else {
                owned(sides.scene.get(id).unwrap_or(&nothing))
            };
            (*id, components)
        })
        .collect();
    // What hangs from an entity the save destroyed went with it: a child the
    // author added under it since is not brought back, parentless.
    let mut gone = read.destroyed.clone();
    loop {
        let before = gone.len();
        for (id, components) in &merged {
            if parent_in(components).is_some_and(|parent| gone.contains(&parent)) {
                gone.insert(*id);
            }
        }
        if gone.len() == before {
            break;
        }
    }
    merged.retain(|(id, _)| !gone.contains(id));
    let entities: Vec<PersistentId> = merged.iter().map(|(id, _)| *id).collect();
    let mut pages = Pages::default();
    for (id, components) in merged {
        pages.push(id, components);
    }
    let record = SceneRecord {
        entities,
        instances: Vec::new(),
        declared: described(&pages.pages, read.declarations()),
        pages: pages.pages,
    };
    let report = about_what_loads(&record, read.report.iter().cloned().chain(report));
    Ok((record, report))
}

/// Returns `base` with what `save` observed of its surviving entities written in as authored.
///
/// A run's spawns and kills are what play did, not what the author meant: only
/// the scene's own entities' values are promoted, never created entities,
/// destructions or runtime state. A scene or a save that cannot be read promotes nothing.
pub fn promote(base: &SceneRecord, save: &SaveRecord) -> SceneRecord {
    let Ok(read) = Read::new(base, save) else {
        return base.clone();
    };
    let sides = Sides::of(&read);
    let nothing = Components::new();
    // A component the records describe themselves was authored: it is one
    // a script declares, saved by a scene.
    let authored = |name: &str| {
        registration_named(name).is_some_and(|reg| reg.is_saved())
            || read
                .declarations()
                .any(|declared| declared.name == name && !declared.resumable)
    };

    let mut pages = Pages::default();
    let mut ignored = Vec::new();
    for id in &base.entities {
        let promoted =
            read.diverged(*id) && !read.destroyed.contains(id) && !read.created.contains(id);
        let components = if promoted {
            sides
                .merged(*id, read.removed_from(*id), &mut ignored)
                .into_iter()
                .filter(|(name, _)| authored(name))
                .collect()
        } else {
            owned(sides.scene.get(id).unwrap_or(&nothing))
        };
        pages.push(*id, components);
    }
    SceneRecord {
        entities: base.entities.clone(),
        instances: Vec::new(),
        declared: described(&pages.pages, read.declarations()),
        pages: pages.pages,
    }
}

/// Reads and checks a composed game record against `world`, as [`prepare`](super::prepare) does.
///
/// Unlike a scene, it admits the runtime components a save keeps to resume.
///
/// # Errors
/// As [`prepare`](super::prepare).
pub fn prepare_game(world: &mut World, record: &SceneRecord) -> Result<Prepared, LoadFailure> {
    prepare_kept(world, record, Kept::Game)
}

/// Does [`prepare_game`] without the world, reading `catalog` and reserving ids into `hold`.
///
/// # Errors
/// As [`prepare`](super::prepare).
pub fn prepare_game_with(
    catalog: &crate::scene::LoadCatalog,
    hold: khora_core::ecs::ReservationHold,
    record: &SceneRecord,
) -> Result<Prepared, LoadFailure> {
    prepare_kept_with(catalog, hold, record, Kept::Game)
}

/// Returns `save` written with `encoding` behind a save header.
///
/// # Errors
/// The encoding cannot write the save.
pub fn write_save_file(
    save: &SaveRecord,
    encoding: &dyn SceneEncoding,
) -> Result<SceneFile, EncodingError> {
    file_with(SAVE_MAGIC_BYTES, encoding.id(), encoding.encode_save(save)?)
}

/// Returns the save a save file holds.
///
/// # Errors
/// The file is not a save, is of an older or newer format, names an unknown
/// encoding, or its payload does not decode.
pub fn read_save_file(file: &SceneFile) -> Result<SaveRecord, SceneFileReadError> {
    check_save_format(file)?;
    let id = encoding_of(file);
    let encoding = encoding_named(&id).ok_or(SceneFileReadError::UnknownEncoding(id))?;
    encoding
        .decode_save(&file.payload)
        .map_err(SceneFileReadError::Encoding)
}
