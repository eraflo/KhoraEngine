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

//! Writing a world, or a subtree, down as a scene record: live rows, page by page, values by name.
//!
//! See the book: concepts/serialization.md, "Pages, persistent ids, provenance".

use std::any::TypeId;
use std::collections::{HashMap, HashSet};

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;

use super::component_registration::{ComponentRegistration, Kept};
use super::declared::{declaration_of, is_saved_declared, write_value};
use super::record::{EntityRef, RecordError, ReferenceWriter, ReportEntry, ReportKind};
use super::scene_record::{PageRecord, SceneRecord};
use crate::ecs::{AnyVec, Children, ComponentKey, ComponentVTable, Parent, World};

/// Why a world could not be written down.
#[derive(Debug, Clone, PartialEq)]
pub enum SaveError {
    /// A component's value could not be written as a record.
    Component {
        /// The component's type name.
        component: String,
        /// Why.
        error: RecordError,
    },
    /// The root of a subtree is not alive.
    NoSuchEntity(EntityId),
    /// The record could not be encoded.
    Encoding(String),
    /// A snapshot was asked of a component whose schema could not be traced
    /// in full: no fingerprint could tell its versions apart.
    Unguarded(String),
    /// The scene a game save is taken against could not be read.
    Base(String),
    /// A prefab an instance links to could not be read.
    Prefab(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Component { component, error } => write!(f, "`{component}`: {error}"),
            Self::NoSuchEntity(entity) => write!(f, "no entity {entity:?}"),
            Self::Encoding(error) => write!(f, "{error}"),
            Self::Base(error) => write!(f, "the scene the save is taken against: {error}"),
            Self::Prefab(error) => write!(f, "{error}"),
            Self::Unguarded(component) => write!(
                f,
                "`{component}` has a schema no fingerprint can guard: it cannot be snapshotted"
            ),
        }
    }
}

impl std::error::Error for SaveError {}

/// Returns a scene record of every entity of `world` but a system's own.
///
/// Entities are listed tree by tree, children in `Children` order: a load
/// rebuilds sibling order from the record's order.
///
/// # Errors
///
/// When a component's value cannot be written as a record.
pub fn capture_world(world: &World) -> Result<SceneRecord, SaveError> {
    capture_world_reporting(world).map(|(record, _)| record)
}

/// Returns a scene record as [`capture_world`] does, and what it left out.
///
/// A value only this session holds (a mesh made in code) is left out: its
/// entity is recorded without it, and it is reported
/// [`ReportKind::NotSaved`] on that entity.
///
/// # Errors
///
/// When a component's value cannot be written as a record.
pub fn capture_world_reporting(
    world: &World,
) -> Result<(SceneRecord, Vec<ReportEntry>), SaveError> {
    let mut left_out = Vec::new();
    let record = capture(
        world,
        &scene_entities(world),
        None,
        Kept::Scene,
        SessionValues::LeftOut(&mut left_out),
    )?;
    Ok((record, left_out))
}

/// Returns every entity a scene holds, in tree order: all `world`'s but the systems' own.
pub(super) fn scene_entities(world: &World) -> Vec<EntityId> {
    // A system's own entity is the engine's, made while the game runs: no
    // scene holds it — whatever its name is now. An authored entity holding a
    // system's component is still the author's.
    let engine_kept: HashSet<PersistentId> = world
        .components()
        .iter()
        .map(|vtable| crate::ecs::systems::script_systems::singleton_id(&vtable.name))
        .collect();
    world_in_tree_order(world)
        .into_iter()
        .filter(|entity| {
            world
                .persistent_id(*entity)
                .is_none_or(|id| !engine_kept.contains(&id))
        })
        .collect()
}

/// Returns a record of every entity of `world` in tree order, with what a game
/// save keeps: what a scene keeps, and the state play resumes from.
///
/// Each value only this session holds is left out, and returned reported `NotSaved`.
pub(super) fn capture_game_reporting(
    world: &World,
) -> Result<(SceneRecord, Vec<ReportEntry>), SaveError> {
    let mut left_out = Vec::new();
    let record = capture(
        world,
        &world_in_tree_order(world),
        None,
        Kept::Game,
        SessionValues::LeftOut(&mut left_out),
    )?;
    Ok((record, left_out))
}

/// Returns a record of what `kept` keeps of `world`, every value only this
/// session holds kept: a reading, never a save.
///
/// The entities of a scene for [`Kept::Scene`], every entity for
/// [`Kept::Game`].
pub(super) fn capture_keeping_session_values(
    world: &World,
    kept: Kept,
) -> Result<SceneRecord, SaveError> {
    let entities = match kept {
        Kept::Scene => scene_entities(world),
        Kept::Game => world_in_tree_order(world),
    };
    capture(world, &entities, None, kept, SessionValues::Kept)
}

/// Returns every entity of `world` tree by tree, children in `Children` order.
pub(super) fn world_in_tree_order(world: &World) -> Vec<EntityId> {
    let roots = world
        .iter_entities()
        .filter(|entity| {
            world
                .get::<Parent>(*entity)
                .is_none_or(|parent| !world.contains(parent.0))
        })
        .collect::<Vec<_>>();
    let mut seen = HashSet::new();
    let mut entities = descend(world, roots, &mut seen);
    // An entity no root reaches sits on a loop a damaged hierarchy made; it
    // is still saved, after the trees.
    entities.extend(world.iter_entities().filter(|entity| seen.insert(*entity)));
    entities
}

/// Returns a record of `root` and everything under it.
///
/// A reference outside the subtree is written as outside, and the root's own
/// parent is left out, so the subtree lands wherever it is instantiated. A
/// value only this session holds is left out; [`capture_subtree_reporting`]
/// says which.
///
/// # Errors
///
/// When `root` is not alive, or a component's value cannot be written as a record.
pub fn capture_subtree(world: &World, root: EntityId) -> Result<SceneRecord, SaveError> {
    if !world.contains(root) {
        return Err(SaveError::NoSuchEntity(root));
    }
    capture_subtree_reporting(world, root).map(|(record, _)| record)
}

/// Returns a record of `root`'s subtree as [`capture_subtree`] does, and what it left out.
///
/// A value only this session holds is recorded nowhere: its member is
/// recorded without it, and it is reported [`ReportKind::NotSaved`].
///
/// # Errors
///
/// As [`capture_subtree`].
pub fn capture_subtree_reporting(
    world: &World,
    root: EntityId,
) -> Result<(SceneRecord, Vec<ReportEntry>), SaveError> {
    if !world.contains(root) {
        return Err(SaveError::NoSuchEntity(root));
    }
    let entities = descend(world, vec![root], &mut HashSet::new());
    let mut left_out = Vec::new();
    let record = capture(
        world,
        &entities,
        Some(root),
        Kept::Scene,
        SessionValues::LeftOut(&mut left_out),
    )?;
    Ok((record, left_out))
}

/// Returns a record of `root`'s subtree to be copied within this session.
///
/// Captured as [`capture_subtree`] does, but a value only this session holds
/// is kept: the copy never leaves the session.
///
/// # Errors
///
/// As [`capture_subtree`].
pub(super) fn capture_subtree_in_session(
    world: &World,
    root: EntityId,
) -> Result<SceneRecord, SaveError> {
    if !world.contains(root) {
        return Err(SaveError::NoSuchEntity(root));
    }
    let entities = descend(world, vec![root], &mut HashSet::new());
    capture(
        world,
        &entities,
        Some(root),
        Kept::Scene,
        SessionValues::Kept,
    )
}

/// Returns `roots` and everything under them, breadth first, children in `Children` order.
///
/// Each entity once: a list repeating a child, or naming an ancestor, does not loop.
fn descend(world: &World, roots: Vec<EntityId>, seen: &mut HashSet<EntityId>) -> Vec<EntityId> {
    let mut entities: Vec<EntityId> = roots
        .into_iter()
        .filter(|root| seen.insert(*root))
        .collect();
    let mut next = 0;
    while next < entities.len() {
        if let Some(children) = world.get::<Children>(entities[next]) {
            for &child in &children.0 {
                if world.contains(child) && seen.insert(child) {
                    entities.push(child);
                }
            }
        }
        next += 1;
    }
    entities
}

/// Returns the registrations of the components `kept` keeps, by type.
fn kept_registrations(kept: Kept) -> HashMap<TypeId, &'static ComponentRegistration> {
    inventory::iter::<ComponentRegistration>
        .into_iter()
        .filter(|reg| kept.keeps(reg))
        .map(|reg| (reg.type_id, reg))
        .collect()
}

/// Returns a record of `entities`, each value only this session holds kept or
/// left out as `session` says.
fn capture(
    world: &World,
    entities: &[EntityId],
    root: Option<EntityId>,
    kept: Kept,
    session: SessionValues<'_>,
) -> Result<SceneRecord, SaveError> {
    let (ids, pages) = capture_pages(
        world,
        entities,
        root,
        kept,
        session,
        |saved, column, row, references| match saved {
            Saved::Rust(reg) => (reg.column_to_record)(column, row, references),
            Saved::Declared(vtable) => write_value(vtable, column, row, references),
        },
    )?;
    Ok(SceneRecord {
        entities: ids,
        instances: Vec::new(),
        declared: declarations_in(&pages),
        pages: pages
            .into_iter()
            .map(|page| PageRecord {
                components: page
                    .components
                    .iter()
                    .map(|saved| saved.name().to_owned())
                    .collect(),
                rows: page.rows,
                columns: page.columns,
            })
            .collect(),
    })
}

/// Returns the declaration of every declared component `pages` hold, sorted by name.
pub(super) fn declarations_in<V>(pages: &[CapturedPage<'_, V>]) -> Vec<super::DeclaredRecord> {
    let mut declared: Vec<&ComponentVTable> = pages
        .iter()
        .flat_map(|page| page.components.iter())
        .filter_map(|saved| match saved {
            Saved::Declared(vtable) => Some(*vtable),
            Saved::Rust(_) => None,
        })
        .collect();
    declared.sort_by(|a, b| a.name.cmp(&b.name));
    declared.dedup_by(|a, b| a.key == b.key);
    declared.into_iter().filter_map(declaration_of).collect()
}

/// A component a capture writes: a Rust one, through its registration, or
/// one declared while the engine runs, through its declaration.
#[derive(Clone, Copy)]
pub(super) enum Saved<'w> {
    /// A Rust component.
    Rust(&'static ComponentRegistration),
    /// A component declared while the engine runs.
    Declared(&'w ComponentVTable),
}

impl<'w> Saved<'w> {
    /// Returns its name.
    pub(super) fn name(&self) -> &'w str {
        match self {
            Self::Rust(reg) => reg.type_name,
            Self::Declared(vtable) => &vtable.name,
        }
    }

    /// Returns its key in the world's pages.
    pub(super) fn key(&self) -> ComponentKey {
        match self {
            Self::Rust(reg) => ComponentKey::Rust(reg.type_id),
            Self::Declared(vtable) => vtable.key,
        }
    }
}

/// What a capture does with a value only this session holds: see
/// [`ComponentRegistration::session_only`].
pub(super) enum SessionValues<'r> {
    /// Kept, as a snapshot read back in the same session keeps it.
    Kept,
    /// Left out of the record, each one reported `NotSaved` here.
    LeftOut(&'r mut Vec<ReportEntry>),
}

/// One page of a capture: its saved components, sorted by name, its rows'
/// identities, and a value per row in each component's column.
pub(super) struct CapturedPage<'w, V> {
    pub(super) components: Vec<Saved<'w>>,
    pub(super) rows: Vec<PersistentId>,
    pub(super) columns: Vec<Vec<V>>,
}

/// Writes each saved component of `entities`' live rows with `write`, one page per signature.
///
/// Returns the identities of `entities`, in order, and the pages. A value only
/// this session holds is kept or left out as `session` says.
pub(super) fn capture_pages<'w, V>(
    world: &'w World,
    entities: &[EntityId],
    root: Option<EntityId>,
    kept: Kept,
    mut session: SessionValues<'_>,
    mut write: impl FnMut(
        Saved<'w>,
        &dyn AnyVec,
        usize,
        &mut dyn ReferenceWriter,
    ) -> Result<V, RecordError>,
) -> Result<(Vec<PersistentId>, Vec<CapturedPage<'w, V>>), SaveError> {
    let mut inside = InsideOnly {
        ids: HashMap::with_capacity(entities.len()),
    };
    let mut ids = Vec::with_capacity(entities.len());
    for &entity in entities {
        let id = world
            .persistent_id(entity)
            .ok_or(SaveError::NoSuchEntity(entity))?;
        inside.ids.insert(entity, id);
        ids.push(id);
    }
    let captured: HashSet<EntityId> = entities.iter().copied().collect();

    let registrations = kept_registrations(kept);
    let parent = TypeId::of::<crate::ecs::Parent>();
    // Pages whose saved components are the same land in one page record:
    // the record holds what a load needs to build, one page per signature.
    let mut pages: Vec<CapturedPage<V>> = Vec::new();
    let mut by_signature: HashMap<Vec<&'w str>, usize> = HashMap::new();
    let registry = world.components();

    for (page_id, page) in world.storage.pages.iter().enumerate() {
        let mut saved: Vec<Saved<'w>> = page
            .keys
            .iter()
            .filter_map(|key| match key {
                ComponentKey::Rust(type_id) => registrations.get(type_id).copied().map(Saved::Rust),
                // Described by the record, beside its pages.
                ComponentKey::Declared(_) => registry
                    .vtable(*key)
                    .filter(|vtable| is_saved_declared(vtable, kept))
                    .map(Saved::Declared),
            })
            .collect();
        if saved.is_empty() {
            continue;
        }
        saved.sort_by_key(|saved| saved.name());

        for (row, &entity) in page.entities.iter().enumerate() {
            if !captured.contains(&entity) || !world.is_live_row_of(page_id as u32, row, entity) {
                continue;
            }
            // The subtree root's parent is outside the subtree: left out, so
            // the instance is placed wherever it is made.
            let mut columns: Vec<&Saved<'w>> = saved
                .iter()
                .filter(|saved| {
                    !(Some(entity) == root && saved.key() == ComponentKey::Rust(parent))
                })
                .collect();
            if let SessionValues::LeftOut(left_out) = &mut session {
                columns.retain(|saved| {
                    let Saved::Rust(reg) = saved else {
                        return true;
                    };
                    let session_only = page
                        .columns
                        .get(&saved.key())
                        .is_some_and(|column| reg.is_session_only(column.as_ref(), row));
                    if session_only {
                        left_out.push(ReportEntry {
                            entity: Some(inside.ids[&entity]),
                            component: Some(reg.type_name.to_owned()),
                            path: String::new(),
                            kind: ReportKind::NotSaved,
                        });
                    }
                    !session_only
                });
            }
            if columns.is_empty() {
                continue;
            }
            let signature: Vec<&'w str> = columns.iter().map(|saved| saved.name()).collect();
            let index = *by_signature.entry(signature).or_insert_with(|| {
                pages.push(CapturedPage {
                    components: columns.iter().map(|saved| **saved).collect(),
                    rows: Vec::new(),
                    columns: std::iter::repeat_with(Vec::new)
                        .take(columns.len())
                        .collect(),
                });
                pages.len() - 1
            });
            let record = &mut pages[index];
            record.rows.push(inside.ids[&entity]);
            for (slot, saved) in columns.iter().enumerate() {
                // In the signature, so in the columns: a page keeps both in step.
                let Some(column) = page.columns.get(&saved.key()) else {
                    return Err(SaveError::Component {
                        component: saved.name().to_owned(),
                        error: RecordError("its page has no column for it".to_owned()),
                    });
                };
                let value = write(**saved, column.as_ref(), row, &mut inside).map_err(|error| {
                    SaveError::Component {
                        component: saved.name().to_owned(),
                        error,
                    }
                })?;
                record.columns[slot].push(value);
            }
        }
    }

    Ok((ids, pages))
}

/// Writes an entity the record holds by its identity, any other as outside.
struct InsideOnly {
    ids: HashMap<EntityId, PersistentId>,
}

impl ReferenceWriter for InsideOnly {
    fn write_entity(&mut self, entity: EntityId) -> EntityRef {
        self.ids
            .get(&entity)
            .map_or(EntityRef::Outside, |id| EntityRef::Id(*id))
    }
}

#[cfg(test)]
mod tests {
    use khora_core::asset::{AssetUUID, ProceduralMesh};
    use khora_core::renderer::api::gpu_scene::Mesh;

    use super::*;
    use crate::ecs::{AssetRef, Name, Transform};

    /// A value only the session holds is left out of the record and reported
    /// `NotSaved` on its entity; a saved value of the same component is kept.
    #[test]
    fn capture_world_reporting_leaves_out_and_reports_a_session_only_value() {
        let mut world = World::new();
        let made = world.spawn((
            Transform::identity(),
            Name::new("Made"),
            AssetRef::<Mesh>::Made(AssetUUID::new()),
        ));
        let authored = world.spawn((
            Transform::identity(),
            AssetRef::<Mesh>::Local(ProceduralMesh::unit_cube()),
        ));
        let made_id = world.mark_authored(made).expect("authored");
        let authored_id = world.mark_authored(authored).expect("authored");

        let (record, left_out) = capture_world_reporting(&world).expect("captures");

        let holds_mesh_ref = |id: PersistentId| {
            record.pages.iter().any(|page| {
                page.rows.contains(&id)
                    && page.components.iter().any(|name| name == "AssetRef<Mesh>")
            })
        };
        assert!(
            !holds_mesh_ref(made_id),
            "the mesh made in code is recorded"
        );
        assert!(holds_mesh_ref(authored_id), "the authored mesh is left out");
        assert_eq!(
            left_out,
            [ReportEntry {
                entity: Some(made_id),
                component: Some("AssetRef<Mesh>".to_owned()),
                path: String::new(),
                kind: ReportKind::NotSaved,
            }],
            "only the mesh made in code is reported left out"
        );
    }

    /// A prefab captured from a subtree leaves out a value only the session
    /// holds, and reports it `NotSaved` on its member.
    #[test]
    fn capture_subtree_reporting_leaves_out_and_reports_a_session_only_value() {
        let mut world = World::new();
        let root = world.spawn((Transform::identity(), Name::new("Root")));
        let member = world.spawn((
            Transform::identity(),
            AssetRef::<Mesh>::Made(AssetUUID::new()),
        ));
        world.set_parent(member, Some(root));
        world.mark_authored(root).expect("authored");
        let member_id = world.mark_authored(member).expect("authored");

        let (record, left_out) = capture_subtree_reporting(&world, root).expect("captures");

        assert!(
            record.entities.contains(&member_id),
            "the member holding a mesh made in code is not captured"
        );
        assert!(
            !record
                .pages
                .iter()
                .any(|page| page.components.iter().any(|name| name == "AssetRef<Mesh>")),
            "the prefab holds the mesh made in code"
        );
        assert_eq!(
            left_out,
            [ReportEntry {
                entity: Some(member_id),
                component: Some("AssetRef<Mesh>".to_owned()),
                path: String::new(),
                kind: ReportKind::NotSaved,
            }]
        );
    }
}
