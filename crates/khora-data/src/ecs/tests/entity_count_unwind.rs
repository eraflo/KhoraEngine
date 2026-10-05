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

//! The live count against the one path that kills an entity behind its back:
//! a structural change that unwinds between taking the entity's metadata out
//! of its slot and putting it back.

use std::panic::{catch_unwind, AssertUnwindSafe};

use super::{Position, Velocity};
use crate::ecs::{Component, SemanticDomain, World};

/// A registered component whose clone panics: copying it from one page to
/// another — which every migration does — unwinds mid-change.
#[derive(Debug, PartialEq, Eq)]
struct PanicsOnClone;
impl Clone for PanicsOnClone {
    fn clone(&self) -> Self {
        panic!("PanicsOnClone is cloned by a migration");
    }
}
impl Component for PanicsOnClone {}

#[test]
fn a_migration_that_unwinds_does_not_leave_the_count_disagreeing_with_the_live() {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);
    world.register_component::<PanicsOnClone>(SemanticDomain::Spatial);

    // A spawn moves its bundle into the page; nothing is cloned.
    let keep = world.spawn(Position(0));
    let entity = world.spawn((Position(1), PanicsOnClone));
    assert_eq!(world.entity_count(), 2);

    // Migrating the entity copies every component of its page row, and
    // copying `PanicsOnClone` panics: the migration unwinds after it started
    // on the entity's metadata, before the change was put back.
    let unwound = catch_unwind(AssertUnwindSafe(|| {
        let _ = world.add_component(entity, Velocity(1));
    }));
    assert!(unwound.is_err(), "the migration unwinds");

    let live = world.iter_entities().count();
    assert_eq!(
        world.entity_count(),
        live,
        "the entity is no longer alive (contains: {}), yet it is still counted",
        world.contains(entity)
    );
    assert!(world.contains(keep));
}
