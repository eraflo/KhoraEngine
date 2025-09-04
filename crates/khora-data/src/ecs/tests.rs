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

use crate::ecs::query::Without;
use crate::ecs::SemanticDomain;

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

// A component that is Clone but not Copy, to test migration logic.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NonCopyableComponent(String);
impl Component for NonCopyableComponent {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderTag;
impl Component for RenderTag {}

// --- TESTS ---

#[test]
fn test_spawn_single_entity() {
    // --- 1. SETUP ---
    // Create a new, empty world.
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);

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
    assert_eq!(
        metadata.locations.len(),
        1,
        "Should have one location entry"
    );
    let location = metadata
        .locations
        .get(&SemanticDomain::Spatial)
        .expect("Spatial location should be set");
    assert_eq!(
        location.page_id, 0,
        "Location should point to the first page"
    );
    assert_eq!(
        location.row_index, 0,
        "Location should point to the first row"
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
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);

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
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);

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
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);

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
    let (_, metadata_b_before_opt) = &world.entities[entity_b.index as usize];
    let metadata_b_before = metadata_b_before_opt.as_ref().unwrap();
    let location_b_before = metadata_b_before
        .locations
        .get(&SemanticDomain::Spatial)
        .unwrap();
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
    let (_, metadata_b_after_opt) = &world.entities[entity_b.index as usize];
    let metadata_b_after = metadata_b_after_opt.as_ref().unwrap();
    let location_b_after = metadata_b_after
        .locations
        .get(&SemanticDomain::Spatial)
        .unwrap();

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

#[test]
fn test_simple_query_fetches_correct_components() {
    // ARRANGE: Set up the world with a variety of entities.
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);

    // Spawn an entity that should be found by a `query::<&Position>()`.
    world.spawn(Position(10));

    // Spawn an entity that should *not* be found.
    // This will create a different ComponentPage based on its unique signature.
    world.spawn(Velocity(-5));

    // Spawn another entity that should also be found.
    world.spawn(Position(30));

    // ACT: Run the query to collect all `Position` components.
    let mut found_positions = Vec::new();
    for position_ref in world.query::<&Position>() {
        found_positions.push(*position_ref);
    }

    // ASSERT: Verify that the query returned the correct data.
    // Check that we found the correct number of entities.
    assert_eq!(
        found_positions.len(),
        2,
        "Query should have found exactly 2 entities with a Position"
    );

    // Sort the results to make the test deterministic.
    found_positions.sort_by_key(|p| p.0);

    // Check that we have the correct data.
    assert_eq!(found_positions, vec![Position(10), Position(30)]);
}

#[test]
fn test_mutable_query_modifies_components() {
    // --- 1. ARRANGE ---
    // Set up a world with some entities that we intend to modify.
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);

    world.spawn(Position(10));
    world.spawn(Velocity(-5)); // This one should be ignored.
    world.spawn(Position(30));

    // --- 2. ACT ---
    // Run a mutable query and modify the `Position` components.
    for position_ref in world.query::<&mut Position>() {
        // Multiply the position's value by 2.
        position_ref.0 *= 2;
    }

    // --- 3. ASSERT ---
    // Run an immutable query to verify the changes were applied.
    let mut final_positions = Vec::new();
    for position_ref in world.query::<&Position>() {
        final_positions.push(*position_ref);
    }

    // Sort the results for a deterministic test.
    final_positions.sort_by_key(|p| p.0);

    // Check that the values have been updated.
    assert_eq!(
        final_positions,
        vec![Position(20), Position(60)], // 10*2=20, 30*2=60
        "The mutable query should have updated the component values"
    );
}

#[test]
fn test_query_with_without_filter() {
    // --- 1. ARRANGE ---
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);

    // Spawn an entity with both components. This one should be IGNORED by the query.
    world.spawn((Position(10), Velocity(100)));

    // Spawn an entity with only a Position. This one should be FOUND.
    world.spawn(Position(20));

    // --- 2. ACT ---
    // Query for entities that have a `Position` but `Without` a `Velocity`.
    let mut found_positions = Vec::new();
    for position_ref in world.query::<(&Position, Without<Velocity>)>() {
        // The query item for a tuple is a tuple, so we destructure it.
        // The item for `Without<Velocity>` is `()`, which we can ignore.
        let (pos, ()) = position_ref;
        found_positions.push(*pos);
    }

    // --- 3. ASSERT ---
    assert_eq!(
        found_positions.len(),
        1,
        "Query should find exactly one entity"
    );
    assert_eq!(
        found_positions[0],
        Position(20),
        "Query should find the entity with only a Position"
    );
}

#[test]
fn test_tuple_query_matches_entities_with_all_components() {
    // --- 1. ARRANGE ---
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);

    // Spawn an entity with both components. Should be FOUND.
    world.spawn((Position(10), Velocity(100)));

    // Spawn an entity with only one of the required components. Should be IGNORED.
    world.spawn(Position(20));

    // Spawn another entity with both components. Should be FOUND.
    world.spawn((Position(30), Velocity(300)));

    // --- 2. ACT ---
    // Query for all entities that have *both* a `Position` and a `Velocity`.
    let mut found_results = Vec::new();
    for (pos_ref, vel_ref) in world.query::<(&Position, &Velocity)>() {
        found_results.push((*pos_ref, *vel_ref));
    }

    // --- 3. ASSERT ---
    assert_eq!(
        found_results.len(),
        2,
        "Query should find exactly two entities with both components"
    );

    // Sort the results by position to make the test deterministic.
    found_results.sort_by_key(|(pos, _vel)| pos.0);

    // Check that we have the correct data.
    assert_eq!(
        found_results,
        vec![(Position(10), Velocity(100)), (Position(30), Velocity(300))]
    );
}

#[test]
fn test_spawn_with_unregistered_component() {
    // --- 1. ARRANGE ---
    let mut world = World::default();

    // NOTE: We do NOT register the `Position` component.
    // world.register_component::<Position>(SemanticDomain::Spatial);

    // --- 2. ACT ---
    // We spawn an entity with a component that the world knows nothing about.
    let entity_id = world.spawn(Position(10));

    // --- 3. ASSERT ---
    // We verify the current behavior: the entity is created, but its
    // metadata is empty because the component's domain could not be found.

    // The entity ID is still allocated correctly.
    assert_eq!(entity_id.index, 0);

    let (_id, metadata_opt) = &world.entities[0];
    let metadata = metadata_opt.as_ref().unwrap();

    // CRITICAL CHECK: The locations map should be empty.
    assert!(
        metadata.locations.is_empty(),
        "Metadata should have no locations for an unregistered component"
    );

    // A page is still created, but the entity's metadata doesn't point to it.
    // This highlights that the data is stored but becomes unreachable.
    assert_eq!(
        world.pages.len(),
        1,
        "A page for the new component layout should still be created"
    );
}

#[test]
fn test_add_component_to_new_domain() {
    // --- 1. ARRANGE ---
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<RenderTag>(SemanticDomain::Render); // Register in a different domain

    let entity = world.spawn(Position(100));

    // --- 2. ACT ---
    let success = world.add_component(entity, RenderTag);

    // --- 3. ASSERT ---
    assert!(success, "add_component should succeed");

    // Check metadata: should now have TWO locations.
    let (_id, metadata_opt) = &world.entities[entity.index as usize];
    let metadata = metadata_opt.as_ref().unwrap();
    assert_eq!(
        metadata.locations.len(),
        2,
        "Entity should have locations in two domains"
    );

    // Check the Spatial location: it should NOT have moved.
    let spatial_loc = metadata.locations.get(&SemanticDomain::Spatial).unwrap();
    assert_eq!(spatial_loc.page_id, 0);
    assert_eq!(spatial_loc.row_index, 0);

    // Check the new Render location.
    let render_loc = metadata.locations.get(&SemanticDomain::Render).unwrap();
    assert_eq!(
        render_loc.page_id, 1,
        "Render component should be in a new page"
    );
    assert_eq!(render_loc.row_index, 0);

    // Check that the data is correct in both pages.
    let spatial_pos = world.get::<Position>(entity).unwrap();
    assert_eq!(spatial_pos.0, 100);
    let render_tag = world.get::<RenderTag>(entity).unwrap();
    assert_eq!(*render_tag, RenderTag);
}

#[test]
fn test_add_component_triggers_data_migration() {
    // --- 1. ARRANGE ---
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);
    world.register_component::<NonCopyableComponent>(SemanticDomain::Spatial);

    // Entity A will be modified.
    let entity_a = world.spawn((Position(10), NonCopyableComponent("Hello".to_string())));
    // Entity B is a "witness" in the same page that should not be affected.
    let entity_b = world.spawn((Position(20), NonCopyableComponent("World".to_string())));

    // Pre-action state verification
    assert_eq!(
        world.pages.len(),
        1,
        "Both entities should be in a single page"
    );
    let initial_page = &world.pages[0];
    assert_eq!(initial_page.row_count(), 2);
    let initial_loc_a = world.entities[entity_a.index as usize]
        .1
        .as_ref()
        .unwrap()
        .locations
        .get(&SemanticDomain::Spatial)
        .unwrap();
    assert_eq!(initial_loc_a.page_id, 0);

    // --- 2. ACT ---
    // Add Velocity to entity A. This forces it to migrate to a new page.
    let success = world.add_component(entity_a, Velocity(100));

    // --- 3. ASSERT ---
    assert!(success, "add_component should succeed");

    // A) Check Page State
    assert_eq!(
        world.pages.len(),
        2,
        "A new page should have been created for the new component layout"
    );

    // The old page (page 0) should now only contain entity B.
    let old_page = &world.pages[0];
    assert_eq!(
        old_page.row_count(),
        1,
        "Old page should now have one entity"
    );
    assert_eq!(
        old_page.entities[0], entity_b,
        "Old page should only contain entity B"
    );

    // The new page (page 1) should now contain entity A.
    let new_page = &world.pages[1];
    assert_eq!(new_page.row_count(), 1, "New page should have one entity");
    assert_eq!(
        new_page.entities[0], entity_a,
        "New page should contain entity A"
    );

    // B) Check Entity A's Metadata and Data
    let (_id, metadata_a_opt) = &world.entities[entity_a.index as usize];
    let metadata_a = metadata_a_opt.as_ref().unwrap();
    let loc_a = metadata_a.locations.get(&SemanticDomain::Spatial).unwrap();
    assert_eq!(
        loc_a.page_id, 1,
        "Entity A's metadata should point to the new page"
    );
    assert_eq!(
        loc_a.row_index, 0,
        "Entity A should be at the first row of the new page"
    );

    // Verify all of A's components are accessible and correct.
    let pos_a = world.get::<Position>(entity_a).unwrap();
    let vel_a = world.get::<Velocity>(entity_a).unwrap();
    let non_copy_a = world.get::<NonCopyableComponent>(entity_a).unwrap();
    assert_eq!(pos_a.0, 10);
    assert_eq!(vel_a.0, 100);
    assert_eq!(non_copy_a.0, "Hello");

    // C) Check Entity B's State (should be unchanged)
    let (_id, metadata_b_opt) = &world.entities[entity_b.index as usize];
    let metadata_b = metadata_b_opt.as_ref().unwrap();
    let loc_b = metadata_b.locations.get(&SemanticDomain::Spatial).unwrap();
    assert_eq!(
        loc_b.page_id, 0,
        "Entity B's metadata should still point to the old page"
    );
}
