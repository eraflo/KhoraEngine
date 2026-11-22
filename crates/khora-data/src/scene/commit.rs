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

//! Committing a prepared load: everything it read added at once, or nothing.
//!
//! See the book: concepts/serialization.md, "Atomic loading".

use std::collections::HashSet;
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;

use super::apply::{prepare_shared, Applied, Identity, LoadFailure, Prepared, Source};
use super::record::{LoadReport, ReportEntry, ReportKind};
use crate::ecs::World;

impl Prepared {
    /// Returns whether this load was prepared for `world`, whose reserver holds its ids.
    ///
    /// A load committed into another world adds nothing.
    pub fn is_for(&self, world: &World) -> bool {
        self.hold.is_over(&world.reserver())
    }

    /// Returns whether `world`'s components are those this load was prepared against.
    ///
    /// A load that is not current is prepared again as it commits.
    pub fn is_current(&self, world: &World) -> bool {
        self.generation == world.components().generation()
    }

    /// Returns the hold of this load's ids, ready to reserve them again.
    ///
    /// A load prepared again reuses it, so it takes no new slots.
    pub(crate) fn into_hold(self) -> khora_core::ecs::ReservationHold {
        self.hold.recycle();
        self.hold
    }

    /// Adds everything the load read, giving the entities `identity`'s identities.
    ///
    /// A load whose world's components changed since it was prepared is
    /// prepared again from what it was read from, as it was first prepared
    /// (a scene, a game or a snapshot), and lands as a load prepared now would.
    ///
    /// Never fails: a load prepared for another world, or one that fails when
    /// prepared again against changed components, adds nothing and reports
    /// `NotLoaded`.
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
        // Prepared against components the world has changed since: prepared
        // again against the world as it is now, so it lands as a load
        // prepared now would.
        if self.generation != world.components().generation() {
            let Some(source) = self.source.clone() else {
                log::error!("a load kept no source and its components changed; nothing is added");
                let reason = "its components changed since it was prepared".to_owned();
                return not_loaded(self.report, reason);
            };
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

    /// Gives every reserved id back, adding nothing.
    ///
    /// An id the caller gave the load stays the caller's.
    pub fn abandon(self, world: &mut World) {
        let _ = world;
    }
}

impl Source {
    /// Prepares the load again from its source, against `world` as it is now.
    fn prepare_now(&self, world: &mut World) -> Result<Prepared, LoadFailure> {
        match self {
            Self::Record(record, kept) => prepare_shared(world, Arc::clone(record), *kept),
            Self::Snapshot(file) => super::snapshot::prepare_snapshot(world, Arc::clone(file)),
        }
    }
}

/// Returns what a load that could not land gives back: no entity, a `NotLoaded` entry saying why.
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

#[cfg(test)]
mod tests {
    use khora_core::math::Vec3;
    use khora_core::script::ScriptValue;

    use crate::ecs::{
        ComponentProvenance, FieldKind, PackedField, PackedLayout, RuntimeComponentDecl,
        SemanticDomain, Transform, World,
    };
    use crate::scene::apply::prepare_in_place;
    use crate::scene::component_registration::Kept;
    use crate::scene::record::ReportKind;
    use crate::scene::{capture_world, prepare, Identity, SceneRecord};

    /// A record of one authored entity holding a `Transform`.
    fn one_entity() -> SceneRecord {
        let mut src = World::new();
        let entity = src.spawn(Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)));
        src.mark_authored(entity).expect("authored");
        capture_world(&src).expect("captures")
    }

    /// Registers `Charge`, a declared component no record holds: the world's
    /// components change.
    fn register_charge(world: &mut World) {
        world
            .register_runtime_component(RuntimeComponentDecl {
                name: "Charge".to_owned(),
                domain: SemanticDomain::Spatial,
                provenance: ComponentProvenance::Authored,
                resumable: false,
                layout: PackedLayout::new(vec![PackedField {
                    name: "amount".to_owned(),
                    kind: FieldKind::Int,
                    default: ScriptValue::Int(0),
                }])
                .expect("a valid layout"),
            })
            .expect("`Charge` registers");
    }

    #[test]
    fn a_load_is_for_the_world_it_was_prepared_in_only() {
        let record = one_entity();
        let mut world = World::new();
        let other = World::new();
        let prepared = prepare(&mut world, record.clone()).expect("prepares");

        assert!(prepared.is_for(&world));
        assert!(!prepared.is_for(&other));
        prepared.abandon(&mut world);
    }

    #[test]
    fn a_load_stops_being_current_once_its_world_registers_a_component() {
        let record = one_entity();
        let mut world = World::new();
        let prepared = prepare(&mut world, record).expect("prepares");
        assert!(prepared.is_current(&world));

        register_charge(&mut world);

        assert!(!prepared.is_current(&world));
        prepared.abandon(&mut world);
    }

    #[test]
    fn a_prepared_load_is_prepared_again_when_its_components_changed() {
        let mut world = World::new();
        let prepared = prepare(&mut world, one_entity()).expect("prepares");
        register_charge(&mut world);

        let applied = prepared.commit(&mut world, Identity::Keep);

        assert_eq!(applied.entities.len(), 1);
        assert!(applied.report.entries.is_empty());
        assert_eq!(world.iter_entities().count(), 1);
    }

    #[test]
    fn a_stale_load_with_nothing_to_prepare_again_from_adds_nothing() {
        let record = one_entity();
        let mut world = World::new();
        let prepared = prepare_in_place(&mut world, &record, Kept::Scene).expect("prepares");
        register_charge(&mut world);

        let applied = prepared.commit(&mut world, Identity::Keep);

        assert!(applied.entities.is_empty());
        assert!(matches!(
            applied.report.entries.last().map(|entry| &entry.kind),
            Some(ReportKind::NotLoaded { .. })
        ));
        assert_eq!(world.iter_entities().count(), 0);
    }
}
