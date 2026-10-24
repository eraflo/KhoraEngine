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

//! Which pairs a sensor reports, on the Rapier backend: what enters it, not
//! the level geometry it was placed against.

use khora_core::ecs::entity::EntityId;
use khora_core::math::{Quat, Vec3};
use khora_core::physics::{
    ColliderDesc, ColliderHandle, ColliderShape, CollisionEvent, PhysicsProvider,
};
use khora_infra::physics::rapier::RapierPhysicsWorld;

const DT: f32 = 1.0 / 60.0;

fn standalone_box(owner: u32, position: Vec3, half: Vec3, is_sensor: bool) -> ColliderDesc {
    ColliderDesc {
        owner: Some(EntityId {
            index: owner,
            generation: 0,
        }),
        parent_body: None,
        position,
        rotation: Quat::IDENTITY,
        shape: ColliderShape::Box(half),
        active_events: false,
        friction: 0.5,
        restitution: 0.0,
        is_sensor,
        is_character: false,
    }
}

fn events_between(
    provider: &mut dyn PhysicsProvider,
    frames: usize,
    a: ColliderHandle,
    b: ColliderHandle,
) -> Vec<CollisionEvent> {
    let mut seen = Vec::new();
    for _ in 0..frames {
        provider.step(DT);
        seen.extend(provider.take_collision_events().into_iter().filter(|e| {
            let (x, y) = match e {
                CollisionEvent::Started(x, y) | CollisionEvent::Stopped(x, y) => (*x, *y),
            };
            (x == a && y == b) || (x == b && y == a)
        }));
    }
    seen
}

/// **A trigger placed on the floor does not report the floor.** A trigger
/// volume sunk 5 cm into the static ground — how every floor trigger is
/// placed — reports nothing about the ground: the ground did not enter it.
#[test]
fn a_trigger_placed_against_the_static_ground_does_not_report_it() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;
    let ground = provider.add_collider(standalone_box(
        0,
        Vec3::new(0.0, -0.5, 0.0),
        Vec3::new(40.0, 0.5, 40.0),
        false,
    ));
    let trigger = provider.add_collider(standalone_box(
        1,
        Vec3::new(0.0, 0.95, 0.0),
        Vec3::new(1.0, 1.0, 1.0),
        true,
    ));

    let events = events_between(provider, 10, ground, trigger);
    assert!(
        events.is_empty(),
        "a trigger resting on the static ground reports nothing about it; it reported {events:?}"
    );
}

/// Two trigger volumes that overlap where the level placed them report
/// nothing about each other: neither entered the other.
#[test]
fn two_overlapping_triggers_do_not_report_each_other() {
    let mut world = RapierPhysicsWorld::default();
    let provider: &mut dyn PhysicsProvider = &mut world;
    let first = provider.add_collider(standalone_box(
        1,
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(1.0, 1.0, 1.0),
        true,
    ));
    let second = provider.add_collider(standalone_box(
        2,
        Vec3::new(1.5, 1.0, 0.0),
        Vec3::new(1.0, 1.0, 1.0),
        true,
    ));

    let events = events_between(provider, 10, first, second);
    assert!(
        events.is_empty(),
        "two overlapping triggers report nothing about each other; they reported {events:?}"
    );
}
