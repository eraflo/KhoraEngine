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

//! Bringing a scene record into a world — all of it, or none of it, a page at
//! a time.
//!
//! A load reads everything before it changes anything: every component name
//! is checked, an entity id is reserved for every recorded entity, and every
//! value is read into a staged component with its references pointing at the
//! reserved ids. Only once all of that succeeded is anything added — so a
//! file that cannot be loaded leaves the world exactly as it was, instead of
//! half-populated.
//!
//! Adding is page by page, the unit CRPECS stores data in: each page record
//! becomes rows of the page for its signature, every value pushed straight
//! into its column. No entity is migrated from page to page on the way, so a
//! load leaves no orphan row behind for compaction.

use std::any::TypeId;

use crate::ecs::ComponentKey;
use std::collections::{HashMap, HashSet};

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;

use super::component_registration::{
    registration_named, ComponentRegistration, Kept, Staged, StagedComponent,
};
use super::record::{EntityRef, LoadReport, RecordError, ReferenceReader, ReportEntry, ReportKind};
use super::retired::is_retired;
use super::scene_record::SceneRecord;
use crate::ecs::{LoadedHierarchy, Parent, SemanticDomain, World};

/// Which identities the entities brought in take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Identity {
    /// The ones the record holds, wherever the world does not already use
    /// them — a taken one is replaced by a fresh identity of its kind: a scene
    /// being opened.
    Keep,
    /// New authored ones: a prefab instance, a duplicate.
    Fresh,
}

/// What a successful load brought in.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// Each recorded entity, and the entity it became.
    pub entities: Vec<(PersistentId, EntityId)>,
    /// What the load adapted.
    pub report: LoadReport,
}

/// Why a load brought nothing in.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadFailure {
    /// What went wrong.
    pub message: String,
    /// What the load had adapted before it stopped.
    pub report: LoadReport,
}

impl std::fmt::Display for LoadFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LoadFailure {}

/// Brings `record` into `world`, atomically: on failure the world is left
/// exactly as it was.
pub fn apply(
    world: &mut World,
    record: &SceneRecord,
    identity: Identity,
) -> Result<Applied, LoadFailure> {
    Ok(prepare(world, record)?.commit(world, identity))
}

/// A record read and checked, its entities reserved, nothing added yet.
///
/// Hold it only as long as it takes to decide: until it is committed or
/// abandoned, its ids are reserved in the world.
#[must_use = "a prepared load holds reserved ids until it is committed or abandoned"]
pub struct Prepared {
    /// Every recorded entity and the id reserved for it, in record order.
    entities: Vec<(PersistentId, EntityId)>,
    pages: Vec<PreparedPage>,
    /// The hierarchy the record brings: each loaded child and its loaded
    /// parent, checked for cycles. A `Parent` naming anything else — gone,
    /// outside the record — is not kept.
    hierarchy: LoadedHierarchy,
    report: LoadReport,
}

/// One page record, read: its signature and, per row, a value per column.
struct PreparedPage {
    /// The page's component types, sorted as CRPECS keys a page.
    signature: Vec<TypeId>,
    /// Each row's entity and its values, one per signature type.
    rows: Vec<(EntityId, StagedRow)>,
}

/// One row's values, staged: a value per component type of its page.
type StagedRow = Vec<(TypeId, Box<dyn StagedComponent>)>;

/// Reads and checks `record` against `world`'s registrations, reserving an id
/// for each of its entities, without adding anything.
pub fn prepare(world: &mut World, record: &SceneRecord) -> Result<Prepared, LoadFailure> {
    prepare_kept(world, record, Kept::Scene)
}

/// [`prepare`], keeping the components `kept` keeps and skipping, reported,
/// any other the record holds.
pub(super) fn prepare_kept(
    world: &mut World,
    record: &SceneRecord,
    kept: Kept,
) -> Result<Prepared, LoadFailure> {
    let mut report = LoadReport::default();

    // A record still linking to prefabs is not a world yet: its instances'
    // entities are in the prefabs. Expanding it is the caller's to do.
    if !record.instances.is_empty() {
        return Err(failure(
            "the record links to prefabs — expand it before loading".to_owned(),
            report,
        ));
    }

    // Names first: nothing is reserved for a file that names a type nobody
    // has, or whose pages do not fit together.
    let mut plans = Vec::with_capacity(record.pages.len());
    for page in &record.pages {
        if page.columns.len() != page.components.len() {
            return Err(failure(
                format!(
                    "a page lists {} components but holds {} columns",
                    page.components.len(),
                    page.columns.len()
                ),
                report,
            ));
        }
        let mut columns = Vec::with_capacity(page.components.len());
        for (index, (name, column)) in page.components.iter().zip(&page.columns).enumerate() {
            if column.len() != page.rows.len() {
                return Err(failure(
                    format!(
                        "the `{name}` column holds {} values for {} rows",
                        column.len(),
                        page.rows.len()
                    ),
                    report,
                ));
            }
            match registration_named(name) {
                // What the engine derives or keeps while running is rebuilt,
                // never read: a recorded copy could only disagree with what
                // it comes from. A save holding one — written before the
                // component stopped being saved, or by a tool — loses
                // nothing by skipping it, and says so.
                Some(reg) if !kept.keeps(reg) => {
                    for id in &page.rows {
                        report.entries.push(ReportEntry {
                            entity: Some(*id),
                            component: Some(reg.type_name.to_owned()),
                            path: String::new(),
                            kind: ReportKind::NotSaved,
                        });
                    }
                }
                Some(reg) => {
                    if reg.type_name != name {
                        for id in &page.rows {
                            report.entries.push(ReportEntry {
                                entity: Some(*id),
                                component: Some(reg.type_name.to_owned()),
                                path: String::new(),
                                kind: ReportKind::Renamed { from: name.clone() },
                            });
                        }
                    }
                    columns.push((index, reg));
                }
                None if is_retired(name) => {
                    for id in &page.rows {
                        report.entries.push(ReportEntry {
                            entity: Some(*id),
                            component: Some(name.clone()),
                            path: String::new(),
                            kind: ReportKind::Retired,
                        });
                    }
                }
                None => {
                    let named = page.rows.first().map_or_else(
                        || "a page".to_owned(),
                        |id| format!("entity {:#x}", id.to_bits()),
                    );
                    return Err(failure(
                        format!("{named}: unknown component type `{name}`"),
                        report,
                    ));
                }
            }
        }
        plans.push(PagePlan {
            rows: &page.rows,
            columns,
        });
    }

    prepare_pages(
        world,
        &record.entities,
        plans,
        report,
        |reg, page, column, row, references| {
            (reg.stage)(&record.pages[page].columns[column][row], references)
        },
    )
}

/// One page of a load, its names resolved: its rows' identities and, for
/// each column it keeps, the column's position in the file and its
/// registration.
pub(super) struct PagePlan<'r> {
    pub(super) rows: &'r [PersistentId],
    pub(super) columns: Vec<(usize, &'static ComponentRegistration)>,
}

/// What every load does once its names are resolved, whatever the file
/// held its values as: checks that the pages fit together, reserves an id
/// per entity, stages every value through `stage` — given the registration,
/// the page, the column's position in the file and the row — and reads the
/// hierarchy. On any failure, nothing stays reserved.
pub(super) fn prepare_pages(
    world: &mut World,
    entities: &[PersistentId],
    plans: Vec<PagePlan<'_>>,
    mut report: LoadReport,
    mut stage: impl FnMut(
        &'static ComponentRegistration,
        usize,
        usize,
        usize,
        &mut dyn ReferenceReader,
    ) -> Result<Staged, RecordError>,
) -> Result<Prepared, LoadFailure> {
    let mut known = HashSet::new();
    for id in entities {
        if !known.insert(*id) {
            return Err(failure(
                format!("entity {:#x} is recorded twice", id.to_bits()),
                report,
            ));
        }
    }

    let mut domains_of: HashMap<PersistentId, Vec<SemanticDomain>> = HashMap::new();
    for plan in &plans {
        let mut page_domains: Vec<SemanticDomain> = Vec::new();
        for (index, (_, reg)) in plan.columns.iter().enumerate() {
            let Some(domain) = world.component_domain(reg.type_id) else {
                return Err(failure(
                    format!(
                        "component `{}` is not registered in this world",
                        reg.type_name
                    ),
                    report,
                ));
            };
            if plan.columns[..index]
                .iter()
                .any(|(_, known)| known.type_id == reg.type_id)
            {
                return Err(failure(
                    format!("component `{}` appears twice in one page", reg.type_name),
                    report,
                ));
            }
            if !page_domains.contains(&domain) {
                page_domains.push(domain);
            }
        }
        let mut in_page = HashSet::new();
        for id in plan.rows {
            if !known.contains(id) {
                return Err(failure(
                    format!(
                        "a page holds entity {:#x}, which the record does not list",
                        id.to_bits()
                    ),
                    report,
                ));
            }
            if !in_page.insert(*id) {
                return Err(failure(
                    format!("entity {:#x} has two rows in one page", id.to_bits()),
                    report,
                ));
            }
            let taken = domains_of.entry(*id).or_default();
            if page_domains.iter().any(|domain| taken.contains(domain)) {
                return Err(failure(
                    format!(
                        "entity {:#x} is in two pages of the same domain",
                        id.to_bits()
                    ),
                    report,
                ));
            }
            taken.extend(page_domains.iter().copied());
        }
    }

    let reserved: HashMap<PersistentId, EntityId> = entities
        .iter()
        .map(|id| (*id, world.reserve_entity()))
        .collect();
    let mut references = Resolver {
        world,
        reserved: &reserved,
        current: None,
        bound: None,
        entries: Vec::new(),
    };

    let parent_type = TypeId::of::<Parent>();
    let mut pages = Vec::with_capacity(plans.len());
    let mut parents: HashMap<EntityId, EntityId> = HashMap::new();
    let mut error = None;
    'read: for (page, plan) in plans.iter().enumerate() {
        let mut signature: Vec<TypeId> = plan.columns.iter().map(|(_, reg)| reg.type_id).collect();
        signature.sort();
        let mut rows = Vec::with_capacity(plan.rows.len());
        for (row, id) in plan.rows.iter().enumerate() {
            let mut values = Vec::with_capacity(plan.columns.len());
            for &(column, reg) in &plan.columns {
                references.current = Some((*id, reg.type_name));
                references.bound = None;
                match stage(reg, page, column, row, &mut references) {
                    Ok(read) => {
                        // A `Parent` names the entity it was read as: the
                        // one the resolver bound, if the load brings it.
                        if reg.type_id == parent_type {
                            if let Some(parent) = references.bound {
                                parents.insert(reserved[id], parent);
                            }
                        }
                        values.push((reg.type_id, read.component));
                        report
                            .entries
                            .extend(read.report.into_iter().map(|entry| ReportEntry {
                                entity: Some(*id),
                                component: Some(reg.type_name.to_owned()),
                                ..entry
                            }));
                    }
                    Err(failed) => {
                        error = Some(format!(
                            "entity {:#x}, component `{}`: {failed}",
                            id.to_bits(),
                            reg.type_name
                        ));
                        break 'read;
                    }
                }
            }
            rows.push((reserved[id], values));
        }
        pages.push(PreparedPage { signature, rows });
    }
    report.entries.append(&mut references.entries);

    let hierarchy = match error {
        Some(_) => None,
        None => match LoadedHierarchy::new(parents, entities.iter().map(|id| reserved[id])) {
            Ok(hierarchy) => Some(hierarchy),
            Err(entity) => {
                let id = reserved
                    .iter()
                    .find(|(_, reserved)| **reserved == entity)
                    .map_or(0, |(id, _)| id.to_bits());
                error = Some(format!("entity {id:#x} is its own ancestor"));
                None
            }
        },
    };
    let (None, Some(hierarchy)) = (&error, hierarchy) else {
        let message = error.unwrap_or_default();
        for entity in reserved.values() {
            world.release_reserved(*entity);
        }
        return Err(failure(message, report));
    };
    Ok(Prepared {
        entities: entities.iter().map(|id| (*id, reserved[id])).collect(),
        pages,
        hierarchy,
        report,
    })
}

impl Prepared {
    /// Adds everything, giving the entities `identity`'s identities: each
    /// page built row by row, then the hierarchy rebuilt from each entity's
    /// `Parent`.
    pub fn commit(self, world: &mut World, identity: Identity) -> Applied {
        let placed: HashSet<EntityId> = self
            .pages
            .iter()
            .flat_map(|page| page.rows.iter().map(|(entity, _)| *entity))
            .collect();
        // Which saved identities are free is decided against the world as
        // it was before this load: only an entity the load did not bring can
        // make a newcomer give its identity up.
        let kept: Vec<bool> = self
            .entities
            .iter()
            .map(|&(id, _)| identity == Identity::Keep && world.entity_with_id(id).is_none())
            .collect();
        for &(_, entity) in &self.entities {
            world.bring_reserved_to_life(entity, !placed.contains(&entity));
        }
        for (&(id, entity), &kept) in self.entities.iter().zip(&kept) {
            if kept {
                world.set_persistent_id(entity, id);
            }
        }
        // The rest take a fresh identity of their kind; under `Fresh`, every
        // entity is a new authored one.
        for (&(id, entity), &kept) in self.entities.iter().zip(&kept) {
            if kept {
                continue;
            }
            if identity == Identity::Keep && id.is_created() {
                world.assign_created_id(entity);
            } else {
                world.mark_authored(entity);
            }
        }

        // `Parent` is recorded; `Children` is derived from it. The hierarchy
        // module writes both as the rows are placed.
        let mut hierarchy = self.hierarchy;
        for page in self.pages {
            for (entity, values) in page.rows {
                let placed =
                    world.place_loaded_row(entity, &page.signature, &mut hierarchy, |target| {
                        for (type_id, value) in values {
                            if let Some(column) =
                                target.columns.get_mut(&ComponentKey::Rust(type_id))
                            {
                                value.push_into(column.as_mut());
                            }
                        }
                    });
                if !placed {
                    log::error!("a loaded page row could not be placed for {entity:?}");
                }
            }
        }
        world.finish_loaded_hierarchy(hierarchy);
        Applied {
            entities: self.entities,
            report: self.report,
        }
    }

    /// Gives every reserved id back, adding nothing.
    pub fn abandon(self, world: &mut World) {
        for (_, entity) in self.entities {
            world.release_reserved(entity);
        }
    }
}

pub(super) fn failure(message: String, report: LoadReport) -> LoadFailure {
    LoadFailure { message, report }
}

/// Resolves the references a record holds to the ids reserved for this load.
struct Resolver<'w, 'r> {
    world: &'w mut World,
    reserved: &'r HashMap<PersistentId, EntityId>,
    /// The entity and component being read, for report entries.
    current: Option<(PersistentId, &'static str)>,
    /// The entity the last reference read was bound to, if the load brings
    /// it — what a `Parent` names.
    bound: Option<EntityId>,
    entries: Vec<ReportEntry>,
}

impl ReferenceReader for Resolver<'_, '_> {
    fn read_entity(&mut self, reference: EntityRef) -> Result<EntityId, RecordError> {
        if let EntityRef::Id(id) = reference {
            if let Some(entity) = self.reserved.get(&id) {
                self.bound = Some(*entity);
                return Ok(*entity);
            }
        }
        let (entity, component) = self.current.unzip();
        self.entries.push(ReportEntry {
            entity,
            component: component.map(str::to_owned),
            path: String::new(),
            kind: ReportKind::DeadReference,
        });
        Ok(self.world.nowhere())
    }
}
