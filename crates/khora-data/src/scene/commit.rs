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

//! Committing a prepared load: everything it read added at once, under the
//! identities it keeps — or nothing, when it was prepared for another world
//! or fails when prepared again against components changed since.

use std::collections::HashSet;

use khora_core::ecs::entity::EntityId;

use super::apply::{prepare_kept, Applied, Identity, LoadFailure, Prepared, Source};
use super::record::{LoadReport, ReportEntry, ReportKind};
use crate::ecs::World;

impl Prepared {
    /// Whether this load was prepared for `world`: its ids are held by
    /// `world`'s reserver. A load prepared for another world adds nothing.
    pub fn is_for(&self, world: &World) -> bool {
        self.hold.is_over(&world.reserver())
    }

    /// Whether `world`'s components are those this load was prepared
    /// against: a load prepared off the frame before they changed is
    /// prepared again as it commits.
    pub fn is_current(&self, world: &World) -> bool {
        self.generation == world.components().generation()
    }

    /// The hold this load's ids are held by, its ids ready to reserve again:
    /// what a load prepared again reuses, so it takes no new slots.
    pub fn into_hold(self) -> khora_core::ecs::ReservationHold {
        self.hold.recycle();
        self.hold
    }

    /// Adds everything, giving the entities `identity`'s identities: each
    /// page built row by row, then the hierarchy rebuilt from each entity's
    /// `Parent`.
    pub fn commit(self, world: &mut World, identity: Identity) -> Applied {
        // Ids another world's reserver holds name nothing here — maybe this
        // world's own live entities: the load adds nothing.
        if !self.hold.is_over(&world.reserver()) {
            let reason = "it was prepared for another world".to_owned();
            log::error!(
                "a load was committed into a world it was not prepared for; nothing is added"
            );
            return not_loaded(self.report, reason);
        }
        // Prepared off the frame against components the world has changed
        // since: prepared again against the world as it is now, so it lands
        // as a load prepared now would.
        if self.generation != world.components().generation() {
            if let Some(source) = self.source.clone() {
                drop(self);
                return match source.prepare_now(world) {
                    Ok(prepared) => prepared.commit(world, identity),
                    Err(failure) => {
                        log::error!(
                            "a load prepared before its components changed fails now: {failure}"
                        );
                        not_loaded(failure.report, failure.message)
                    }
                };
            }
        }
        // What the file describes and the world did not have: registered
        // first, so the pages holding it can be built.
        for declaration in self.register {
            let name = declaration.name.clone();
            if let Err(error) = world.register_runtime_component(declaration) {
                log::error!("the file's `{name}` could not be registered: {error}");
            }
        }
        let placed: HashSet<EntityId> = self
            .pages
            .iter()
            .filter(|page| !page.signature.is_empty())
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
            let empty = !placed.contains(&entity);
            if Some(entity) == self.given {
                world.bring_reserved_to_life(entity, empty);
            } else if !world.commit_held(&self.hold, entity, empty) {
                log::error!("a loaded entity's id {entity:?} was not held for the load");
            }
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
            if page.signature.is_empty() {
                continue;
            }
            for (entity, values) in page.rows {
                let placed =
                    world.place_loaded_row(entity, &page.signature, &mut hierarchy, |target| {
                        for (key, value) in values {
                            if let Some(column) = target.columns.get_mut(&key) {
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
        for (entity, key, value) in self.moved {
            if let Err(error) = value.commit(world, entity) {
                log::error!("a loaded {key:?} could not join {entity:?}: {error}");
            }
        }
        Applied {
            entities: self.entities,
            report: self.report,
        }
    }

    /// Gives every reserved id back, adding nothing: the hold, dropped with
    /// the load, gives back the ids it holds; the id given before the load
    /// stays its caller's.
    pub fn abandon(self, world: &mut World) {
        let _ = world;
    }
}

impl Source {
    /// The load prepared again from what it was read from, against `world`
    /// as it is now.
    fn prepare_now(&self, world: &mut World) -> Result<Prepared, LoadFailure> {
        match self {
            Self::Record(record, kept) => prepare_kept(world, record, *kept),
            Self::Snapshot(file) => super::snapshot::prepare_snapshot(world, file),
        }
    }
}

/// What a load that could not land at all gives back: no entity, and its
/// report saying why, beside what it found before.
fn not_loaded(mut report: LoadReport, reason: String) -> Applied {
    report.entries.push(ReportEntry {
        entity: None,
        component: None,
        path: String::new(),
        kind: ReportKind::NotLoaded { reason },
    });
    Applied {
        entities: Vec::new(),
        report,
    }
}
