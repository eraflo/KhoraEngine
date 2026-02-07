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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    let (stored_id, metadata_opt) = world.entities.get(0).unwrap();
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
    assert_eq!(
        world.storage.pages.len(),
        1,
        "There should be one page allocated"
    );
    let page = &world.storage.pages[0];

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
        world.entities.get(0).unwrap().1.is_none(),
        "The entity's metadata slot should now be None"
    );
    assert_eq!(world.entities.freed_entities, vec![0]);

    // Check the page's state.
    assert_eq!(world.storage.pages.len(), 1);
    let page = &world.storage.pages[0];
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
    assert_eq!(world.entities.freed_entities, vec![0]); // Index 0 is now free.
    assert!(world.entities.get(0).unwrap().1.is_none());

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
        world.entities.freed_entities.is_empty(),
        "The free list should be empty again"
    );
    assert!(
        world.entities.get(0).unwrap().1.is_some(),
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
        world.entities.get(0).unwrap().1.is_some(),
        "The slot for id_b should not have been freed"
    );
    let (current_id, _) = world.entities.get(0).unwrap();
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
        world.entities.get(0).unwrap().1.is_none(),
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
        world.storage.pages.len(),
        1,
        "Both entities should be in the same, single page"
    );
    assert_eq!(
        world.storage.pages[0].row_count(),
        2,
        "The page should have two rows"
    );

    // Check initial metadata for entity B
    let (_, metadata_b_before_opt) = world.entities.get(entity_b.index as usize).unwrap();
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
        world.storage.pages[0].row_count(),
        1,
        "The page should now have only one row"
    );
    assert!(
        world
            .entities
            .get(entity_a.index as usize)
            .unwrap()
            .1
            .is_none(),
        "Entity A's slot should be None"
    );
    assert!(
        world
            .entities
            .get(entity_b.index as usize)
            .unwrap()
            .1
            .is_some(),
        "Entity B's slot should still be Some"
    );

    // THE CRITICAL CHECK: Verify that entity B's metadata has been updated.
    let (_, metadata_b_after_opt) = world.entities.get(entity_b.index as usize).unwrap();
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
    let page = &world.storage.pages[0];
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

    let (_id, metadata_opt) = world.entities.get(0).unwrap();
    let metadata = metadata_opt.as_ref().unwrap();

    // CRITICAL CHECK: The locations map should be empty.
    assert!(
        metadata.locations.is_empty(),
        "Metadata should have no locations for an unregistered component"
    );

    // A page is still created, but the entity's metadata doesn't point to it.
    // This highlights that the data is stored but becomes unreachable.
    assert_eq!(
        world.storage.pages.len(),
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
    let orphan_result = world.add_component(entity, RenderTag).unwrap();

    // --- 3. ASSERT ---
    assert!(
        orphan_result.is_none(),
        "Adding to a new domain should not create an orphan"
    );

    // Check metadata: should now have TWO locations.
    let (_id, metadata_opt) = world.entities.get(entity.index as usize).unwrap();
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

    let entity_a = world.spawn((Position(10), NonCopyableComponent("Hello".to_string())));
    let entity_b = world.spawn((Position(20), NonCopyableComponent("World".to_string())));

    assert_eq!(
        world.storage.pages.len(),
        1,
        "Both entities should be in a single page"
    );
    let initial_loc_a = *world
        .entities
        .get(entity_a.index as usize)
        .unwrap()
        .1
        .as_ref()
        .unwrap()
        .locations
        .get(&SemanticDomain::Spatial)
        .unwrap();
    assert_eq!(initial_loc_a.page_id, 0);

    // --- 2. ACT ---
    // Add Velocity to entity A. This forces it to migrate and should return the old location.
    let orphan_result = world.add_component(entity_a, Velocity(100)).unwrap();

    // --- 3. ASSERT ---

    // A) Check the result of the operation
    assert!(
        orphan_result.is_some(),
        "Adding to an existing domain SHOULD create an orphan"
    );
    assert_eq!(
        orphan_result.unwrap(),
        initial_loc_a,
        "The orphan location should be the original location of entity A"
    );

    // B) Check Page State (before Garbage Collection)
    assert_eq!(
        world.storage.pages.len(),
        2,
        "A new page should have been created for the new component layout"
    );

    // The old page (page 0) still contains the orphaned data for entity A
    // and the valid data for entity B. Its row count has NOT changed yet.
    let old_page = &world.storage.pages[0];
    assert_eq!(
        old_page.row_count(),
        2,
        "Old page should still have two rows (one is an orphan)"
    );

    // The new page (page 1) should now contain entity A.
    let new_page = &world.storage.pages[1];
    assert_eq!(new_page.row_count(), 1, "New page should have one entity");
    assert_eq!(
        new_page.entities[0], entity_a,
        "New page should contain entity A"
    );

    // C) Check Entity A's Metadata and Data
    let (_id, metadata_a_opt) = world.entities.get(entity_a.index as usize).unwrap();
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

    // Verify all of A's components are accessible at its new location.
    let pos_a = world.get::<Position>(entity_a).unwrap();
    let vel_a = world.get::<Velocity>(entity_a).unwrap();
    let non_copy_a = world.get::<NonCopyableComponent>(entity_a).unwrap();

    assert_eq!(pos_a.0, 10);
    assert_eq!(vel_a.0, 100);
    assert_eq!(non_copy_a.0, "Hello");

    // D) Check Entity B's State (should be completely unchanged by the migration)
    let (_id, metadata_b_opt) = world.entities.get(entity_b.index as usize).unwrap();
    let metadata_b = metadata_b_opt.as_ref().unwrap();
    let loc_b = metadata_b.locations.get(&SemanticDomain::Spatial).unwrap();

    assert_eq!(
        loc_b.page_id, 0,
        "Entity B's metadata should still point to the old page"
    );

    let pos_b = world.get::<Position>(entity_b).unwrap();

    assert_eq!(pos_b.0, 20, "Entity B's data should be unaffected");
}

// --- MUTABLE QUERY TESTS ---

#[test]
fn test_simple_mutable_query_modifies_components() {
    // --- 1. ARRANGE ---
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.spawn(Position(10));
    world.spawn(Position(30));

    // --- 2. ACT ---
    // Run a mutable query to modify the `Position` components.
    for position_mut in world.query_mut::<&mut Position>() {
        position_mut.0 *= 2;
    }

    // --- 3. ASSERT ---
    // Run an immutable query to verify the changes.
    let mut final_positions = Vec::new();
    for position_ref in world.query::<&Position>() {
        final_positions.push(*position_ref);
    }
    final_positions.sort_by_key(|p| p.0);
    assert_eq!(final_positions, vec![Position(20), Position(60)]);
}

#[test]
fn test_complex_mutable_query_with_filter() {
    // --- 1. ARRANGE ---
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);
    world.register_component::<RenderTag>(SemanticDomain::Render);

    // Entity 1: Should be processed. Has Position and Velocity, but no RenderTag.
    world.spawn((Position(10), Velocity(5)));

    // Entity 2: Should be ignored. Has Position, but no Velocity.
    world.spawn(Position(20));

    // Entity 3: Should be processed.
    world.spawn((Position(30), Velocity(1)));

    // Entity 4: Should be ignored. Has all components, but the `Without<RenderTag>` filter excludes it.
    world.spawn((Position(40), Velocity(1), RenderTag));

    // --- 2. ACT ---
    // Query for entities with a mutable Position and an immutable Velocity,
    // but only if they do NOT have a RenderTag.
    // The physics system is a classic example of this pattern.
    for (pos_mut, vel_ref, ()) in
        world.query_mut::<(&mut Position, &Velocity, Without<RenderTag>)>()
    {
        pos_mut.0 += vel_ref.0;
    }

    // --- 3. ASSERT ---
    // Check the final state of all entities to ensure only the correct ones were modified.
    let mut final_positions = Vec::new();
    for pos in world.query::<&Position>() {
        final_positions.push(*pos);
    }
    final_positions.sort_by_key(|p| p.0);

    // Entity 1: 10 + 5 = 15
    // Entity 2: 20 (unchanged)
    // Entity 3: 30 + 1 = 31
    // Entity 4: 40 (unchanged)
    assert_eq!(
        final_positions,
        vec![Position(15), Position(20), Position(31), Position(40)]
    );
}

#[test]
fn test_transversal_lifecycle() {
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<RenderTag>(SemanticDomain::Render);

    // 1. Spawn: Verify bits are set
    let entity = world.spawn((Position(1), RenderTag));

    {
        let spatial_bitset = world
            .storage
            .domain_bitsets
            .get(&SemanticDomain::Spatial)
            .unwrap();
        let render_bitset = world
            .storage
            .domain_bitsets
            .get(&SemanticDomain::Render)
            .unwrap();

        assert!(spatial_bitset.is_set(entity.index));
        assert!(render_bitset.is_set(entity.index));
    }

    // 2. Remove domain: Verify bit is cleared
    world.remove_component_domain::<RenderTag>(entity).unwrap();
    {
        let render_bitset = world
            .storage
            .domain_bitsets
            .get(&SemanticDomain::Render)
            .unwrap();
        let spatial_bitset = world
            .storage
            .domain_bitsets
            .get(&SemanticDomain::Spatial)
            .unwrap();
        assert!(!render_bitset.is_set(entity.index));
        assert!(spatial_bitset.is_set(entity.index)); // Spatial should remain
    }

    // 3. Add component: Verify bit is set again
    world.add_component(entity, RenderTag).unwrap();
    {
        let render_bitset = world
            .storage
            .domain_bitsets
            .get(&SemanticDomain::Render)
            .unwrap();
        assert!(render_bitset.is_set(entity.index));
    }

    // 4. Despawn: Verify all bits cleared
    world.despawn(entity);
    {
        let spatial_bitset = world
            .storage
            .domain_bitsets
            .get(&SemanticDomain::Spatial)
            .unwrap();
        let render_bitset = world
            .storage
            .domain_bitsets
            .get(&SemanticDomain::Render)
            .unwrap();
        assert!(!spatial_bitset.is_set(entity.index));
        assert!(!render_bitset.is_set(entity.index));
    }
}

#[test]
fn test_transversal_join() {
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<RenderTag>(SemanticDomain::Render);

    // Spawn entities across different domains
    world.spawn((Position(1), RenderTag)); // Entity 0: both
    world.spawn(Position(2)); // Entity 1: Spatial only
    world.spawn(RenderTag); // Entity 2: Render only
    world.spawn((Position(3), RenderTag)); // Entity 3: both

    // Native query: Spatial only
    let spatial_count = world.query::<&Position>().count();
    assert_eq!(spatial_count, 3); // 0, 1, 3

    // Transversal query: Spatial + Render
    let join_results: Vec<_> = world
        .query::<(&Position, &RenderTag)>()
        .map(|(p, _)| p.0)
        .collect();

    assert_eq!(join_results, vec![1, 3]);

    // Verify it works with QueryMut too
    for (p, _) in world.query_mut::<(&mut Position, &RenderTag)>() {
        p.0 += 10;
    }

    let final_results: Vec<_> = world
        .query::<(&Position, &RenderTag)>()
        .map(|(p, _)| p.0)
        .collect();
    assert_eq!(final_results, vec![11, 13]);
}

#[test]
fn test_transversal_recycled_entities() {
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<RenderTag>(SemanticDomain::Render);

    // 1. Spawn and despawn to create a "hole"
    let e1 = world.spawn((Position(10), RenderTag));
    world.despawn(e1);

    // 2. Spawn a new entity - it should recycle the index
    let e2 = world.spawn((Position(20), RenderTag));
    assert_eq!(e1.index, e2.index);
    assert_ne!(e1.generation, e2.generation);

    // 3. Query should only find the new entity
    let mut query = world.query::<(&Position, &RenderTag)>();
    let (p, _) = query.next().unwrap();
    assert_eq!(p.0, 20);
    assert!(query.next().is_none());

    // 4. Verify bitset was cleared and set correctly
    {
        let spatial_bitset = world
            .storage
            .domain_bitsets
            .get(&SemanticDomain::Spatial)
            .unwrap();
        assert!(spatial_bitset.is_set(e2.index));
    }
}

#[test]
fn test_transversal_sparse_join() {
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<RenderTag>(SemanticDomain::Render);

    // Create a sparse situation:
    // Domain A (Spatial) has many entities.
    // Domain B (Render) has very few.
    for i in 0..100 {
        world.spawn(Position(i));
    }

    // Only 2 entities have both
    world.spawn((Position(1000), RenderTag));
    world.spawn((Position(2000), RenderTag));

    // Another 50 in Domain B only
    for _ in 0..50 {
        world.spawn(RenderTag);
    }

    // Query for BOTH.
    // The driver should ideally be Render (Domain B) because it has fewer pages/entities
    // (though in this small test it might vary, the bitset intersection should fast-skip).
    let results: Vec<_> = world
        .query::<(&Position, &RenderTag)>()
        .map(|(p, _)| p.0)
        .collect();

    assert_eq!(results.len(), 2);
    assert!(results.contains(&1000));
    assert!(results.contains(&2000));
}

#[test]
fn test_transversal_concurrency() {
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<RenderTag>(SemanticDomain::Render);

    for i in 0..1000 {
        if i % 2 == 0 {
            world.spawn((Position(i), RenderTag));
        } else {
            world.spawn(Position(i));
        }
    }

    // Wrap World in Arc for sharing (note: query is &self, so it should be Send/Sync)
    use std::sync::Arc;
    let world = Arc::new(world);
    let mut handles = Vec::new();

    for _ in 0..8 {
        let world_clone = world.clone();
        handles.push(std::thread::spawn(move || {
            let count = world_clone.query::<(&Position, &RenderTag)>().count();
            assert_eq!(count, 500);
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn test_get_many_mut() {
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Spatial);

    let e1 = world.spawn((Position(10), Velocity(1)));
    let e2 = world.spawn((Position(20), Velocity(2)));
    let e3 = world.spawn(Position(30));

    // 1. Success case: disjoint entities
    {
        let [p1, p2] = world.get_many_mut::<Position, 2>([e1, e2]);
        assert_eq!(p1.as_ref().unwrap().0, 10);
        assert_eq!(p2.as_ref().unwrap().0, 20);

        p1.unwrap().0 += 100;
        p2.unwrap().0 += 200;
    }
    assert_eq!(world.get::<Position>(e1).unwrap().0, 110);
    assert_eq!(world.get::<Position>(e2).unwrap().0, 220);

    // 2. Failure case: duplicate IDs
    {
        let [p1, p2] = world.get_many_mut::<Position, 2>([e1, e1]);
        assert!(p1.is_none());
        assert!(p2.is_none());
    }

    // 3. Mixed case: one missing component
    {
        let [p1, p2] = world.get_many_mut::<Velocity, 2>([e1, e3]);
        assert!(p1.is_some());
        assert!(p2.is_none()); // e3 only has Position
    }

    // 4. Case: invalid EntityId
    {
        use khora_core::ecs::entity::EntityId;
        let invalid_id = EntityId {
            index: 999,
            generation: 0,
        };
        let [p1] = world.get_many_mut::<Position, 1>([invalid_id]);
        assert!(p1.is_none());
    }
}
