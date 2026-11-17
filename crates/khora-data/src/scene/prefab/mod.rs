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

//! Prefabs and duplicates: a subtree written down and brought back fresh.
//!
//! An instance is collapsed to a link and its differences on save, and expanded
//! from the prefab as it stands at load time.
//! See the book: concepts/prefabs.md, "A link, not a copy".

use std::collections::{HashMap, HashSet};

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::scene::SceneFile;

use super::apply::{apply, Identity, LoadFailure};
use super::capture::{capture_subtree, SaveError};
use super::declared::described;
use super::encoding::CompactEncoding;
use super::entity_refs::parent_in;
use super::file::{read_scene_file, write_scene_file};
use super::record::{to_record, EntityRef, LoadReport, Record, ReferenceWriter, ReportEntry};
use super::save::{
    about_what_loads, components_of, compose_reporting, delta_between, owned, Pages, SaveRecord,
};
use super::scene_record::{InstanceRecord, SceneRecord};
use crate::ecs::{PrefabInstance, SerializablePrefabInstance, World};

mod catalog;
mod except;
mod library;
mod overrides;
mod rekey;
mod siblings;
mod sources;

pub use catalog::PrefabCatalog;
pub use library::PrefabLibrary;
pub use overrides::{apply_to_prefab, instance_of, prefab_world, InstanceOf, PrefabApply};
use rekey::{members, moved_record, rekeyed};
use siblings::{children_of, restore_sibling_order};
use sources::{Except, ReadOnce};

/// The registered name of the component linking an instance root to its prefab.
const LINK: &str = "PrefabInstance";

/// Where the prefabs a record links to are read from.
pub trait PrefabSource {
    /// Returns the record of the prefab `id` as its file holds it, nested instances collapsed.
    ///
    /// # Errors
    /// Why the prefab cannot be read.
    fn prefab(&self, id: AssetUUID) -> Result<SceneRecord, String>;
}

/// A source with no prefab at all: every link is missing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoPrefabs;

impl PrefabSource for NoPrefabs {
    fn prefab(&self, id: AssetUUID) -> Result<SceneRecord, String> {
        Err(format!("no prefab source to read the prefab {id} from"))
    }
}

fn failed(message: String) -> LoadFailure {
    LoadFailure {
        message,
        report: LoadReport::default(),
    }
}

/// The prefabs being expanded, outermost first: one met again contains itself.
type Stack = Vec<AssetUUID>;

/// Reads the prefab `id`, refusing one that contains itself.
fn read_prefab(
    id: AssetUUID,
    prefabs: &dyn PrefabSource,
    stack: &Stack,
) -> Result<SceneRecord, LoadFailure> {
    if stack.contains(&id) {
        return Err(failed(format!("the prefab {id} contains itself")));
    }
    let record = prefabs
        .prefab(id)
        .map_err(|error| failed(format!("the prefab {id} cannot be read: {error}")))?;
    if record.entities.is_empty() {
        return Err(failed(format!("the prefab {id} holds no entity")));
    }
    Ok(record)
}

fn link_to(prefab: AssetUUID) -> Record {
    struct NoEntities;
    impl ReferenceWriter for NoEntities {
        fn write_entity(&mut self, _entity: EntityId) -> EntityRef {
            EntityRef::Outside
        }
    }
    to_record(
        &SerializablePrefabInstance::from(PrefabInstance { prefab }),
        &mut NoEntities,
    )
    .unwrap_or(Record::Asset(prefab))
}

fn linked(value: &Record) -> Option<AssetUUID> {
    match value {
        Record::Asset(id) => Some(*id),
        Record::Struct { fields, .. } => fields
            .iter()
            .find(|(name, _)| name == "prefab")
            .and_then(|(_, value)| linked(value)),
        Record::Map(entries) => entries
            .iter()
            .find(|(key, _)| matches!(key, Record::Str(name) if name == "prefab"))
            .and_then(|(_, value)| linked(value)),
        Record::Newtype { value, .. } => linked(value),
        _ => None,
    }
}

/// Returns `prefab` expanded as an unchanged instance rooted at `root`.
///
/// The link is left off the root: it is the instance's to carry.
fn prefab_under(
    prefab: AssetUUID,
    root: PersistentId,
    prefabs: &dyn PrefabSource,
    stack: &mut Stack,
) -> Result<(SceneRecord, Vec<ReportEntry>), LoadFailure> {
    let record = read_prefab(prefab, prefabs, stack)?;
    let prefab_root = record.entities[0];
    let mut map: HashMap<PersistentId, PersistentId> = record
        .entities
        .iter()
        .chain(record.pages.iter().flat_map(|page| page.rows.iter()))
        .map(|id| (*id, rekeyed(*id, prefab_root, root)))
        .collect();
    stack.push(prefab);
    let keyed = moved_record(&record, &mut map, prefabs, stack)?;
    let (expanded, report) = expand_in(&keyed, prefabs, stack)?;
    stack.pop();
    let mut rows = rows_of(&expanded);
    if let Some(components) = rows.get_mut(&root) {
        components.retain(|(name, _)| name != LINK);
    }
    Ok((
        record_from(expanded.entities.clone(), rows, &[&expanded]),
        report,
    ))
}

fn rows_of(record: &SceneRecord) -> HashMap<PersistentId, Vec<(String, Record)>> {
    components_of(record)
        .into_iter()
        .map(|(id, components)| (id, owned(&components)))
        .collect()
}

/// Returns a record of `entities` with their components from `rows`.
///
/// Script-declared components are described as the records in `from` describe them.
fn record_from(
    entities: Vec<PersistentId>,
    mut rows: HashMap<PersistentId, Vec<(String, Record)>>,
    from: &[&SceneRecord],
) -> SceneRecord {
    let mut pages = Pages::default();
    for id in &entities {
        if let Some(components) = rows.remove(id) {
            pages.push(*id, components);
        }
    }
    SceneRecord {
        declared: described(
            &pages.pages,
            from.iter().flat_map(|record| record.declared.iter()),
        ),
        entities,
        pages: pages.pages,
        instances: Vec::new(),
    }
}

/// Returns `record` with its instances expanded, and what the merges left out.
fn expand_in(
    record: &SceneRecord,
    prefabs: &dyn PrefabSource,
    stack: &mut Stack,
) -> Result<(SceneRecord, Vec<ReportEntry>), LoadFailure> {
    if record.instances.is_empty() {
        return Ok((record.clone(), Vec::new()));
    }
    let mut rows = rows_of(record);
    let mut laid_out: HashMap<PersistentId, Vec<PersistentId>> = HashMap::new();
    let mut report = Vec::new();
    let mut sources = Vec::new();
    for instance in &record.instances {
        let (base, inside) = prefab_under(instance.prefab, instance.root, prefabs, stack)?;
        // Read as the record linking to it declares its components: that
        // record's declarations describe every cell once expanded.
        let (base, rekinded) = super::asset_kinds::read_as(base, &record.declared);
        let (composed, left_out) = compose_reporting(&base, &instance.delta)?;
        report.extend(inside);
        report.extend(rekinded);
        report.extend(left_out);
        sources.push(base);
        sources.push(composed.clone());
        let mut members = rows_of(&composed);
        let root = members.entry(instance.root).or_default();
        root.retain(|(name, _)| name != LINK);
        root.push((LINK.to_owned(), link_to(instance.prefab)));
        rows.extend(members);
        laid_out.insert(instance.root, composed.entities);
    }

    let mut entities = Vec::new();
    let mut seen = HashSet::new();
    for id in &record.entities {
        lay(*id, &mut laid_out, &mut seen, &mut entities);
    }
    for instance in &record.instances {
        lay(instance.root, &mut laid_out, &mut seen, &mut entities);
    }
    // Where an instance's entities sit among others' children: the author's
    // own children of a member, a member moved under an entity of the scene.
    for instance in &record.instances {
        restore_sibling_order(&mut entities, &rows, &instance.delta.order);
    }
    // The record being opened last: its declarations win, every cell saying
    // what it holds whichever its prefab was saved under.
    sources.push(record.clone());
    let sources: Vec<&SceneRecord> = sources.iter().collect();
    Ok((record_from(entities, rows, &sources), report))
}

/// Lays `id`'s instance out where its root stands, into `entities`.
///
/// An instance whose layout names another's root lays that one out there too.
fn lay(
    id: PersistentId,
    laid_out: &mut HashMap<PersistentId, Vec<PersistentId>>,
    seen: &mut HashSet<PersistentId>,
    entities: &mut Vec<PersistentId>,
) {
    match laid_out.remove(&id) {
        Some(layout) => {
            for each in layout {
                if each == id {
                    if seen.insert(id) {
                        entities.push(id);
                    }
                } else {
                    lay(each, laid_out, seen, entities);
                }
            }
        }
        None => {
            if seen.insert(id) {
                entities.push(id);
            }
        }
    }
}

/// Returns `record` with every prefab instance it links to expanded from `prefabs`.
///
/// See the book: concepts/prefabs.md, "Identity inside an instance".
///
/// # Errors
/// A missing prefab, or one that contains itself, refuses the whole record.
pub fn expand(
    record: &SceneRecord,
    prefabs: &dyn PrefabSource,
) -> Result<SceneRecord, LoadFailure> {
    expand_reporting(record, prefabs).map(|(record, _)| record)
}

/// Does [`expand`], and returns what the merges left out.
///
/// That is what an instance changed that its prefab does not have: the prefab's
/// removal wins.
///
/// # Errors
/// As [`expand`].
pub fn expand_reporting(
    record: &SceneRecord,
    prefabs: &dyn PrefabSource,
) -> Result<(SceneRecord, Vec<ReportEntry>), LoadFailure> {
    let (expanded, report) = expand_in(record, &ReadOnce::new(prefabs), &mut Stack::new())?;
    let report = about_what_loads(&expanded, report);
    Ok((expanded, report))
}

/// Returns `record` with each outermost instance written as a link and its differences.
///
/// A nested instance is part of its outer instance's differences. An instance
/// whose prefab cannot be read is written whole, its link kept on its root.
/// See the book: concepts/prefabs.md, "When a prefab cannot be read".
///
/// # Errors
/// An instance's differences from its prefab cannot be written.
pub fn collapse(
    record: &SceneRecord,
    prefabs: &dyn PrefabSource,
) -> Result<SceneRecord, SaveError> {
    let prefabs = ReadOnce::new(prefabs);
    let rows = rows_of(record);
    let roots: Vec<(PersistentId, AssetUUID)> = record
        .entities
        .iter()
        .filter_map(|id| {
            rows.get(id)?
                .iter()
                .find(|(name, _)| name == LINK)
                .and_then(|(_, value)| linked(value))
                .map(|prefab| (*id, prefab))
        })
        .collect();
    if roots.is_empty() {
        return Ok(record.clone());
    }

    let position: HashMap<PersistentId, usize> = record
        .entities
        .iter()
        .enumerate()
        .map(|(at, id)| (*id, at))
        .collect();
    let mut membership: Vec<(PersistentId, AssetUUID, HashSet<PersistentId>)> = Vec::new();
    for (root, prefab) in &roots {
        match members(*prefab, *root, &prefabs, &mut Stack::new()) {
            Ok(all) => {
                let ids = all
                    .into_iter()
                    .map(|(_, id)| id)
                    .filter(|id| position.contains_key(id))
                    .collect();
                membership.push((*root, *prefab, ids));
            }
            Err(failure) => log::warn!(
                "a prefab instance is written whole, not as a link: {}",
                failure.message
            ),
        }
    }
    // Outermost only: an instance inside another is part of its differences.
    let outer: Vec<&(PersistentId, AssetUUID, HashSet<PersistentId>)> = membership
        .iter()
        .filter(|(root, _, _)| {
            !membership
                .iter()
                .any(|(other, _, ids)| other != root && ids.contains(root))
        })
        .collect();

    let scene_children = children_of(&record.entities, &rows);
    let mut taken: HashSet<PersistentId> = HashSet::new();
    let mut instances = Vec::new();
    for (root, prefab, ids) in outer {
        // Expanded only to compare the instance against: nothing is loaded
        // from it, so what reading it adapted is not reported.
        let base = match prefab_under(*prefab, *root, &prefabs, &mut Stack::new()) {
            Ok((base, _)) => base,
            Err(failure) => {
                log::warn!(
                    "a prefab instance is written whole, not as a link: {}",
                    failure.message
                );
                continue;
            }
        };
        let mut now_rows: HashMap<PersistentId, Vec<(String, Record)>> = ids
            .iter()
            .filter_map(|id| rows.get(id).map(|components| (*id, components.clone())))
            .collect();
        if let Some(components) = now_rows.get_mut(root) {
            components.retain(|(name, _)| name != LINK);
        }
        let mut in_order: Vec<PersistentId> = ids.iter().copied().collect();
        in_order.sort_by_key(|id| position[id]);
        let now = record_from(in_order.clone(), now_rows, &[record]);
        let mut delta = delta_between(*prefab, &base, &now)?;

        // Sibling order is written only where it is not the prefab's (children
        // reordered or added by the author, a member moved under a scene
        // entity), so a prefab that reorders its own entities is followed.
        let base_children = children_of(&base.entities, &rows_of(&base));
        let mut parents: Vec<PersistentId> = in_order
            .iter()
            .filter(|id| *id != root)
            .filter_map(|id| rows.get(id).and_then(|components| parent_in(components)))
            .collect();
        parents.extend(in_order.iter().copied());
        let mut order = Vec::new();
        let mut done = HashSet::new();
        for parent in parents {
            if !done.insert(parent) {
                continue;
            }
            let Some(siblings) = scene_children.get(&parent) else {
                continue;
            };
            let as_prefab = siblings.iter().all(|id| ids.contains(id))
                && base_children.get(&parent).is_some_and(|prefab| {
                    prefab
                        .iter()
                        .filter(|id| siblings.contains(id))
                        .eq(siblings.iter())
                });
            if !as_prefab {
                order.extend(siblings.iter().copied());
            }
        }
        delta.order = order;

        taken.extend(ids.iter().copied());
        instances.push(InstanceRecord {
            root: *root,
            prefab: *prefab,
            delta,
        });
    }

    // The roots stay in the scene's order — where each instance is laid out
    // again — and lose their rows with every other member's.
    let roots_kept: HashSet<PersistentId> =
        instances.iter().map(|instance| instance.root).collect();
    let entities: Vec<PersistentId> = record
        .entities
        .iter()
        .filter(|id| roots_kept.contains(id) || !taken.contains(id))
        .copied()
        .collect();
    let plain: HashMap<PersistentId, Vec<(String, Record)>> = rows
        .into_iter()
        .filter(|(id, _)| !taken.contains(id))
        .collect();
    let mut collapsed = record_from(entities, plain, &[record]);
    collapsed.instances = instances;
    Ok(collapsed)
}

/// Brings `record` into `world` with fresh identities and returns them, in its order.
///
/// Plain entities and instance roots take new authored ids; members take the
/// ids derived from their instance's new root.
fn bring_fresh(
    world: &mut World,
    record: &SceneRecord,
    prefabs: &dyn PrefabSource,
) -> Result<Vec<PersistentId>, LoadFailure> {
    let prefabs = ReadOnce::new(prefabs);
    let mut map: HashMap<PersistentId, PersistentId> = record
        .entities
        .iter()
        .chain(record.pages.iter().flat_map(|page| page.rows.iter()))
        .chain(record.instances.iter().map(|instance| &instance.root))
        .map(|id| (*id, PersistentId::random_authored()))
        .collect();
    let fresh = moved_record(record, &mut map, &prefabs, &mut Stack::new())?;
    let expanded = expand(&fresh, &prefabs)?;
    apply(world, &expanded, Identity::Keep)?;
    Ok(fresh.entities)
}

/// Brings `prefab` into `world` as a new linked instance and returns its root.
///
/// The root takes a fresh authored id; members take ids derived from it.
///
/// # Errors
/// The prefab cannot be expanded; `world` is then left as it was.
pub fn instantiate_prefab(
    world: &mut World,
    prefab: AssetUUID,
    prefabs: &dyn PrefabSource,
) -> Result<EntityId, LoadFailure> {
    let root = PersistentId::random_authored();
    let expanded = expand(&one_instance(root, prefab), prefabs)?;
    apply(world, &expanded, Identity::Keep)?;
    world
        .entity_with_id(root)
        .ok_or_else(|| failed(format!("the prefab {prefab} brought no root")))
}

/// Returns a record holding only an unchanged instance of `prefab` rooted at `root`.
fn one_instance(root: PersistentId, prefab: AssetUUID) -> SceneRecord {
    SceneRecord {
        declared: Vec::new(),
        entities: vec![root],
        pages: Vec::new(),
        instances: vec![InstanceRecord {
            root,
            prefab,
            delta: SaveRecord {
                base: prefab,
                order: Vec::new(),
                destroyed: Vec::new(),
                created: Vec::new(),
                removed: Vec::new(),
                changes: SceneRecord::default(),
                before: SceneRecord::default(),
            },
        }],
    }
}

/// Brings `prefab` into `world` as a new linked instance rooted at the reserved `entity`.
///
/// The root takes a fresh created identity, its members ids within it.
/// Returns every entity of the instance, in the prefab's order.
///
/// # Errors
/// `entity` is not an id the world's reserver handed out and nothing committed,
/// or the prefab cannot be expanded. `world` is then left as it was.
pub fn instantiate_prefab_at(
    world: &mut World,
    entity: EntityId,
    prefab: AssetUUID,
    prefabs: &dyn PrefabSource,
) -> Result<Vec<EntityId>, LoadFailure> {
    world.settle_reservations();
    let reserved = world.nowhere_entity() != Some(entity)
        && !world.contains(entity)
        && world.reserver().is_handed(entity);
    if !reserved {
        return Err(failed(format!(
            "{entity:?} is no id this world reserved for a spawn"
        )));
    }
    // A created identity, as every entity the game brings into being has.
    let root = world.next_created_id();
    let expanded = expand(&one_instance(root, prefab), prefabs)?;
    let prepared = super::apply::prepare_at(world, &expanded, (root, entity))?;
    let made = prepared.reserved();
    prepared.commit(world, Identity::Keep);
    // Committed: no spawn may take it again, and the frame's end does not
    // give it back.
    world.reserver().take_handed(entity);
    Ok(made)
}

/// Returns `root` and everything under it as the bytes of a scene file.
///
/// A prefab instance inside is written as a link to its prefab, read from
/// `prefabs`, or whole when the prefab cannot be read.
///
/// # Errors
/// The subtree cannot be captured or collapsed, or the file cannot be encoded.
pub fn serialize_subtree(
    world: &World,
    root: EntityId,
    prefabs: &dyn PrefabSource,
) -> Result<Vec<u8>, SaveError> {
    let record = collapse(&capture_subtree(world, root)?, prefabs)?;
    write_scene_file(&record, &CompactEncoding)
        .map(|file| file.to_bytes())
        .map_err(|error| SaveError::Encoding(error.to_string()))
}

/// Does [`serialize_subtree`] for the file of the prefab `written`.
///
/// An instance of `written`, or of a prefab containing it, is written whole,
/// never as a link to the file it is written into.
///
/// # Errors
/// As [`serialize_subtree`].
pub fn serialize_prefab(
    world: &World,
    root: EntityId,
    written: AssetUUID,
    prefabs: &dyn PrefabSource,
) -> Result<Vec<u8>, SaveError> {
    serialize_subtree(
        world,
        root,
        &Except {
            source: prefabs,
            written,
        },
    )
}

/// Brings a subtree written by [`serialize_subtree`] into `world` and returns its root.
///
/// Plain entities and instance roots take new authored ids; an instance's
/// members take the ids derived from its new root.
///
/// # Errors
/// The bytes are not a scene file, its record does not read or cannot be
/// brought in, or it holds no entity. `world` is then left as it was.
pub fn instantiate_subtree(
    world: &mut World,
    bytes: &[u8],
    prefabs: &dyn PrefabSource,
) -> Result<EntityId, LoadFailure> {
    let file = SceneFile::from_bytes(bytes).map_err(|error| failed(format!("{error:?}")))?;
    let record = read_scene_file(&file).map_err(|error| failed(error.to_string()))?;
    let ids = bring_fresh(world, &record, prefabs)?;
    ids.first()
        .and_then(|root| world.entity_with_id(*root))
        .ok_or_else(|| failed("the prefab holds no entity".to_owned()))
}
