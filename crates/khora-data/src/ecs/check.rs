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

//! What a component's own check says about the values it holds.
//!
//! A component that can hold values the engine cannot use (a camera whose
//! near plane sits at its eye, …) declares a `check`. The check never
//! rewrites nor refuses a value: it names the field and says what is wrong,
//! and each boundary that asks it (a load, the running world) reports it.

use khora_core::ecs::entity::EntityId;
use khora_core::problems::ProblemSeverity;

use crate::ecs::{Component, World};

/// One problem a component's check found in one of its fields.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldProblem {
    /// The dotted path of the field in the component's serialized (JSON)
    /// form — the keys the inspector walks: `"z_near"`,
    /// `"projection.Perspective.fov_y_radians"`, `"light_type.Spot.range"`.
    pub field: &'static str,
    /// How much the problem matters.
    pub severity: ProblemSeverity,
    /// A plain English sentence naming the value:
    /// `near must be greater than 0 (it is 0)`.
    pub message: String,
}

/// Every problem `check` finds in every `T` of `world`, each with the entity
/// holding the value — what a registration's `check` hook runs.
///
/// Reads only: no value is rewritten.
pub fn check_all<T: Component>(
    world: &World,
    check: fn(&T) -> Vec<FieldProblem>,
) -> Vec<(EntityId, FieldProblem)> {
    world
        .query::<(EntityId, &T)>()
        .flat_map(|(entity, value)| {
            check(value)
                .into_iter()
                .map(move |problem| (entity, problem))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::Camera;

    /// Each problem the check finds comes back with the entity whose value
    /// it is in; a usable value adds nothing, and nothing is rewritten.
    #[test]
    fn check_all_pairs_each_problem_with_its_entity() {
        let mut world = World::new();
        let fine = world.spawn(Camera::default_perspective());
        let broken_camera = Camera::new_perspective(1.0, 0.0, -1.0);
        let broken = world.spawn(broken_camera);

        let mut found: Vec<(EntityId, &'static str)> = check_all::<Camera>(&world, Camera::check)
            .into_iter()
            .map(|(entity, problem)| (entity, problem.field))
            .collect();
        found.sort_by_key(|(_, field)| *field);

        assert_eq!(found, vec![(broken, "z_far"), (broken, "z_near")]);
        assert_eq!(world.get::<Camera>(broken), Some(&broken_camera));
        assert_eq!(
            world.get::<Camera>(fine),
            Some(&Camera::default_perspective())
        );
    }

    /// A world without the component has no problem with it.
    #[test]
    fn check_all_of_a_world_without_the_component_is_empty() {
        let world = World::new();
        assert!(check_all::<Camera>(&world, Camera::check).is_empty());
    }
}
