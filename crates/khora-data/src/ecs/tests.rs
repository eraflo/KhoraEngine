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

use super::component::Component;
use super::world::World;

// --- DUMMY COMPONENTS FOR TESTING ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Represents a position with a single `i32` value.
struct Position(i32);
impl Component for Position {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Velocity(i32);
impl Component for Velocity {}

// --- TESTS ---

#[test]
fn test_spawn_single_entity() {
    // --- 1. SETUP ---
    // Create a new, empty world.
    let mut world = World::default();

    // Define the component data we want to spawn.
    let position = Position(10);
    let velocity = Velocity(-5);

    // --- 2. ACTION ---
    // Spawn a new entity with the component bundle.
    let entity_id = world.spawn((position, velocity));

    // --- 3. ASSERTIONS ---
    // Verify that the world state is exactly as we expect it to be.

    // Check the returned EntityId.
    assert_eq!(entity_id.index, 0, "The first entity should have index 0");
    assert_eq!(
        entity_id.generation, 0,
        "The first entity should have generation 0"
    );

    // Check the world's entity list.
    assert_eq!(
        world.entities.len(),
        1,
        "There should be one entity entry in the world"
    );
    let (stored_id, metadata_opt) = &world.entities[0];
    assert!(metadata_opt.is_some(), "Entity slot should be occupied");

    let metadata = metadata_opt.as_ref().unwrap();

    // Check that the stored ID and metadata are correct.
    assert_eq!(
        *stored_id, entity_id,
        "The stored ID should match the returned ID"
    );

    // Check the metadata's location pointer.
    let location = metadata
        .physics_location
        .expect("Physics location should be set");
    assert_eq!(
        location.page_id, 0,
        "Location should point to the first page"
    );
    assert_eq!(
        location.row_index, 0,
        "Location should point to the first row"
    );
    assert!(
        metadata.render_location.is_none(),
        "Render location should not be set"
    );

    // Check the world's page list.
    assert_eq!(world.pages.len(), 1, "There should be one page allocated");
    let page = &world.pages[0];

    // Check the page's entity list.
    assert_eq!(page.entities.len(), 1, "The page should track one entity");
    assert_eq!(
        page.entities[0], entity_id,
        "The page should track the correct entity ID"
    );
}

#[test]
fn test_despawn_single_entity() {
    // --- 1. SETUP ---
    let mut world = World::default();
    let entity_id = world.spawn((Position(10), Velocity(-5)));

    // --- 2. ACTION ---
    let despawn_result = world.despawn(entity_id);

    // --- 3. ASSERTIONS ---
    assert!(despawn_result);

    // Check entity list and free list.
    assert_eq!(world.entities.len(), 1);
    assert!(
        world.entities[0].1.is_none(),
        "The entity's metadata slot should now be None"
    );
    assert_eq!(world.freed_entities, vec![0]);

    // Check the page's state.
    assert_eq!(world.pages.len(), 1);
    let page = &world.pages[0];
    // Use our new helper method to confirm data was removed.
    assert_eq!(
        page.row_count(),
        0,
        "All rows should have been removed from the page"
    );
}

#[test]
fn test_entity_id_recycling_and_aba_protection() {
    // --- 1. SETUP ---
    let mut world = World::default();

    // --- 2. ACTION & ASSERTIONS ---

    // --- Part A: Spawn and Despawn ---
    // Spawn the first entity.
    let id_a = world.spawn((Position(1), Velocity(1)));
    assert_eq!(id_a.index, 0);
    assert_eq!(id_a.generation, 0);

    // Despawn it to free up its index.
    let despawn_result_a = world.despawn(id_a);
    assert!(despawn_result_a);
    assert_eq!(world.freed_entities, vec![0]); // Index 0 is now free.
    assert!(world.entities[0].1.is_none());

    // --- Part B: Recycle the ID ---
    // Spawn a second entity. It should reuse the index 0.
    let id_b = world.spawn((Position(2), Velocity(2)));

    // Assert that the new ID has the same index but an incremented generation.
    assert_eq!(id_b.index, 0, "The recycled entity should have index 0");
    assert_eq!(
        id_b.generation, 1,
        "The generation should be incremented to 1"
    );
    assert!(
        world.freed_entities.is_empty(),
        "The free list should be empty again"
    );
    assert!(
        world.entities[0].1.is_some(),
        "The slot should be occupied again"
    );

    // --- Part C: ABA Protection ---
    // Try to despawn using the old, stale handle (`id_a`).
    // This should fail because the generation (0) does not match the
    // world's current generation for this index (1).
    let despawn_result_stale = world.despawn(id_a);
    assert!(
        !despawn_result_stale,
        "Despawning with a stale ID should fail"
    );

    // Verify that the world state was NOT affected by the failed despawn.
    // The entity `id_b` should still be alive and well.
    assert!(
        world.entities[0].1.is_some(),
        "The slot for id_b should not have been freed"
    );
    let (current_id, _) = &world.entities[0];
    assert_eq!(
        *current_id, id_b,
        "The entity in the world should still be id_b"
    );

    // --- Part D: Successful Despawn with the Correct ID ---
    // Despawning with the new, correct handle (`id_b`) should succeed.
    let despawn_result_b = world.despawn(id_b);
    assert!(
        despawn_result_b,
        "Despawning with the correct ID should succeed"
    );
    assert!(
        world.entities[0].1.is_none(),
        "The slot should be free again after the correct despawn"
    );
}

#[test]
fn test_despawn_with_swap_remove_logic() {
    // --- 1. SETUP ---
    let mut world = World::default();

    // Spawn two entities with the same bundle. This ensures they will be
    // placed in the same `ComponentPage`.
    let entity_a = world.spawn((Position(1), Velocity(1))); // Will be at row 0
    let entity_b = world.spawn((Position(2), Velocity(2))); // Will be at row 1

    // Pre-action sanity checks
    assert_eq!(
        world.pages.len(),
        1,
        "Both entities should be in the same, single page"
    );
    assert_eq!(
        world.pages[0].row_count(),
        2,
        "The page should have two rows"
    );

    // Check initial metadata for entity B
    let metadata_b_before = world.entities[entity_b.index as usize].1.as_ref().unwrap();
    let location_b_before = metadata_b_before.physics_location.unwrap();
    assert_eq!(location_b_before.page_id, 0);
    assert_eq!(
        location_b_before.row_index, 1,
        "Entity B should initially be at row 1"
    );

    // --- 2. ACTION ---
    // Despawn the *first* entity (entity_a). This will trigger the swap_remove
    // logic, moving entity_b's data from row 1 to row 0.
    let despawn_result = world.despawn(entity_a);
    assert!(despawn_result, "Despawn should succeed");

    // --- 3. ASSERTIONS ---
    // Verify that entity B has been correctly moved and its metadata updated.

    // Check basic world state
    assert_eq!(
        world.pages[0].row_count(),
        1,
        "The page should now have only one row"
    );
    assert!(
        world.entities[entity_a.index as usize].1.is_none(),
        "Entity A's slot should be None"
    );
    assert!(
        world.entities[entity_b.index as usize].1.is_some(),
        "Entity B's slot should still be Some"
    );

    // THE CRITICAL CHECK: Verify that entity B's metadata has been updated.
    let metadata_b_after = world.entities[entity_b.index as usize].1.as_ref().unwrap();
    let location_b_after = metadata_b_after.physics_location.unwrap();

    assert_eq!(
        location_b_after.page_id, 0,
        "Page ID for B should not change"
    );
    assert_eq!(
        location_b_after.row_index, 0,
        "Entity B should have been moved to row 0"
    );

    // Verify that the entity ID stored in the page at the new location is correct
    let page = &world.pages[0];
    assert_eq!(
        page.entities[0], entity_b,
        "The page should now track entity B at row 0"
    );
}
