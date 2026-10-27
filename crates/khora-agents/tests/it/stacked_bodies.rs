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

//! A stack of dynamic boxes on a static ground, through the whole physics
//! tick: provider sync → agent → write-back → propagation.
//!
//! A 3×3×5 tower of 0.6 m crates used to explode within two seconds with
//! nothing touching it. Every collider sitting on its body's own entity was
//! attached with its **world** position as an offset, and the per-frame sync
//! then overwrote the attached collider's world pose with that offset — so
//! contacts were generated against one pose and solved against another. These
//! tests pin what a resting stack, a resting box, and a scaled body look like
//! from the ECS side.

use khora_agents::lane_agent::LaneAgent;
use khora_core::agent::Agent;
use khora_core::ecs::entity::EntityId;
use khora_core::engine_context::EngineContext;
use khora_core::math::{Quat, Vec3};
use khora_core::physics::{ColliderHandle, PhysicsProvider, RigidBodyHandle};
use khora_core::Runtime;
use khora_data::ecs::{Collider, GlobalTransform, RigidBody, SimulatedTransform, Transform, World};
use khora_infra::physics::rapier::RapierPhysicsWorld;
use std::sync::{Arc, Mutex};

use crate::physics_family::fixtures::{physics_agent, physics_runtime};

type SharedProvider = Arc<Mutex<Box<dyn PhysicsProvider>>>;

/// Crate edge, in metres.
const SIZE: f32 = 0.6;
/// Top face of the ground box: centre −0.5, half-height 0.5.
const GROUND_TOP: f32 = 0.0;

fn make_runtime(provider: &SharedProvider) -> Arc<Runtime> {
    Arc::new(physics_runtime(provider))
}

/// A world, a Rapier provider, and the physics agent.
struct Rig {
    world: World,
    provider: SharedProvider,
    runtime: Arc<Runtime>,
    agent: LaneAgent,
}

impl Rig {
    fn new() -> Self {
        let world = World::new();
        let provider: SharedProvider =
            Arc::new(Mutex::new(Box::new(RapierPhysicsWorld::default())));
        let runtime = make_runtime(&provider);
        let agent = physics_agent();
        Self {
            world,
            provider,
            runtime,
            agent,
        }
    }

    /// One full frame, in the order `EngineCore::run_app_update` runs it:
    /// PreSimulation, PostSimulation (transform propagation), PreExtract
    /// (provider sync) data systems → Substrate-Pass flows → the agent with its
    /// own contention (provider step) → Maintenance (write-back).
    fn frame(&mut self) {
        use khora_control::substrate;
        use khora_core::lane::LaneBus;

        let mut bus = LaneBus::new();
        let mut deck = khora_core::lane::OutputDeck::new();
        for phase in [
            khora_data::ecs::TickPhase::PreSimulation,
            khora_data::ecs::TickPhase::PostSimulation,
            khora_data::ecs::TickPhase::PreExtract,
        ] {
            substrate::run_data_systems(&mut self.world, &self.runtime, &mut deck, phase);
        }
        substrate::run_flows(&mut self.world, &mut bus, &self.runtime);

        let permit = self.agent.contention();
        let mut ctx = EngineContext::for_agent(
            khora_core::WorldAccess::None,
            Arc::clone(&self.runtime),
            &bus,
            &mut deck,
            &permit,
            Some(self.agent.id()),
        );
        self.agent.execute(&mut ctx);

        substrate::run_data_systems(
            &mut self.world,
            &self.runtime,
            &mut deck,
            khora_data::ecs::TickPhase::Maintenance,
        );
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
    }

    /// Spawns an entity the way `Vessel::at` does: the authored `Transform`,
    /// the `GlobalTransform` it composes to, a body and a collider.
    fn spawn_body(
        &mut self,
        transform: Transform,
        body: RigidBody,
        collider: Collider,
    ) -> EntityId {
        let global = GlobalTransform::new(transform.to_mat4());
        self.world.spawn((transform, global, body, collider))
    }

    /// The static ground box: 80 × 1 × 80 m, top face at y = 0.
    fn spawn_ground(&mut self) -> EntityId {
        self.spawn_body(
            Transform::from_translation(Vec3::new(0.0, -0.5, 0.0)),
            RigidBody::new_static(),
            Collider::new_box(Vec3::new(40.0, 0.5, 40.0)),
        )
    }

    /// A 0.6 m, 8 kg crate centred at `at`.
    fn spawn_crate(&mut self, at: Vec3) -> EntityId {
        let half = SIZE * 0.5;
        self.spawn_body(
            Transform::from_translation(at),
            RigidBody::new_dynamic(8.0),
            Collider::new_box(Vec3::new(half, half, half)),
        )
    }

    /// A 3×3×5 tower centred on the origin, `gap` metres between crates and
    /// between the bottom layer and the ground. Returns each crate with its
    /// spawn position.
    fn spawn_tower(&mut self, gap: f32) -> Vec<(EntityId, Vec3)> {
        let step = SIZE + gap;
        let mut crates = Vec::new();
        for level in 0..5 {
            for i in 0..3 {
                for j in 0..3 {
                    let at = Vec3::new(
                        (i as f32 - 1.0) * step,
                        SIZE * 0.5 + gap + level as f32 * step,
                        (j as f32 - 1.0) * step,
                    );
                    crates.push((self.spawn_crate(at), at));
                }
            }
        }
        crates
    }

    /// Where the simulation put an entity this frame.
    fn pose(&self, entity: EntityId) -> (Vec3, Quat) {
        let pose = self
            .world
            .get::<SimulatedTransform>(entity)
            .expect("a dynamic body is simulated, so it has a simulated pose");
        (pose.translation(), pose.rotation())
    }

    fn handles(&self, entity: EntityId) -> (Option<RigidBodyHandle>, Option<ColliderHandle>) {
        let body = self.world.get::<RigidBody>(entity).and_then(|rb| rb.handle);
        let collider = self.world.get::<Collider>(entity).and_then(|c| c.handle);
        (body, collider)
    }

    fn provider_counts(&self) -> (usize, usize) {
        let provider = self.provider.lock().expect("provider mutex");
        (
            provider.get_all_bodies().len(),
            provider.get_all_colliders().len(),
        )
    }
}

/// The angle, in degrees, of the rotation taking `a` to `b`.
fn angle_between_degrees(a: Quat, b: Quat) -> f32 {
    let dot = a.normalize().dot(b.normalize()).abs().min(1.0);
    (2.0 * dot.acos()).to_degrees()
}

/// How far, in degrees, a body's up axis leans from world up.
fn tilt_degrees(rotation: Quat) -> f32 {
    let up = rotation.normalize() * Vec3::Y;
    up.normalize()
        .dot(Vec3::Y)
        .clamp(-1.0, 1.0)
        .acos()
        .to_degrees()
}

/// Runs a tower for 240 frames (4 s at 1/60) and checks, every frame after the
/// 30th, that each crate stays within 2 cm of where it settled at frame 30,
/// within 3° of upright, and with its bottom face not below the ground top.
fn assert_the_tower_stands(gap: f32) {
    const SETTLE: usize = 30;
    const FRAMES: usize = 240;

    let mut rig = Rig::new();
    rig.spawn_ground();
    let crates = rig.spawn_tower(gap);

    rig.frames(SETTLE);
    let settled: Vec<Vec3> = crates.iter().map(|(e, _)| rig.pose(*e).0).collect();

    for frame in SETTLE..FRAMES {
        rig.frame();
        for (index, ((entity, spawn), settled_at)) in crates.iter().zip(&settled).enumerate() {
            let (at, rotation) = rig.pose(*entity);
            let drift = (at - *settled_at).length();
            assert!(
                drift < 0.02,
                "gap {gap}: crate #{index} (spawned at {spawn:?}) moved {drift:.3} m from where \
                 it settled at frame {SETTLE} ({settled_at:?}) by frame {}: now at {at:?}",
                frame + 1
            );
            let tilt = tilt_degrees(rotation);
            assert!(
                tilt < 3.0,
                "gap {gap}: crate #{index} (spawned at {spawn:?}) leans {tilt:.2}° from upright \
                 at frame {}",
                frame + 1
            );
            let bottom = at.y - SIZE * 0.5;
            assert!(
                bottom > GROUND_TOP - 0.02,
                "gap {gap}: crate #{index} (spawned at {spawn:?}) has its bottom at {bottom:.3}, \
                 below the ground top, at frame {}",
                frame + 1
            );
        }
    }
}

/// **The tower that exploded.** 45 touching crates on a static ground, nothing
/// pushing them: they must settle and stay put for four seconds.
#[test]
fn a_tower_of_touching_crates_stands_still_for_four_seconds() {
    assert_the_tower_stands(0.0);
}

/// The same tower with 4 cm between crates: each layer drops onto the one
/// below, then the whole tower must rest. The explosion did not need contact
/// at spawn to happen.
#[test]
fn a_tower_of_spaced_crates_settles_and_stands_still() {
    assert_the_tower_stands(0.04);
}

/// A single crate dropped from 2 m comes to rest **on** the ground's top face,
/// centre at half its edge (0.3 m). The ground's own collider used to sit at
/// twice its body's offset (y = −1.0), so a box landed half a metre low.
#[test]
fn a_dropped_box_comes_to_rest_on_the_ground_top() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let crate_id = rig.spawn_crate(Vec3::new(0.0, 2.0, 0.0));

    rig.frames(120);

    let (at, _) = rig.pose(crate_id);
    assert!(
        (at.y - SIZE * 0.5).abs() < 0.02,
        "a 0.6 m box resting on a ground whose top is y = 0 has its centre at 0.3; it is at {at:?}"
    );
    assert!(
        at.x.abs() < 0.02 && at.z.abs() < 0.02,
        "a box dropped straight down lands straight below; it is at {at:?}"
    );
}

/// The same drop away from the origin. A collider attached at its body's world
/// position carried that position as a lever arm, so the error grew with the
/// distance from the origin: a box at the origin looked fine while one a few
/// metres away did not.
#[test]
fn a_box_dropped_away_from_the_origin_comes_to_rest_below_where_it_fell() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let from = Vec3::new(4.2, 2.0, -3.6);
    let crate_id = rig.spawn_crate(from);

    rig.frames(120);

    let (at, rotation) = rig.pose(crate_id);
    assert!(
        (at.y - SIZE * 0.5).abs() < 0.02,
        "a 0.6 m box resting on a ground whose top is y = 0 has its centre at 0.3; it is at {at:?}"
    );
    let off = Vec3::new(at.x - from.x, 0.0, at.z - from.z).length();
    assert!(
        off < 0.02,
        "a box dropped straight down lands straight below {from:?}; it is {off:.3} m away, at {at:?}"
    );
    let tilt = tilt_degrees(rotation);
    assert!(
        tilt < 3.0,
        "a box dropped flat lands flat; it leans {tilt:.2}°"
    );
}

/// Every entity keeps the body and collider it was given, and the provider
/// holds exactly one body and one collider per entity — no handle swapped
/// between entities, none leaked, none dropped, across the whole run.
#[test]
fn every_body_keeps_its_handles_and_the_provider_holds_one_per_entity() {
    let mut rig = Rig::new();
    let ground = rig.spawn_ground();
    let mut entities: Vec<EntityId> = rig.spawn_tower(0.0).into_iter().map(|(e, _)| e).collect();
    entities.push(ground);

    rig.frame();
    let first: Vec<_> = entities.iter().map(|e| rig.handles(*e)).collect();
    for (entity, (body, collider)) in entities.iter().zip(&first) {
        assert!(
            body.is_some() && collider.is_some(),
            "after one frame every entity has a body and a collider; {entity:?} has {body:?}, \
             {collider:?}"
        );
    }

    for frame in 1..240 {
        rig.frame();
        for (entity, was) in entities.iter().zip(&first) {
            let now = rig.handles(*entity);
            assert_eq!(
                now,
                *was,
                "{entity:?} changed handles at frame {}",
                frame + 1
            );
        }
        let (bodies, colliders) = rig.provider_counts();
        assert_eq!(
            (bodies, colliders),
            (entities.len(), entities.len()),
            "the provider holds one body and one collider per entity at frame {}",
            frame + 1
        );
    }
}

/// Each crate of a resting tower stays in its own column: its (x, z) stays
/// within 5 cm of where it was spawned. A crate wandering into a neighbour's
/// column is what an exploding — or a mis-mapped — stack looks like.
#[test]
fn every_crate_of_a_resting_tower_stays_in_its_own_column() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let crates = rig.spawn_tower(0.0);

    for frame in 0..240 {
        rig.frame();
        for (index, (entity, spawn)) in crates.iter().enumerate() {
            let (at, _) = rig.pose(*entity);
            let off = Vec3::new(at.x - spawn.x, 0.0, at.z - spawn.z).length();
            assert!(
                off < 0.05,
                "crate #{index} left its column (spawned at {spawn:?}): it is {off:.3} m away in \
                 (x, z) at frame {}, at {at:?}",
                frame + 1
            );
        }
    }
}

/// **A scaled body started turned by the wrong angle.** The sync read a body's
/// rotation out of its scaled world matrix, and the extraction let the scale
/// into the angle: scale 0.6 with a 30° yaw reached the solver as another yaw.
#[test]
fn a_scaled_body_starts_with_its_authored_rotation() {
    let mut rig = Rig::new();
    let yaw = Quat::from_axis_angle(Vec3::Y, 30f32.to_radians());
    let body = rig.spawn_body(
        Transform::new(Vec3::new(2.0, 1.0, -3.0), yaw, Vec3::new(0.6, 0.6, 0.6)),
        RigidBody::new_static(),
        Collider::new_box(Vec3::new(0.5, 0.5, 0.5)),
    );

    rig.frame();

    let handle = rig
        .handles(body)
        .0
        .expect("one frame registers the body with the provider");
    let (_, rotation) = rig
        .provider
        .lock()
        .expect("provider mutex")
        .get_body_transform(handle);
    assert!(
        (rotation.magnitude() - 1.0).abs() < 1e-4,
        "the body's rotation is a unit quaternion; norm {}",
        rotation.magnitude()
    );
    let error = angle_between_degrees(rotation, yaw);
    assert!(
        error < 0.5,
        "scale 0.6 + 30° yaw must reach the provider as a 30° yaw; off by {error:.2}° \
         ({rotation:?})"
    );
}

/// **A simulated entity rendered at scale 1.** Propagation took the simulated
/// pose — translation and rotation, a solver does not resize — whole as the
/// `GlobalTransform`, dropping the authored scale from the first simulated
/// frame on.
#[test]
fn a_scaled_dynamic_body_keeps_its_scale_while_simulated() {
    let mut rig = Rig::new();
    let body = rig.spawn_body(
        Transform::new(
            Vec3::new(0.0, 5.0, 0.0),
            Quat::IDENTITY,
            Vec3::new(2.0, 2.0, 2.0),
        ),
        RigidBody::new_dynamic(1.0),
        Collider::new_box(Vec3::new(0.5, 0.5, 0.5)),
    );

    rig.frames(10);

    assert!(
        rig.world.get::<SimulatedTransform>(body).is_some(),
        "after ten frames the body is simulated"
    );
    let scale = rig
        .world
        .get::<GlobalTransform>(body)
        .expect("the body has a world pose")
        .0
        .scale();
    assert!(
        (scale - Vec3::new(2.0, 2.0, 2.0)).length() < 1e-3,
        "the authored scale 2 survives the simulation; GlobalTransform scale is {scale:?}"
    );
}

/// The height at which a ray cast straight down through `(x, z)` first hits a
/// collider, if it hits one — where the solver believes a surface is.
fn top_under(rig: &Rig, x: f32, z: f32) -> Option<f32> {
    let ray = khora_core::physics::Ray {
        origin: Vec3::new(x, 20.0, z),
        direction: Vec3::new(0.0, -1.0, 0.0),
    };
    rig.provider
        .lock()
        .expect("provider mutex")
        .cast_ray(&ray, 40.0, true)
        .map(|hit| hit.position.y)
}

/// A collider on a **child** entity of a body is described by its rigid offset
/// in the body's frame: turned by the body's rotation, stretched by the
/// hierarchy's scale (the offset is in world units), centred where the child
/// is drawn and as big as it is drawn.
///
/// Body at `(2, 1, 0)`, 90° yaw, scale 2; child at local `(1, 0, 0)`, a box of
/// half-extent 0.25 — 0.5 at the scale the child inherits. The child is drawn
/// at `(2, 1, −2)` — top face 1.5 — and nothing belongs at the unturned or
/// unscaled offsets.
#[test]
fn a_collider_on_a_child_of_a_turned_scaled_body_sits_where_the_child_is_drawn() {
    use khora_data::ecs::Parent;

    let mut rig = Rig::new();
    let yaw = Quat::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2);
    let placed = Transform::new(Vec3::new(2.0, 1.0, 0.0), yaw, Vec3::new(2.0, 2.0, 2.0));
    let body = rig.world.spawn((
        placed,
        GlobalTransform::new(placed.to_mat4()),
        RigidBody::new_static(),
    ));
    let child = rig.world.spawn((
        Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
        GlobalTransform::identity(),
        Parent(body),
        Collider::new_box(Vec3::new(0.25, 0.25, 0.25)),
    ));

    rig.frames(3);

    let drawn = rig
        .world
        .get::<GlobalTransform>(child)
        .expect("the child has a world pose")
        .0
        .translation();
    assert!(
        (drawn - Vec3::new(2.0, 1.0, -2.0)).length() < 1e-3,
        "the child is drawn at (2, 1, −2); it is at {drawn:?}"
    );
    let top = top_under(&rig, drawn.x, drawn.z);
    assert!(
        top.is_some_and(|y| (y - 1.5).abs() < 0.01),
        "the child's box belongs where the child is drawn, at the size it is drawn — half-extent          0.25 × the parent's scale 2, top at 1.5; the ray there hit {top:?}"
    );
    for (x, z, what) in [(2.0, -1.0, "unscaled"), (4.0, 0.0, "unturned")] {
        let stray = top_under(&rig, x, z);
        assert!(
            stray.is_none(),
            "nothing belongs at the {what} offset ({x}, ·, {z}); the ray there hit {stray:?}"
        );
    }
}

/// A dynamic body whose second box sits on a **child** entity falls as one
/// compound and rests on the ground with both boxes where they are drawn: the
/// body's box centred at 0.3, the child's on top of it, centred at 0.9.
#[test]
fn a_dynamic_body_with_a_child_collider_rests_where_both_boxes_are_drawn() {
    use khora_data::ecs::Parent;

    let mut rig = Rig::new();
    rig.spawn_ground();
    let half = SIZE * 0.5;
    let from = Vec3::new(1.5, 2.0, -1.0);
    let body = rig.spawn_crate(from);
    let child = rig.world.spawn((
        Transform::from_translation(Vec3::new(0.0, SIZE, 0.0)),
        GlobalTransform::identity(),
        Parent(body),
        Collider::new_box(Vec3::new(half, half, half)),
    ));

    rig.frames(180);

    let (at, rotation) = rig.pose(body);
    assert!(
        (at.y - half).abs() < 0.02 && tilt_degrees(rotation) < 3.0,
        "the body's box rests flat on the ground, centre at {half}; it is at {at:?}, leaning \
         {:.2}°",
        tilt_degrees(rotation)
    );
    let drawn = rig
        .world
        .get::<GlobalTransform>(child)
        .expect("the child has a world pose")
        .0
        .translation();
    assert!(
        (drawn.y - (half + SIZE)).abs() < 0.02,
        "the child box is drawn on top of the body's, centre at {}; it is at {drawn:?}",
        half + SIZE
    );
    let top = top_under(&rig, drawn.x, drawn.z);
    assert!(
        top.is_some_and(|y| (y - (drawn.y + half)).abs() < 0.02),
        "the solver's top surface is the drawn child's top, {}; the ray hit {top:?}",
        drawn.y + half
    );
}

/// **A collider given its body a frame late jumped to the origin.** An entity
/// with a collider and no body has a standalone collider, placed in the world.
/// When a body is added to it later — an inspector adding a component, a
/// script making a prop physical — the sync starts describing that collider
/// relative to its body (identity), but the backend still holds it standalone
/// and reads the identity as a **world** pose.
///
/// Collider at `(3, 1, 0)`, half-extent 0.5, then a static body on the same
/// entity: the box stays where it is drawn, top at 1.5.
#[test]
fn a_collider_that_gets_a_body_later_stays_where_it_is_drawn() {
    let mut rig = Rig::new();
    let at = Vec3::new(3.0, 1.0, 0.0);
    let placed = Transform::from_translation(at);
    let entity = rig.world.spawn((
        placed,
        GlobalTransform::new(placed.to_mat4()),
        Collider::new_box(Vec3::new(0.5, 0.5, 0.5)),
    ));
    rig.frame();
    let before = top_under(&rig, at.x, at.z);
    assert!(
        before.is_some_and(|y| (y - 1.5).abs() < 0.01),
        "a standalone box at (3, 1, 0) has its top at 1.5; the ray hit {before:?}"
    );

    rig.world
        .add_component(entity, RigidBody::new_static())
        .expect("a body can be added to an entity with a collider");
    rig.frames(3);

    let after = top_under(&rig, at.x, at.z);
    assert!(
        after.is_some_and(|y| (y - 1.5).abs() < 0.01),
        "adding a body does not move the box: its top stays at 1.5; the ray at (3, ·, 0) hit \
         {after:?} (none means the collider went to the origin)"
    );
}

/// An entity whose body is taken away keeps its collider, standalone, where
/// the entity is drawn. Removing the body removed the shapes attached to it,
/// and the entity's `Collider.handle` went on naming a collider that no longer
/// existed.
///
/// Static body with its collider at `(−3, 1, 0)`, half-extent 0.5; the
/// `RigidBody` is removed: the box is still there, top at 1.5.
#[test]
fn a_collider_whose_body_is_removed_stays_where_it_is_drawn() {
    let mut rig = Rig::new();
    let at = Vec3::new(-3.0, 1.0, 0.0);
    let entity = rig.spawn_body(
        Transform::from_translation(at),
        RigidBody::new_static(),
        Collider::new_box(Vec3::new(0.5, 0.5, 0.5)),
    );
    rig.frames(3);

    rig.world
        .remove_component::<RigidBody>(entity)
        .expect("the body can be removed");
    rig.frames(3);

    let top = top_under(&rig, at.x, at.z);
    assert!(
        top.is_some_and(|y| (y - 1.5).abs() < 0.01),
        "the entity still has a collider at (−3, 1, 0), top at 1.5; the ray hit {top:?}"
    );
}
