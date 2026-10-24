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

//! The authored physics components at their edges, through the whole physics
//! tick on a real Rapier provider: releases under scaled parents, mirrored
//! bodies, and a character that loses its collider.

use khora_agents::physics_agent::PhysicsAgent;
use khora_core::agent::Agent;
use khora_core::ecs::entity::EntityId;
use khora_core::engine_context::EngineContext;
use khora_core::math::{Mat4, Quat, Vec3};
use khora_core::physics::PhysicsProvider;
use khora_core::Runtime;
use khora_data::ecs::{
    BodyMotion, Collider, GlobalTransform, KinematicCharacterController, Parent, RigidBody,
    SimulatedTransform, Teleported, Transform, World,
};
use khora_infra::physics::rapier::RapierPhysicsWorld;
use std::sync::{Arc, Mutex};

type SharedProvider = Arc<Mutex<Box<dyn PhysicsProvider>>>;

/// Half a 0.6 m crate edge.
const HALF: f32 = 0.3;

static NOTHING: &khora_core::agent::Contention = &khora_core::agent::Contention {
    deck: Vec::new(),
    reads: Vec::new(),
    writes: Vec::new(),
};

/// A world, a Rapier provider and an initialised physics agent.
struct Rig {
    world: World,
    runtime: Arc<Runtime>,
    agent: PhysicsAgent,
}

impl Rig {
    fn new() -> Self {
        let mut world = World::new();
        let provider: SharedProvider =
            Arc::new(Mutex::new(Box::new(RapierPhysicsWorld::default())));
        let runtime = {
            let mut runtime = Runtime::new();
            runtime.backends.insert(Arc::clone(&provider));
            runtime
                .resources
                .insert(khora_core::physics::collision_channel());
            Arc::new(runtime)
        };
        let mut agent = PhysicsAgent::default();
        {
            let bus = khora_core::lane::LaneBus::new();
            let mut deck = khora_core::lane::OutputDeck::new();
            let mut ctx = EngineContext::for_agent(
                khora_core::WorldAccess::Exclusive(&mut world as &mut dyn std::any::Any),
                Arc::clone(&runtime),
                &bus,
                &mut deck,
                NOTHING,
                None,
            );
            agent.on_initialize(&mut ctx);
        }
        Self {
            world,
            runtime,
            agent,
        }
    }

    /// One full frame, in the order `EngineCore::run_app_update` runs it.
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
            khora_core::WorldAccess::Exclusive(&mut self.world as &mut dyn std::any::Any),
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

    fn spawn_body(
        &mut self,
        transform: Transform,
        body: RigidBody,
        collider: Collider,
    ) -> EntityId {
        let global = GlobalTransform::new(transform.to_mat4());
        self.world.spawn((transform, global, body, collider))
    }

    /// The static ground box: 200 × 1 × 200 m, top face at y = 0.
    fn spawn_ground(&mut self) -> EntityId {
        self.spawn_body(
            Transform::from_translation(Vec3::new(0.0, -0.5, 0.0)),
            RigidBody::new_static(),
            Collider::new_box(Vec3::new(100.0, 0.5, 100.0)),
        )
    }

    fn drawn(&self, entity: EntityId) -> Mat4 {
        Mat4::from(
            self.world
                .get::<GlobalTransform>(entity)
                .expect("the entity has a world pose")
                .0,
        )
    }

    fn transform(&self, entity: EntityId) -> Transform {
        *self
            .world
            .get::<Transform>(entity)
            .expect("the entity has a Transform")
    }
}

/// The largest element-wise difference between two matrices.
fn matrix_gap(a: Mat4, b: Mat4) -> f32 {
    let mut gap = 0.0f32;
    for i in 0..4 {
        for j in 0..4 {
            gap = gap.max((a.cols[i][j] - b.cols[i][j]).abs());
        }
    }
    gap
}

// ─── Release ────────────────────────────────────────────────────────────────

/// **A release under a parent authored in centimetres.** A parent at scale
/// 0.01 — the root an FBX export gives a scene authored in centimetres — with
/// a child body at local scale 100 (1 m in the world). The body falls and
/// rests; its `RigidBody` is removed; it stays drawn where it was and its
/// `Transform`, under its parent, says so.
#[test]
fn a_body_released_under_a_centimetre_parent_stays_drawn_where_it_was() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let root = Transform::new(Vec3::ZERO, Quat::IDENTITY, Vec3::new(0.01, 0.01, 0.01));
    let parent = rig
        .world
        .spawn((root, GlobalTransform::new(root.to_mat4())));
    let child = rig.world.spawn((
        // 300 cm up, at 100 × the parent's scale: a 0.6 m crate 3 m up.
        Transform::new(
            Vec3::new(0.0, 300.0, 0.0),
            Quat::IDENTITY,
            Vec3::new(100.0, 100.0, 100.0),
        ),
        GlobalTransform::identity(),
        Parent(parent),
        RigidBody::new_dynamic(8.0),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    ));
    rig.frames(120);
    let rested = rig.drawn(child);
    assert!(
        (rested.cols[3].y - HALF).abs() < 0.03,
        "precondition: the 0.6 m crate fell and rests at {HALF}; it is drawn at {:?}",
        rested.cols[3]
    );

    rig.world
        .remove_component::<RigidBody>(child)
        .expect("the body can be removed");
    rig.frames(3);

    let gap = matrix_gap(rig.drawn(child), rested);
    assert!(
        gap < 1e-3,
        "the released crate is drawn where it last was; its world matrix moved by {gap} (drawn \
         at {:?}, its Transform says {:?})",
        rig.drawn(child).cols[3],
        rig.transform(child)
    );
}

/// **A mirrored body keeps its mirror where it was authored.** A crate
/// authored at scale (1, 1, −1), turned 30° about Y, settles and loses its
/// body. It stays drawn where it was, and its `Transform` keeps the authored
/// scale and turn — not an equivalent pose mirrored on another axis and
/// turned half a turn more, which an inspector would show as numbers nobody
/// typed.
#[test]
fn a_released_mirrored_body_keeps_its_authored_scale_and_turn() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let turn = Quat::from_axis_angle(Vec3::Y, 30f32.to_radians());
    let authored = Vec3::new(1.0, 1.0, -1.0);
    let crate_id = rig.spawn_body(
        Transform::new(Vec3::new(0.0, HALF + 0.01, 0.0), turn, authored),
        RigidBody::new_dynamic(8.0),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    );
    rig.frames(90);
    let rested = rig.drawn(crate_id);
    rig.world
        .remove_component::<RigidBody>(crate_id)
        .expect("the body can be removed");
    rig.frames(3);

    let gap = matrix_gap(rig.drawn(crate_id), rested);
    assert!(
        gap < 1e-3,
        "the released mirrored crate is drawn where it last was; moved by {gap}"
    );
    let placed = rig.transform(crate_id);
    assert!(
        (placed.scale - authored).length() < 1e-3,
        "the released crate keeps its authored scale {authored:?}; its Transform says {:?} \
         (rotation {:?})",
        placed.scale,
        placed.rotation
    );
}

/// **A release under a non-uniformly scaled parent.** A parent at scale
/// (2, 1, 1) holds a child body turned 30° about Y. The child falls flat and
/// loses its body: it stays where it was.
///
/// Its *shape* may not: simulated, it was drawn unsheared; handed back, it is
/// drawn as any child of a stretched parent turned against its axes is —
/// sheared — and no `Transform` can say otherwise. The place is what holds.
#[test]
fn a_body_released_under_a_stretched_parent_stays_where_it_was() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let stretched = Transform::new(Vec3::ZERO, Quat::IDENTITY, Vec3::new(2.0, 1.0, 1.0));
    let parent = rig
        .world
        .spawn((stretched, GlobalTransform::new(stretched.to_mat4())));
    let child = rig.world.spawn((
        Transform::new(
            Vec3::new(0.0, 1.0, 0.0),
            Quat::from_axis_angle(Vec3::Y, 30f32.to_radians()),
            Vec3::ONE,
        ),
        GlobalTransform::identity(),
        Parent(parent),
        RigidBody::new_dynamic(8.0),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    ));
    rig.frames(120);
    let rested = rig.drawn(child);

    rig.world
        .remove_component::<RigidBody>(child)
        .expect("the body can be removed");
    rig.frames(3);

    let now = rig.drawn(child).cols[3];
    let then = rested.cols[3];
    let moved =
        ((now.x - then.x).powi(2) + (now.y - then.y).powi(2) + (now.z - then.z).powi(2)).sqrt();
    assert!(
        moved < 1e-3,
        "the released child stays where it was; it moved by {moved}"
    );
}

/// **A character that loses its collider is released.** It is no longer
/// simulated — no `SimulatedTransform` — stays drawn where it walked to, and
/// its `Transform` says so.
#[test]
fn a_character_whose_collider_is_removed_is_released_where_it_walked_to() {
    let mut rig = Rig::new();
    let placed = Transform::from_translation(Vec3::new(0.0, 1.0, 0.0));
    let character = rig.world.spawn((
        placed,
        GlobalTransform::new(placed.to_mat4()),
        KinematicCharacterController::default(),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    ));
    for _ in 0..20 {
        rig.world
            .get_mut::<KinematicCharacterController>(character)
            .expect("a character has a controller")
            .desired_translation = Vec3::new(0.05, 0.0, 0.0);
        rig.frame();
    }
    rig.frame();
    let walked = rig.drawn(character);
    assert!(
        walked.cols[3].x > 0.9,
        "precondition: the character walked about 1 m along +X; it is drawn at {:?}",
        walked.cols[3]
    );

    rig.world
        .remove_component::<Collider>(character)
        .expect("the collider can be removed");
    rig.frames(3);

    assert!(
        rig.world.get::<SimulatedTransform>(character).is_none(),
        "a controller with no collider is not simulated"
    );
    let gap = matrix_gap(rig.drawn(character), walked);
    assert!(
        gap < 1e-3,
        "the released character is drawn where it walked to; it moved by {gap}"
    );
    let now = rig.transform(character).translation;
    assert!(
        (now - walked.cols[3].truncate()).length() < 1e-3,
        "its Transform says where it walked to; it says {now:?}"
    );
}

// ─── Sleep ──────────────────────────────────────────────────────────────────

/// **A resting crate falls asleep.** The per-frame sync used to take every
/// collider mutably, and Rapier counts that alone as a change: the collider's
/// body was woken every frame, and nothing in a scene ever slept. Asleep, the
/// solver leaves it alone and its velocity is exactly zero.
#[test]
fn a_crate_resting_on_the_ground_falls_asleep() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let crate_id = rig.spawn_body(
        Transform::from_translation(Vec3::new(0.0, HALF + 0.05, 0.0)),
        RigidBody::new_dynamic(8.0),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    );

    rig.frames(600);

    let motion = *rig
        .world
        .get::<BodyMotion>(crate_id)
        .expect("a simulated body has a motion");
    assert!(
        motion.linear == Vec3::ZERO && motion.angular == Vec3::ZERO,
        "a crate at rest for ten seconds is asleep, its velocity exactly zero; it moves at          {motion:?}"
    );
}

/// Whether the provider has put a body to sleep: its velocity is then exactly
/// zero.
fn is_asleep(rig: &Rig, entity: EntityId) -> bool {
    let motion = *rig
        .world
        .get::<BodyMotion>(entity)
        .expect("a simulated body has a motion");
    motion.linear == Vec3::ZERO && motion.angular == Vec3::ZERO
}

fn simulated_at(rig: &Rig, entity: EntityId) -> Vec3 {
    rig.world
        .get::<SimulatedTransform>(entity)
        .expect("a dynamic body is simulated")
        .translation()
}

/// **A surface edited under a sleeping body reaches it.** A crate held on a
/// 20° slope by friction 1 falls asleep there; both surfaces are then made
/// frictionless, and the crate slides down — `tan 20° ≈ 0.36`, so nothing
/// holds it any more.
#[test]
fn a_crate_asleep_on_a_slope_slides_once_the_slope_is_made_frictionless() {
    let mut rig = Rig::new();
    let tilt = Quat::from_axis_angle(Vec3::Z, 20f32.to_radians());
    let mut rough = Collider::new_box(Vec3::new(5.0, 0.25, 5.0));
    rough.friction = 1.0;
    let slope = rig.spawn_body(
        Transform::new(Vec3::ZERO, tilt, Vec3::ONE),
        RigidBody::new_static(),
        rough,
    );
    let up = tilt * Vec3::Y;
    let mut grippy = Collider::new_box(Vec3::new(HALF, HALF, HALF));
    grippy.friction = 1.0;
    let crate_id = rig.spawn_body(
        Transform::new(up * (0.25 + HALF + 0.01), tilt, Vec3::ONE),
        RigidBody::new_dynamic(8.0),
        grippy,
    );
    rig.frames(600);
    assert!(
        is_asleep(&rig, crate_id),
        "precondition: the crate held on the slope by friction is asleep"
    );
    let rested = simulated_at(&rig, crate_id);

    for entity in [slope, crate_id] {
        rig.world
            .get_mut::<Collider>(entity)
            .expect("the entity has a collider")
            .friction = 0.0;
    }
    rig.frames(60);

    let slid = (simulated_at(&rig, crate_id) - rested).length();
    assert!(
        slid > 0.5,
        "a second after both surfaces became frictionless the crate has slid down the slope \
         (≈ 1.7 m); it moved {slid:.4} m"
    );
}

/// **A sensor toggle reaches a body asleep on it.** A crate asleep on a
/// platform falls once the platform is made a sensor.
#[test]
fn a_crate_asleep_on_a_platform_made_a_sensor_falls_through_it() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let platform = rig.world.spawn((
        Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
        GlobalTransform::new(Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0))),
        Collider::new_box(Vec3::new(2.0, 0.25, 2.0)),
    ));
    let crate_id = rig.spawn_body(
        Transform::from_translation(Vec3::new(0.0, 1.56, 0.0)),
        RigidBody::new_dynamic(8.0),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    );
    rig.frames(600);
    assert!(
        is_asleep(&rig, crate_id),
        "precondition: the crate is asleep on the platform"
    );

    rig.world
        .get_mut::<Collider>(platform)
        .expect("the platform has a collider")
        .is_sensor = true;
    rig.frames(120);

    let at = simulated_at(&rig, crate_id);
    assert!(
        (at.y - HALF).abs() < 0.03,
        "the crate falls through the platform made a sensor onto the ground, centre at {HALF}; \
         it is at {at:?}"
    );
}

/// **A crate rescaled where it stands grows out of the ground.** A crate
/// resting on the ground is scaled to 2 by its `Transform` alone — no declared
/// move — a few frames after landing. Its collider reaches the provider at the
/// new size, and the crate, now half sunk into the floor, is pushed up and
/// rests with its centre at 0.6.
#[test]
fn a_crate_rescaled_where_it_rests_rises_to_its_new_height() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let crate_id = rig.spawn_body(
        Transform::from_translation(Vec3::new(0.0, HALF + 0.01, 0.0)),
        RigidBody::new_dynamic(8.0),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    );
    rig.frames(30);

    rig.world
        .get_mut::<Transform>(crate_id)
        .expect("the crate has a Transform")
        .scale = Vec3::new(2.0, 2.0, 2.0);
    rig.frames(120);

    let at = simulated_at(&rig, crate_id);
    assert!(
        (at.y - 2.0 * HALF).abs() < 0.03,
        "rescaled to 2 where it rests, the crate rests with its centre at 0.6; it is at {at:?}"
    );
}

/// **A static platform moved from under a sleeping body lets it fall.**
#[test]
fn a_crate_asleep_on_a_static_platform_moved_away_falls() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let placed = Transform::from_translation(Vec3::new(0.0, 1.0, 0.0));
    let platform = rig.spawn_body(
        placed,
        RigidBody::new_static(),
        Collider::new_box(Vec3::new(2.0, 0.25, 2.0)),
    );
    let crate_id = rig.spawn_body(
        Transform::from_translation(Vec3::new(0.0, 1.56, 0.0)),
        RigidBody::new_dynamic(8.0),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    );
    rig.frames(600);
    assert!(
        is_asleep(&rig, crate_id),
        "precondition: the crate is asleep on the platform"
    );

    *rig.world
        .get_mut::<Transform>(platform)
        .expect("the platform has a Transform") =
        Transform::from_translation(Vec3::new(50.0, 1.0, 0.0));
    rig.world
        .add_component(platform, Teleported)
        .expect("the platform can be marked as moved");
    rig.frames(120);

    let at = simulated_at(&rig, crate_id);
    assert!(
        (at.y - HALF).abs() < 0.03,
        "the platform moved away, the crate falls to the ground, centre at {HALF}; it is at {at:?}"
    );
}

/// **A release under a turned, mirrored parent.** A parent at scale
/// (−1, 1, 1) turned 40° holds a child body authored at (1, 1, −1) turned
/// 30°. Released, the child stays drawn where it was and keeps its authored
/// scale.
#[test]
fn a_body_released_under_a_turned_mirrored_parent_stays_drawn_and_keeps_its_scale() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let mirror = Transform::new(
        Vec3::new(2.0, 0.0, 0.0),
        Quat::from_axis_angle(Vec3::Y, 40f32.to_radians()),
        Vec3::new(-1.0, 1.0, 1.0),
    );
    let parent = rig
        .world
        .spawn((mirror, GlobalTransform::new(mirror.to_mat4())));
    let authored = Vec3::new(1.0, 1.0, -1.0);
    let child = rig.world.spawn((
        Transform::new(
            Vec3::new(0.0, 2.0, 0.0),
            Quat::from_axis_angle(Vec3::Y, 30f32.to_radians()),
            authored,
        ),
        GlobalTransform::identity(),
        Parent(parent),
        RigidBody::new_dynamic(8.0),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    ));
    rig.frames(120);
    let rested = rig.drawn(child);

    rig.world
        .remove_component::<RigidBody>(child)
        .expect("the body can be removed");
    rig.frames(3);

    let gap = matrix_gap(rig.drawn(child), rested);
    assert!(
        gap < 1e-3,
        "the released child is drawn where it last was; its world matrix moved by {gap}"
    );
    let placed = rig.transform(child);
    assert!(
        (placed.scale - authored).length() < 1e-3,
        "the released child keeps its authored scale {authored:?}; its Transform says {:?}",
        placed.scale
    );
}
