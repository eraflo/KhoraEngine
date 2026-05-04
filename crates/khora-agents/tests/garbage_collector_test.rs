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

use khora_data::ecs::{Component, EcsMaintenance, SemanticDomain, World};

// --- DUMMY COMPONENTS FOR THIS TEST ---
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Position(i32);
impl Component for Position {}

#[test]
fn test_ecs_maintenance_cleans_up_orphans() {
    // --- 1. ARRANGE ---
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    let mut maintenance = EcsMaintenance::new();

    // Create an orphan by spawning an entity and immediately removing its component domain.
    let entity_to_remove = world.spawn(Position(10));
    let orphan_location = world
        .remove_component_domain::<Position>(entity_to_remove)
        .expect("Removing component should create an orphan");

    // Create a "witness" entity that shared the page with the orphaned data.
    let witness_entity = world.spawn(Position(20));

    // Pre-action state verification
    assert!(
        world.get::<Position>(entity_to_remove).is_none(),
        "Component should be logically removed"
    );
    assert_eq!(
        *world.get::<Position>(witness_entity).unwrap(),
        Position(20),
        "Witness data should be correct before GC"
    );

    // Queue the cleanup task.
    maintenance.queue_cleanup(orphan_location, SemanticDomain::Spatial);

    // --- 2. ACT ---
    maintenance.tick(&mut world);

    // --- 3. ASSERT ---
    let witness_pos = world.get::<Position>(witness_entity);
    assert!(
        witness_pos.is_some(),
        "Witness entity should still have its Position component"
    );
    assert_eq!(
        *witness_pos.unwrap(),
        Position(20),
        "Witness data should be unchanged after GC"
    );

    let mut positions = Vec::new();
    for pos in world.query::<&Position>() {
        positions.push(*pos);
    }
    assert_eq!(
        positions.len(),
        1,
        "Query should only find one entity with a Position after GC"
    );
    assert_eq!(positions[0], Position(20));
}
