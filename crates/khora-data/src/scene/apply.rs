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

//! Bringing a scene record into a world, all of it or none, a page at a time.
//!
//! See the book: concepts/serialization.md, "Atomic loading".

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;

use khora_core::ecs::ReservationHold;

use super::component_registration::{
    registration_named, ComponentRegistration, Kept, PendingComponent, StagedComponent,
};
use super::declared::{plan, DeclaredLoad};
use super::load_catalog::LoadCatalog;
use super::page_plan::{check_plans, PagePlan};
use super::record::{LoadReport, RecordError, ReferenceReader, ReportEntry, ReportKind};
use super::resolver::Resolver;
use super::retired::is_retired;
use super::scene_record::SceneRecord;
use super::session_only_read::SessionOnlyRead;
use crate::ecs::{ComponentKey, LoadedHierarchy, Parent, RuntimeComponentDecl, World};

/// Which identities the entities brought in take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Identity {
    /// The ones the record holds, a taken one replaced by a fresh identity of
    /// its kind: a scene being opened.
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
///
/// # Errors
///
/// As [`prepare`].
pub fn apply(
    world: &mut World,
    record: &SceneRecord,
    identity: Identity,
) -> Result<Applied, LoadFailure> {
    Ok(prepare_in_place(world, record, Kept::Scene)?.commit(world, identity))
}

/// A record read and checked, its entities reserved, nothing added yet.
///
/// Hold it only as long as it takes to decide: until it is committed or
/// abandoned, its ids are reserved in the world.
#[must_use = "a prepared load holds reserved ids until it is committed or abandoned"]
pub struct Prepared {
    /// Every recorded entity and the id reserved for it, in record order.
    pub(super) entities: Vec<(PersistentId, EntityId)>,
    /// The id reserved before the load, which giving up does not release.
    pub(super) given: Option<EntityId>,
    pub(super) pages: Vec<PreparedPage>,
    /// Declared components this world keeps in another domain than the file did.
    ///
    /// Attached once the pages are placed, each into its entity's page of the world's domain.
    pub(super) moved: Vec<(EntityId, ComponentKey, Box<dyn PendingComponent>)>,
    /// Each loaded child and its loaded parent, checked for cycles.
    ///
    /// A `Parent` naming an entity outside the record is not kept.
    pub(super) hierarchy: LoadedHierarchy,
    /// The declarations the file describes and the world does not have yet:
    /// registered when the load commits, never before.
    pub(super) register: Vec<RuntimeComponentDecl>,
    pub(super) report: LoadReport,
    /// What holds the reserved ids until the load commits them; dropped
    /// with the load, it gives them back.
    pub(super) hold: ReservationHold,
    /// The generation of the catalogue the load was prepared against.
    pub(super) generation: u64,
    /// What the load was read from, to prepare it again should the world's
    /// components change before it commits.
    ///
    /// `None` only for a load committed under the same borrow of the world it
    /// was prepared with ([`apply`], a prefab instance), which nothing can
    /// change in between.
    pub(super) source: Option<Source>,
}

impl Prepared {
    /// Returns the id reserved for each entity the load brings, in record order.
    pub(crate) fn reserved(&self) -> Vec<EntityId> {
        self.entities.iter().map(|(_, entity)| *entity).collect()
    }

    /// Returns the generation of the catalogue the load was prepared against.
    ///
    /// Older than [`World::load_catalog`]'s once the world's components changed.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Records what the load was read from, to prepare it again should its
    /// world's components change before it commits.
    pub(super) fn set_source(&mut self, source: Source) {
        self.source = Some(source);
    }

    /// Sets the declarations to register when the load commits.
    pub(super) fn set_register(&mut self, register: Vec<RuntimeComponentDecl>) {
        self.register = register;
    }
}

/// What a load was read from, shared rather than copied.
#[derive(Clone)]
pub(super) enum Source {
    /// A record, kept as it keeps its components.
    Record(Arc<SceneRecord>, Kept),
    /// A snapshot file.
    Snapshot(Arc<khora_core::scene::SceneFile>),
}

/// One page record, read: its signature and, per row, a value per column.
pub(super) struct PreparedPage {
    /// The page's components, sorted as CRPECS keys a page.
    pub(super) signature: Vec<ComponentKey>,
    /// Each row's entity and its values, one per signature component.
    pub(super) rows: Vec<(EntityId, StagedRow)>,
}

/// One row's values, staged: a value per component of its page.
pub(super) type StagedRow = Vec<(ComponentKey, Box<dyn PendingComponent>)>;

/// A component a load reads: a Rust one, through its registration, or one
/// declared while the engine runs, through the file's declaration.
#[derive(Clone)]
pub(super) enum Loaded {
    /// A Rust component.
    Rust(&'static ComponentRegistration),
    /// A component declared while the engine runs.
    Declared(Arc<DeclaredLoad>),
}

impl Loaded {
    /// Returns its name.
    pub(super) fn name(&self) -> &str {
        match self {
            Self::Rust(reg) => reg.type_name,
            Self::Declared(load) => &load.name,
        }
    }

    /// Returns its key in the world's pages.
    pub(super) fn key(&self) -> ComponentKey {
        match self {
            Self::Rust(reg) => ComponentKey::Rust(reg.type_id),
            Self::Declared(load) => load.key,
        }
    }
}

/// Plans every component the record declares, refusing one named like a
/// Rust component — whether a page holds it or not.
pub(super) fn plan_declared(
    world: &LoadCatalog,
    record: &SceneRecord,
    report: &LoadReport,
) -> Result<HashMap<String, Arc<DeclaredLoad>>, LoadFailure> {
    let mut loads = HashMap::with_capacity(record.declared.len());
    for declared in &record.declared {
        let load = plan(world, declared).map_err(|message| failure(message, report.clone()))?;
        if loads
            .insert(declared.name.clone(), Arc::new(load))
            .is_some()
        {
            return Err(failure(
                format!("the file declares `{}` twice", declared.name),
                report.clone(),
            ));
        }
    }
    Ok(loads)
}

/// Returns the declarations among `loads` the world does not have yet, of
/// the components `used` names: a declaration no page holds registers nothing.
pub(super) fn to_register<'n>(
    loads: &HashMap<String, Arc<DeclaredLoad>>,
    used: impl IntoIterator<Item = &'n str>,
) -> Vec<RuntimeComponentDecl> {
    let used: HashSet<&str> = used.into_iter().collect();
    let mut register: Vec<RuntimeComponentDecl> = loads
        .iter()
        .filter(|(name, _)| used.contains(name.as_str()))
        .filter_map(|(_, load)| load.register.clone())
        .collect();
    register.sort_by(|a, b| a.name.cmp(&b.name));
    register
}

/// Reads and checks `record` against `world`'s registrations, reserving an id
/// for each of its entities, without adding anything.
///
/// # Errors
///
/// When the record still links to prefabs (expand it first), a component name
/// is unknown, a file declaration is refused (named like a Rust component,
/// declared twice, or unreadable), the pages do not fit together, a value cannot
/// be read, or the hierarchy has a cycle. The failure carries the report so far.
pub fn prepare(
    world: &mut World,
    record: impl Into<Arc<SceneRecord>>,
) -> Result<Prepared, LoadFailure> {
    prepare_shared(world, record.into(), Kept::Scene)
}

/// Returns what a load reads of `world`, and a hold for the `wanted` ids it reserves.
///
/// The ids are made ready from slots despawns freed, so loads in a row with no
/// frame between them reuse the store rather than grow it.
pub(super) fn readings(world: &mut World, wanted: usize) -> (Arc<LoadCatalog>, ReservationHold) {
    world.ready_reservations(wanted);
    (world.load_catalog(), world.hold_reservations())
}

/// Prepares a load as [`prepare`] does, without the world: it reads `catalog`.
///
/// `hold` holds the ids it reserves.
///
/// Runs on any thread; only [`commit`](Prepared::commit) needs the world. The
/// ids stay held until the load is committed, abandoned or dropped.
///
/// # Errors
///
/// As [`prepare`].
pub fn prepare_with(
    catalog: &LoadCatalog,
    hold: ReservationHold,
    record: impl Into<Arc<SceneRecord>>,
) -> Result<Prepared, LoadFailure> {
    prepare_kept_with(catalog, hold, record.into(), Kept::Scene)
}

/// Prepares a load as [`prepare`] does, landing the entity known as `given.0` at `given.1`.
///
/// `given.1` is an id reserved before the load, which the load neither reserves nor gives
/// back. Like [`prepare_in_place`], it keeps no source: it is committed before the borrow
/// of `world` ends.
pub(crate) fn prepare_at(
    world: &mut World,
    record: &SceneRecord,
    given: (PersistentId, EntityId),
) -> Result<Prepared, LoadFailure> {
    let (catalog, hold) = readings(world, record.entities.len());
    prepare_given(
        &catalog,
        hold,
        record,
        Kept::Scene,
        Some(given),
        SessionOnlyRead::Refused,
    )
}

/// Prepares a load as [`prepare`] does, keeping only the components `kept` keeps.
///
/// Any other component the record holds is skipped and reported. The load
/// shares `record`, to be prepared again from it should the world's
/// components change before it commits.
pub(super) fn prepare_shared(
    world: &mut World,
    record: Arc<SceneRecord>,
    kept: Kept,
) -> Result<Prepared, LoadFailure> {
    let (catalog, hold) = readings(world, record.entities.len());
    prepare_kept_with(&catalog, hold, record, kept)
}

/// Prepares a load as [`prepare_shared`] does, without the world.
pub(super) fn prepare_kept_with(
    catalog: &LoadCatalog,
    hold: ReservationHold,
    record: Arc<SceneRecord>,
    kept: Kept,
) -> Result<Prepared, LoadFailure> {
    let mut prepared = prepare_given(catalog, hold, &record, kept, None, SessionOnlyRead::Refused)?;
    prepared.set_source(Source::Record(record, kept));
    Ok(prepared)
}

/// Prepares a load as [`prepare_shared`] does, keeping no source: for a load
/// committed before the borrow of `world` ends, which nothing can make stale.
pub(super) fn prepare_in_place(
    world: &mut World,
    record: &SceneRecord,
    kept: Kept,
) -> Result<Prepared, LoadFailure> {
    let (catalog, hold) = readings(world, record.entities.len());
    prepare_given(&catalog, hold, record, kept, None, SessionOnlyRead::Refused)
}

/// Prepares a load of `record`, the entity known as `given.0` landing at
/// `given.1` when given, a value only a session holds read as `session_only` says.
pub(super) fn prepare_given(
    world: &LoadCatalog,
    hold: ReservationHold,
    record: &SceneRecord,
    kept: Kept,
    given: Option<(PersistentId, EntityId)>,
    session_only: SessionOnlyRead,
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

    // The components the file describes itself, read before any page names
    // one: what they are decides how their columns are read.
    let declared = plan_declared(world, record, &report)?;

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
                // Derived or runtime state is rebuilt, never read: a recorded
                // copy could only disagree with its source. Skipped, reported.
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
                    columns.push((index, Loaded::Rust(reg)));
                }
                // Runtime state a scene does not keep — a behavior's state
                // companion — is skipped, said so; its declaration was read.
                None if declared
                    .get(name.as_str())
                    .is_some_and(|load| load.resumable && kept == Kept::Scene) =>
                {
                    for id in &page.rows {
                        report.entries.push(ReportEntry {
                            entity: Some(*id),
                            component: Some(name.clone()),
                            path: String::new(),
                            kind: ReportKind::NotSaved,
                        });
                    }
                }
                None if declared.contains_key(name) => {
                    columns.push((index, Loaded::Declared(Arc::clone(&declared[name]))));
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
                // A name nothing registers, declares or retires fails the whole load.
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

    let mut prepared = prepare_pages(
        world,
        hold,
        &record.entities,
        given,
        plans,
        report,
        session_only,
        |loaded, page, column, row, references| {
            let value = &record.pages[page].columns[column][row];
            match loaded {
                Loaded::Rust(reg) => (reg.stage)(value, references),
                Loaded::Declared(load) => load.stage_record(value, references),
            }
        },
    )?;
    // A component the load skipped registers nothing.
    let used = record
        .pages
        .iter()
        .flat_map(|page| page.components.iter().map(String::as_str))
        .filter(|name| {
            !declared
                .get(*name)
                .is_some_and(|load| load.resumable && kept == Kept::Scene)
        });
    prepared.set_register(to_register(&declared, used));
    Ok(prepared)
}

/// Prepares a load whose names are resolved, whatever its values are held as.
///
/// `stage` reads one value, given its component, page, file column and row.
/// A value only a session holds is kept or refused as `session_only` says; a
/// row refused one is placed without it. On any failure, nothing stays reserved.
#[expect(
    clippy::too_many_arguments,
    reason = "a record load and a snapshot load differ in these inputs and in how they stage"
)]
pub(super) fn prepare_pages(
    world: &LoadCatalog,
    hold: ReservationHold,
    entities: &[PersistentId],
    given: Option<(PersistentId, EntityId)>,
    plans: Vec<PagePlan<'_>>,
    mut report: LoadReport,
    session_only: SessionOnlyRead,
    mut stage: impl FnMut(
        &Loaded,
        usize,
        usize,
        usize,
        &mut dyn ReferenceReader,
    ) -> Result<StagedComponent, RecordError>,
) -> Result<Prepared, LoadFailure> {
    if let Err(message) = check_plans(world, entities, &plans) {
        return Err(failure(message, report));
    }

    let reserved: HashMap<PersistentId, EntityId> = entities
        .iter()
        .map(|id| match given {
            Some((known, entity)) if known == *id => (*id, entity),
            _ => (*id, hold.reserve()),
        })
        .collect();
    let given = given.map(|(_, entity)| entity);
    let mut references = Resolver {
        nowhere: world.nowhere(),
        _world: std::marker::PhantomData,
        reserved: &reserved,
        current: None,
        bound: None,
        entries: Vec::new(),
    };

    let parent_key = ComponentKey::of::<Parent>();
    let mut pages = Vec::with_capacity(plans.len());
    let mut moved = Vec::new();
    let mut parents: HashMap<EntityId, EntityId> = HashMap::new();
    let mut error = None;
    'read: for (page, plan) in plans.iter().enumerate() {
        // A declared component this world keeps in another domain than the
        // file did leaves the page: it joins its entity's page of the
        // world's domain once the pages are placed.
        let elsewhere = |loaded: &Loaded| match loaded {
            Loaded::Declared(load) => load.domain != load.file_domain,
            Loaded::Rust(_) => false,
        };
        let mut signature: Vec<ComponentKey> = plan
            .columns
            .iter()
            .filter(|(_, loaded)| !elsewhere(loaded))
            .map(|(_, loaded)| loaded.key())
            .collect();
        signature.sort();
        let mut rows = Vec::with_capacity(plan.rows.len());
        // The rows a refused value left with fewer components, by what they hold.
        let mut narrowed: HashMap<Vec<ComponentKey>, Vec<(EntityId, StagedRow)>> = HashMap::new();
        for (row, id) in plan.rows.iter().enumerate() {
            let mut values = Vec::with_capacity(plan.columns.len());
            let mut refused = false;
            for (column, loaded) in &plan.columns {
                references.current = Some((*id, loaded.clone()));
                references.bound = None;
                match stage(loaded, page, *column, row, &mut references) {
                    Ok(read) if session_only.refuses(loaded, &read) => {
                        report.entries.push(ReportEntry {
                            entity: Some(*id),
                            component: Some(loaded.name().to_owned()),
                            path: String::new(),
                            kind: ReportKind::Dropped,
                        });
                        refused = true;
                    }
                    Ok(read) => {
                        // A `Parent` names the entity it was read as: the
                        // one the resolver bound, if the load brings it.
                        if loaded.key() == parent_key {
                            if let Some(parent) = references.bound {
                                parents.insert(reserved[id], parent);
                            }
                        }
                        if elsewhere(loaded) {
                            moved.push((reserved[id], loaded.key(), read.component));
                        } else {
                            values.push((loaded.key(), read.component));
                        }
                        report
                            .entries
                            .extend(read.report.into_iter().map(|entry| ReportEntry {
                                entity: Some(*id),
                                component: Some(loaded.name().to_owned()),
                                ..entry
                            }));
                    }
                    Err(failed) => {
                        error = Some(format!(
                            "entity {:#x}, component `{}`: {failed}",
                            id.to_bits(),
                            loaded.name()
                        ));
                        break 'read;
                    }
                }
            }
            if refused {
                let mut keys: Vec<ComponentKey> = values.iter().map(|(key, _)| *key).collect();
                keys.sort();
                narrowed
                    .entry(keys)
                    .or_default()
                    .push((reserved[id], values));
            } else {
                rows.push((reserved[id], values));
            }
        }
        pages.push(PreparedPage { signature, rows });
        pages.extend(
            narrowed
                .into_iter()
                .map(|(signature, rows)| PreparedPage { signature, rows }),
        );
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
        // The hold, dropped here, gives every id it reserved back.
        let message = error.unwrap_or_default();
        return Err(failure(message, report));
    };
    Ok(Prepared {
        entities: entities.iter().map(|id| (*id, reserved[id])).collect(),
        given,
        pages,
        moved,
        hierarchy,
        register: Vec::new(),
        report,
        hold,
        generation: world.generation(),
        source: None,
    })
}

pub(super) fn failure(message: String, report: LoadReport) -> LoadFailure {
    LoadFailure { message, report }
}
