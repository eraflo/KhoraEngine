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

//! A game save: how a running world differs from the scene it started from.
//!
//! A scene holds what was authored. A save holds, against that scene, what the
//! game changed — the components it changed or added on the scene's entities,
//! the entities it made, the ones it destroyed — plus the state the engine
//! needs to resume. Loading merges the save into the scene as it is *now*, so
//! an edit made to the scene since reaches every value the game left alone.
//!
//! # A three-way merge
//!
//! A changed component is kept whole, beside its value in the scene when the
//! save was taken. Loading reads both through today's type — a component or
//! field renamed since, a struct a text format wrote as a map, a number at
//! another width all come back as today's values — then compares them field
//! by field, and only the fields the game changed are written over the scene's
//! current value. A partial value could not be read that way: a field it lacks
//! is not a field it changed, and a name it has may be one the type gave up.

use std::collections::{HashMap, HashSet};

use khora_core::asset::AssetUUID;
use khora_core::ecs::PersistentId;
use khora_core::scene::{SceneFile, SAVE_MAGIC_BYTES};
use serde::{Deserialize, Serialize};

use super::apply::{prepare_kept, Identity, LoadFailure, Prepared};
use super::capture::{capture_game, capture_world, SaveError};
use super::component_registration::{registration_named, Kept};
use super::encoding::{encoding_named, EncodingError, SceneEncoding};
use super::entity_refs::{parent_in, referenced};
use super::file::{check_save_format, encoding_of, file_with, SceneFileReadError};
use super::record::{diff, patch, same, Record, ReportEntry, ReportKind};
use super::scene_record::{PageRecord, SceneRecord};
use crate::ecs::World;

/// What a game save holds, against the scene it was taken from.
///
/// Page-shaped, as a scene is: `changes` and `before` are scene records, each
/// entity in the page its components make. An entity `changes` does not list
/// is the scene's, untouched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRecord {
    /// The scene the save was taken against.
    pub base: AssetUUID,
    /// Every entity of the saved world, tree by tree, each parent's children
    /// in their order — what the hierarchy is rebuilt from, so siblings keep
    /// the order play left them in.
    pub order: Vec<PersistentId>,
    /// The scene's entities the game destroyed.
    pub destroyed: Vec<PersistentId>,
    /// The entities the game made. Every other entity `changes` lists is one
    /// of the scene's.
    pub created: Vec<PersistentId>,
    /// The components the game removed from the scene's entities. Named, not
    /// inferred from what a row lacks: a component the author adds to the
    /// scene after the save is not one the game removed.
    pub removed: Vec<RemovedComponents>,
    /// Every entity that differs from the scene, with the components that
    /// differ, whole: those the game changed or added on the scene's
    /// entities, every component of the entities it made.
    #[serde(deserialize_with = "super::scene_record::plain_record")]
    pub changes: SceneRecord,
    /// The scene's values, when the save was taken, of the components the
    /// game changed — what a load compares the game's values with, field by
    /// field, so an edit the author made since to a field the game left alone
    /// is kept.
    #[serde(deserialize_with = "super::scene_record::plain_record")]
    pub before: SceneRecord,
}

/// The components the game removed from one of the scene's entities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovedComponents {
    /// The scene's entity.
    pub entity: PersistentId,
    /// The components it no longer has, by name.
    pub components: Vec<String>,
}

/// An entity's components in a record, by name, in its page's order.
pub(super) type Components<'r> = Vec<(&'r str, &'r Record)>;

/// Every entity of `record` that has components, and its components.
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

/// `record` as this build reads it: loaded into a world of its own, among
/// `entities`, and written down again — components by today's names, fields
/// by today's names, every value at today's type.
///
/// `entities` are every entity the values may refer to, so a reference to
/// one the record has no row for is still a reference, not an outside one.
pub(super) fn as_read(
    record: &SceneRecord,
    entities: Vec<PersistentId>,
    kept: Kept,
) -> Result<SceneRecord, LoadFailure> {
    let mut world = World::new();
    let whole = SceneRecord {
        entities,
        instances: Vec::new(),
        pages: record.pages.clone(),
    };
    prepare_kept(&mut world, &whole, kept)?.commit(&mut world, Identity::Keep);
    let captured = match kept {
        Kept::Scene => capture_world(&world),
        Kept::Game => capture_game(&world),
    };
    captured.map_err(|error| LoadFailure {
        message: error.to_string(),
        report: Default::default(),
    })
}

/// `scene` as this build reads it.
fn scene_as_read(scene: &SceneRecord) -> Result<SceneRecord, LoadFailure> {
    as_read(scene, scene.entities.clone(), Kept::Scene)
}

/// How `world` differs from `base`, the scene it started from, known as
/// `base_id`.
pub fn capture_save(
    world: &World,
    base_id: AssetUUID,
    base: &SceneRecord,
) -> Result<SaveRecord, SaveError> {
    let base = scene_as_read(base).map_err(|failure| SaveError::Base(failure.message))?;
    let now = capture_game(world)?;
    Ok(differences(base_id, &base, &now))
}

/// How `now` differs from `base`, two scene records of the components a
/// scene keeps. Each is read as this build reads it first, among the entities
/// its values refer to.
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
        as_read(record, among, Kept::Scene)
            .map(|canonical| SceneRecord {
                entities: record.entities.clone(),
                ..canonical
            })
            .map_err(|failure| SaveError::Base(failure.message))
    };
    Ok(differences(base_id, &read(base)?, &read(now)?))
}

/// What a save of `now` against `base` holds.
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
            pages: changes.pages,
        },
        before: SceneRecord {
            entities: before_entities,
            instances: Vec::new(),
            pages: before.pages,
        },
    }
}

/// A record that could not be read, and why.
type Unreadable = (SceneRecord, LoadFailure);

/// A save, read for merging: its values as this build reads them, and what it
/// says of each entity.
struct Read {
    base: SceneRecord,
    changes: SceneRecord,
    before: SceneRecord,
    destroyed: HashSet<PersistentId>,
    created: HashSet<PersistentId>,
    listed: HashSet<PersistentId>,
    removed: HashMap<PersistentId, Vec<String>>,
}

impl Read {
    /// Reads `save` against `base`. What cannot be read is the error, with
    /// the record that could not.
    fn new(base: &SceneRecord, save: &SaveRecord) -> Result<Read, Unreadable> {
        let base = scene_as_read(base).map_err(|failure| (base.clone(), failure))?;
        let in_base: HashSet<PersistentId> = base.entities.iter().copied().collect();
        let mut everyone = base.entities.clone();
        let mut seen = in_base.clone();
        for id in save
            .order
            .iter()
            .chain(&save.changes.entities)
            .chain(&save.before.entities)
            .chain(&referenced(&save.changes))
            .chain(&referenced(&save.before))
        {
            if seen.insert(*id) {
                everyone.push(*id);
            }
        }
        let unreadable = |record: &SceneRecord, failure: LoadFailure| {
            let record = SceneRecord {
                entities: everyone.clone(),
                instances: Vec::new(),
                pages: record.pages.clone(),
            };
            (record, failure)
        };
        let changes = as_read(&save.changes, everyone.clone(), Kept::Game)
            .map_err(|failure| unreadable(&save.changes, failure))?;
        let before = as_read(&save.before, everyone.clone(), Kept::Scene)
            .map_err(|failure| unreadable(&save.before, failure))?;
        // A name the save wrote is read as today's: a component renamed since
        // is the same component.
        let today = |name: &String| {
            registration_named(name).map_or_else(|| name.clone(), |reg| reg.type_name.to_owned())
        };
        Ok(Read {
            destroyed: save.destroyed.iter().copied().collect(),
            // An entity the scene has is the scene's, whatever a save says.
            created: save
                .created
                .iter()
                .filter(|id| !in_base.contains(id))
                .copied()
                .collect(),
            listed: save.changes.entities.iter().copied().collect(),
            removed: save
                .removed
                .iter()
                .map(|entry| (entry.entity, entry.components.iter().map(today).collect()))
                .collect(),
            base,
            changes,
            before,
        })
    }

    /// Whether the save says anything of `id`, one of the scene's entities.
    fn diverged(&self, id: PersistentId) -> bool {
        self.listed.contains(&id) || self.removed.contains_key(&id)
    }

    /// The components the game removed from `id`, by today's names.
    fn removed_from(&self, id: PersistentId) -> &[String] {
        self.removed.get(&id).map_or(&[][..], Vec::as_slice)
    }

    /// The order the merged world lists its entities in: the save's, less
    /// what the scene lost, with what the scene gained since right after the
    /// entity before it in the scene. Reports each entity the save changed
    /// that the scene no longer has.
    fn order(&self, save: &SaveRecord, report: &mut Vec<ReportEntry>) -> Vec<PersistentId> {
        let in_base: HashSet<PersistentId> = self.base.entities.iter().copied().collect();
        let mut seen = HashSet::new();
        let mut order = Vec::with_capacity(save.order.len());
        for id in save.order.iter().chain(&save.changes.entities) {
            if self.destroyed.contains(id) || !seen.insert(*id) {
                continue;
            }
            if self.created.contains(id) || in_base.contains(id) {
                order.push(*id);
            } else if self.listed.contains(id) {
                report.push(ReportEntry {
                    entity: Some(*id),
                    component: None,
                    path: String::new(),
                    kind: ReportKind::RemovedFromScene,
                });
            }
        }
        let mut after: HashMap<Option<PersistentId>, Vec<PersistentId>> = HashMap::new();
        let mut previous = None;
        for id in &self.base.entities {
            if seen.contains(id) {
                previous = Some(*id);
            } else if !self.destroyed.contains(id) {
                after.entry(previous).or_default().push(*id);
                previous = Some(*id);
            }
        }
        let mut entities = Vec::with_capacity(order.len());
        follow(None, &mut after, &mut entities);
        for id in order {
            entities.push(id);
            follow(Some(id), &mut after, &mut entities);
        }
        entities
    }
}

/// Lays out the entities the author added to the scene after `key`, each
/// followed by the ones added after it.
fn follow(
    key: Option<PersistentId>,
    after: &mut HashMap<Option<PersistentId>, Vec<PersistentId>>,
    out: &mut Vec<PersistentId>,
) {
    let mut stack: Vec<PersistentId> = after.remove(&key).unwrap_or_default();
    stack.reverse();
    while let Some(added) = stack.pop() {
        out.push(added);
        let mut more = after.remove(&Some(added)).unwrap_or_default();
        more.reverse();
        stack.extend(more);
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

    /// One of the scene's entities as the save leaves it: the scene's current
    /// components, less those the game removed; each the game changed merged
    /// field by field — the game's value where it differs from the scene's at
    /// the save, the scene's current one everywhere else; each it added,
    /// whole. A component the author removed since stays removed, reported.
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

/// The world `save` describes: `base`'s surviving entities, merged with what
/// the game changed, and the entities the game created, whole — in the order
/// the save left them.
///
/// A scene or a save that cannot be read is handed back as the record that
/// could not: loading it fails the way loading it would.
pub fn compose(base: &SceneRecord, save: &SaveRecord) -> SceneRecord {
    match composed(base, save) {
        Ok((record, _)) => record,
        Err((unreadable, _)) => unreadable,
    }
}

/// [`compose`], and what it had to leave out: what the save changed that the
/// scene no longer has — the author's removal wins. A scene or a save that
/// cannot be read is the error.
pub fn compose_reporting(
    base: &SceneRecord,
    save: &SaveRecord,
) -> Result<(SceneRecord, Vec<ReportEntry>), LoadFailure> {
    composed(base, save).map_err(|(_, failure)| failure)
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
    Ok((
        SceneRecord {
            entities,
            instances: Vec::new(),
            pages: pages.pages,
        },
        report,
    ))
}

/// `base`, with what `save` observed of its surviving entities written in as
/// authored — no created entities, no destructions, no runtime state.
///
/// A run's spawns and kills are what play did, not what the author meant:
/// only the values of the scene's own entities — their fields, the
/// components added to or removed from them — are promoted. A scene or a
/// save that cannot be read promotes nothing.
pub fn promote(base: &SceneRecord, save: &SaveRecord) -> SceneRecord {
    let Ok(read) = Read::new(base, save) else {
        return base.clone();
    };
    let sides = Sides::of(&read);
    let nothing = Components::new();
    let authored = |name: &str| registration_named(name).is_some_and(|reg| reg.is_saved());

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
        pages: pages.pages,
    }
}

/// Reads and checks a composed game record against `world`, as
/// [`prepare`](super::prepare) does, admitting the runtime components a save
/// keeps to resume.
pub fn prepare_game(world: &mut World, record: &SceneRecord) -> Result<Prepared, LoadFailure> {
    prepare_kept(world, record, Kept::Game)
}

/// `save`, written with `encoding` behind a save header.
pub fn write_save_file(
    save: &SaveRecord,
    encoding: &dyn SceneEncoding,
) -> Result<SceneFile, EncodingError> {
    file_with(SAVE_MAGIC_BYTES, encoding.id(), encoding.encode_save(save)?)
}

/// The save a save file holds.
pub fn read_save_file(file: &SceneFile) -> Result<SaveRecord, SceneFileReadError> {
    check_save_format(file)?;
    let id = encoding_of(file);
    let encoding = encoding_named(&id).ok_or(SceneFileReadError::UnknownEncoding(id))?;
    encoding
        .decode_save(&file.payload)
        .map_err(SceneFileReadError::Encoding)
}
