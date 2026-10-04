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
//! A prefab instance stays linked to its prefab: its root carries a
//! [`PrefabInstance`](crate::ecs::PrefabInstance), and each entity inside it
//! is known by the root's identity and its own id in the prefab
//! ([`PersistentId::within`]). A record keeps an instance collapsed — the
//! link and its differences — and a load expands it from the prefab as it is
//! now: a prefab edit reaches every field an instance did not override.
//!
//! # Expanding, and what it is keyed by
//!
//! A prefab's record names its entities by its own ids; its root is the
//! instance root. Expanding an instance under a root `R` re-keys the prefab —
//! its root becomes `R`, every other entity `x` becomes `within(R, x)` — then
//! expands the instances the prefab itself links to, under their re-keyed
//! roots, and merges the instance's differences on top, the way a game save
//! is merged into its scene.

use std::collections::{HashMap, HashSet};

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::scene::SceneFile;

use super::apply::{apply, Identity, LoadFailure};
use super::capture::{capture_subtree, SaveError};
use super::encoding::CompactEncoding;
use super::entity_refs::parent_in;
use super::file::{read_scene_file, write_scene_file};
use super::record::{to_record, EntityRef, LoadReport, Record, ReferenceWriter, ReportEntry};
use super::save::{components_of, compose_reporting, delta_between, owned, Pages, SaveRecord};
use super::scene_record::{InstanceRecord, SceneRecord};
use crate::ecs::{PrefabInstance, SerializablePrefabInstance, World};

mod overrides;
mod rekey;
mod siblings;
mod sources;

pub use overrides::{apply_to_prefab, instance_of, prefab_world, InstanceOf, PrefabApply};
use rekey::{members, moved_record, rekeyed};
use siblings::{children_of, restore_sibling_order};
use sources::{Except, ReadOnce};

/// Where the prefabs a record links to are read from.
pub trait PrefabSource {
    /// The record of the prefab known as `id`, as its file holds it — its own
    /// nested instances still collapsed — or why it cannot be had.
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

/// The component that links an instance root to its prefab, by its
/// registered name.
const LINK: &str = "PrefabInstance";

fn failed(message: String) -> LoadFailure {
    LoadFailure {
        message,
        report: LoadReport::default(),
    }
}

/// The prefabs being expanded, outermost first: one met again contains
/// itself.
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

/// The link component's value for `prefab`, as a record.
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

/// The prefab a link component's record names.
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

/// The prefab `prefab` expanded as an instance rooted at `root`, with no
/// difference of its own — and without the link on its root, which is the
/// instance's to carry.
fn prefab_under(
    prefab: AssetUUID,
    root: PersistentId,
    prefabs: &dyn PrefabSource,
    stack: &mut Stack,
) -> Result<SceneRecord, LoadFailure> {
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
    let (expanded, _) = expand_in(&keyed, prefabs, stack)?;
    stack.pop();
    let mut rows = rows_of(&expanded);
    if let Some(components) = rows.get_mut(&root) {
        components.retain(|(name, _)| name != LINK);
    }
    Ok(record_from(expanded.entities, rows))
}

/// Each entity's components in `record`, owned.
fn rows_of(record: &SceneRecord) -> HashMap<PersistentId, Vec<(String, Record)>> {
    components_of(record)
        .into_iter()
        .map(|(id, components)| (id, owned(&components)))
        .collect()
}

/// A record of `entities`, each with its components from `rows`.
fn record_from(
    entities: Vec<PersistentId>,
    mut rows: HashMap<PersistentId, Vec<(String, Record)>>,
) -> SceneRecord {
    let mut pages = Pages::default();
    for id in &entities {
        if let Some(components) = rows.remove(id) {
            pages.push(*id, components);
        }
    }
    SceneRecord {
        entities,
        pages: pages.pages,
        instances: Vec::new(),
    }
}

/// `record` with its instances expanded, and what the merges left out.
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
    for instance in &record.instances {
        let base = prefab_under(instance.prefab, instance.root, prefabs, stack)?;
        let (composed, left_out) = compose_reporting(&base, &instance.delta)?;
        report.extend(left_out);
        let mut members = rows_of(&composed);
        let root = members.entry(instance.root).or_default();
        root.retain(|(name, _)| name != LINK);
        root.push((LINK.to_owned(), link_to(instance.prefab)));
        rows.extend(members);
        laid_out.insert(instance.root, composed.entities);
    }

    // Each instance where its root stands; an instance whose layout names
    // another's root lays that one out there too.
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
    Ok((record_from(entities, rows), report))
}

/// `record` with every prefab instance it links to brought in from
/// `prefabs`: each instance's prefab, itself expanded, re-keyed under the
/// instance root, with the instance's differences on top. A missing prefab
/// or a prefab that contains itself refuses the whole record.
pub fn expand(
    record: &SceneRecord,
    prefabs: &dyn PrefabSource,
) -> Result<SceneRecord, LoadFailure> {
    expand_reporting(record, prefabs).map(|(record, _)| record)
}

/// [`expand`], and what the merges left out: what an instance changed that
/// its prefab no longer has — the prefab's removal wins.
pub fn expand_reporting(
    record: &SceneRecord,
    prefabs: &dyn PrefabSource,
) -> Result<(SceneRecord, Vec<ReportEntry>), LoadFailure> {
    expand_in(record, &ReadOnce::new(prefabs), &mut Stack::new())
}

/// `record` with every prefab instance not inside another one written as a
/// link to its prefab and its differences from it; nested instances are part
/// of their outer instance's differences.
///
/// An instance whose prefab cannot be read stays expanded — written whole,
/// its link still on its root — rather than lost: a later save that can read
/// the prefab links it again.
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
        let base = match prefab_under(*prefab, *root, &prefabs, &mut Stack::new()) {
            Ok(base) => base,
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
        let now = record_from(in_order.clone(), now_rows);
        let mut delta = delta_between(*prefab, &base, &now)?;

        // The order of siblings, where it is not the prefab's: a member's
        // children the author reordered or added to, the children of a scene
        // entity a member was moved under. Elsewhere none is written, and a
        // prefab that reorders its own entities is followed.
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
    let mut collapsed = record_from(entities, plain);
    collapsed.instances = instances;
    Ok(collapsed)
}

/// Brings `record` into `world` with fresh identities: its plain entities and
/// instance roots take new authored ids, the members of each instance the
/// ones derived from its new root. Returns the new id of each of the record's
/// entities, in order.
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

/// Brings the prefab known as `prefab` into `world` as a new linked
/// instance — a fresh authored root, members derived from it — and returns
/// its root. On failure `world` is left as it was.
pub fn instantiate_prefab(
    world: &mut World,
    prefab: AssetUUID,
    prefabs: &dyn PrefabSource,
) -> Result<EntityId, LoadFailure> {
    let root = PersistentId::random_authored();
    let record = SceneRecord {
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
    };
    let expanded = expand(&record, prefabs)?;
    apply(world, &expanded, Identity::Keep)?;
    world
        .entity_with_id(root)
        .ok_or_else(|| failed(format!("the prefab {prefab} brought no root")))
}

/// `root` and everything under it, as the bytes of a scene file. A prefab
/// instance inside the subtree is written as a link to its prefab, read from
/// `prefabs` — or whole, where its prefab cannot be read.
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

/// [`serialize_subtree`], for the file of the prefab known as `written`: an
/// instance of that very prefab — or of one that contains it — is written
/// whole, never as a link to the file it is written into.
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

/// Brings a subtree written by [`serialize_subtree`] into `world` as a new
/// instance and returns its root: plain entities and prefab instance roots
/// take new authored identities, the entities inside an instance the ones
/// derived from its new root, its prefab read from `prefabs`.
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
