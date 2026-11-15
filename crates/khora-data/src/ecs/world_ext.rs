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

//! Finds the entities carrying given [`Tag`]s, in O(N) over the world's entities.

use crate::ecs::components::Tag;
use crate::ecs::World;
use khora_core::ecs::entity::EntityId;

/// Returns every entity that carries the given tag.
pub fn entities_with_tag(world: &World, tag: &str) -> Vec<EntityId> {
    world
        .iter_entities()
        .filter(|&e| {
            world
                .get::<Tag>(e)
                .map(|t| t.contains(tag))
                .unwrap_or(false)
        })
        .collect()
}

/// Returns every entity that carries **any** of the given tags; none for an empty `tags`.
pub fn entities_with_any_tag(world: &World, tags: &[&str]) -> Vec<EntityId> {
    if tags.is_empty() {
        return Vec::new();
    }
    world
        .iter_entities()
        .filter(|&e| {
            world
                .get::<Tag>(e)
                .map(|t| tags.iter().any(|needle| t.contains(needle)))
                .unwrap_or(false)
        })
        .collect()
}

/// Returns every entity that carries **all** the given tags.
///
/// An empty `tags` matches every entity that has a `Tag` component.
pub fn entities_with_all_tags(world: &World, tags: &[&str]) -> Vec<EntityId> {
    world
        .iter_entities()
        .filter(|&e| {
            world
                .get::<Tag>(e)
                .map(|t| tags.iter().all(|needle| t.contains(needle)))
                .unwrap_or(false)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::Tag;

    #[test]
    fn entities_with_tag_filters_correctly() {
        let mut world = World::new();
        let a = world.spawn(Tag::from_iter(["enemy", "boss"]));
        let b = world.spawn(Tag::from_iter(["enemy"]));
        let _c = world.spawn(()); // no Tag component

        let enemies = entities_with_tag(&world, "enemy");
        assert_eq!(enemies.len(), 2);
        assert!(enemies.contains(&a));
        assert!(enemies.contains(&b));

        let bosses = entities_with_tag(&world, "boss");
        assert_eq!(bosses, vec![a]);

        let none = entities_with_tag(&world, "missing");
        assert!(none.is_empty());
    }

    #[test]
    fn entities_with_all_and_any_tags() {
        let mut world = World::new();
        let a = world.spawn(Tag::from_iter(["enemy", "boss", "fire"]));
        let _b = world.spawn(Tag::from_iter(["enemy"]));
        let c = world.spawn(Tag::from_iter(["fire"]));

        let any_fire_or_boss = entities_with_any_tag(&world, &["fire", "boss"]);
        assert_eq!(any_fire_or_boss.len(), 2);
        assert!(any_fire_or_boss.contains(&a));
        assert!(any_fire_or_boss.contains(&c));

        let all_enemy_and_fire = entities_with_all_tags(&world, &["enemy", "fire"]);
        assert_eq!(all_enemy_and_fire, vec![a]);
    }
}
