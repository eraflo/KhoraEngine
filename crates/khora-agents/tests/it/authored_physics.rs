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

//! What an author writes on the physics components reaches the simulation
//! whole, through the whole physics tick: provider sync → agent → write-back →
//! propagation, on a real Rapier provider.
//!
//! - **Surface.** A collider's friction and restitution are its `Collider`
//!   fields.
//! - **Sensors.** `Collider.is_sensor` makes a collider that pushes nothing,
//!   weighs nothing, reports what enters and leaves it — with or without
//!   `ActiveEvents` — and lets a character walk through it.
//! - **Scale.** `Collider.shape` is in the entity's local units: the
//!   simulation sees it at the size the entity is drawn.
//! - **Release.** An entity nothing simulates any more is handed back where it
//!   is, and its `Transform` drives it again.
//! - **Character teleport.** A character moved by its `Transform` stays where
//!   it was put.

use khora_agents::lane_agent::LaneAgent;
use khora_core::agent::Agent;
use khora_core::ecs::entity::EntityId;
use khora_core::engine_context::EngineContext;
use khora_core::event::Channel;
use khora_core::math::{Mat4, Quat, Vec3};
use khora_core::physics::{ColliderShape, Collision, CollisionKind, PhysicsProvider};
use khora_core::Runtime;
use khora_data::ecs::{
    ActiveEvents, BodyMotion, Collider, GlobalTransform, KinematicCharacterController, Parent,
    RigidBody, SimulatedTransform, Teleported, Transform, World,
};
use khora_infra::physics::rapier::RapierPhysicsWorld;
use std::sync::{Arc, Mutex};

use crate::physics_family::fixtures::{physics_agent, physics_runtime};

type SharedProvider = Arc<Mutex<Box<dyn PhysicsProvider>>>;

/// Crate edge, in metres.
const SIZE: f32 = 0.6;
/// Half a crate edge: where a crate resting on the ground has its centre.
const HALF: f32 = SIZE * 0.5;
/// The reader name this file reads the collision channel under.
const READER: &str = "authored_physics_test";

/// A world, a Rapier provider, the physics agent, and every contact
/// the collision channel carried so far.
struct Rig {
    world: World,
    provider: SharedProvider,
    runtime: Arc<Runtime>,
    agent: LaneAgent,
    collisions: Channel<Collision>,
    contacts: Vec<Collision>,
}

impl Rig {
    fn new() -> Self {
        let world = World::new();
        let provider: SharedProvider =
            Arc::new(Mutex::new(Box::new(RapierPhysicsWorld::default())));
        let collisions = khora_core::physics::collision_channel();
        let runtime = {
            let mut runtime = physics_runtime(&provider);
            runtime.resources.insert(collisions.clone());
            Arc::new(runtime)
        };
        let agent = physics_agent();
        Self {
            world,
            provider,
            runtime,
            agent,
            collisions,
            contacts: Vec::new(),
        }
    }

    /// One full frame, in the order `EngineCore::run_app_update` runs it:
    /// PreSimulation, PostSimulation (transform propagation), PreExtract
    /// (provider sync) data systems → Substrate-Pass flows → the agent with its
    /// own contention (provider step) → Maintenance (write-back, collision
    /// dispatch). The contacts the channel received are kept.
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

        self.contacts.extend(self.collisions.read_for(READER));
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

    /// A collider with no body: it lives in the world where it is drawn.
    fn spawn_standalone(&mut self, transform: Transform, collider: Collider) -> EntityId {
        let global = GlobalTransform::new(transform.to_mat4());
        self.world.spawn((transform, global, collider))
    }

    /// The static ground box: 80 × 1 × 80 m, top face at y = 0.
    fn spawn_ground(&mut self) -> EntityId {
        self.spawn_ground_with(Collider::new_box(Vec3::new(40.0, 0.5, 40.0)))
    }

    /// The ground box with the collider given.
    fn spawn_ground_with(&mut self, collider: Collider) -> EntityId {
        self.spawn_body(
            Transform::from_translation(Vec3::new(0.0, -0.5, 0.0)),
            RigidBody::new_static(),
            collider,
        )
    }

    /// A 0.6 m, 8 kg crate centred at `at`.
    fn spawn_crate(&mut self, at: Vec3) -> EntityId {
        self.spawn_body(
            Transform::from_translation(at),
            RigidBody::new_dynamic(8.0),
            Collider::new_box(Vec3::new(HALF, HALF, HALF)),
        )
    }

    /// A character: a kinematic character controller and a 0.6 m box collider,
    /// no rigid body, centred at `at`.
    fn spawn_character(&mut self, at: Vec3) -> EntityId {
        let placed = Transform::from_translation(at);
        self.world.spawn((
            placed,
            GlobalTransform::new(placed.to_mat4()),
            KinematicCharacterController::default(),
            Collider::new_box(Vec3::new(HALF, HALF, HALF)),
        ))
    }

    /// Asks a character to move by `step` this frame.
    fn walk(&mut self, character: EntityId, step: Vec3) {
        self.world
            .get_mut::<KinematicCharacterController>(character)
            .expect("a character has a controller")
            .desired_translation = step;
    }

    /// Runs `frames` frames, asking the character to move by `step` in each.
    fn walk_for(&mut self, character: EntityId, step: Vec3, frames: usize) {
        for _ in 0..frames {
            self.walk(character, step);
            self.frame();
        }
    }

    /// Where the simulation put an entity this frame.
    fn pose(&self, entity: EntityId) -> (Vec3, Quat) {
        let pose = self
            .world
            .get::<SimulatedTransform>(entity)
            .expect("a dynamic body is simulated, so it has a simulated pose");
        (pose.translation(), pose.rotation())
    }

    /// How fast a simulated body moves, as the write-back last observed it.
    fn velocity(&self, entity: EntityId) -> Vec3 {
        self.world
            .get::<BodyMotion>(entity)
            .expect("a simulated body has a motion")
            .linear
    }

    /// The world matrix the entity is drawn with.
    fn drawn(&self, entity: EntityId) -> Mat4 {
        Mat4::from(
            self.world
                .get::<GlobalTransform>(entity)
                .expect("the entity has a world pose")
                .0,
        )
    }

    /// Where the entity is drawn.
    fn drawn_at(&self, entity: EntityId) -> Vec3 {
        self.world
            .get::<GlobalTransform>(entity)
            .expect("the entity has a world pose")
            .0
            .translation()
    }

    /// Sets an entity's `Transform` the way a script or a gizmo moves it: the
    /// new placement, and the marker that says it was moved.
    fn teleport(&mut self, entity: EntityId, to: Transform) {
        *self
            .world
            .get_mut::<Transform>(entity)
            .expect("the entity has a Transform") = to;
        self.world
            .add_component(entity, Teleported)
            .expect("an entity can be marked as moved");
    }

    fn collider_mut(&mut self, entity: EntityId) -> &mut Collider {
        self.world
            .get_mut::<Collider>(entity)
            .expect("the entity has a collider")
    }

    /// The kinds of every contact reported between `a` and `b`, in order.
    fn contacts_between(&self, a: EntityId, b: EntityId) -> Vec<CollisionKind> {
        self.contacts
            .iter()
            .filter(|c| (c.a == a && c.b == b) || (c.a == b && c.b == a))
            .map(|c| c.kind)
            .collect()
    }

    /// The first point a ray from `origin` along `direction` hits, if any.
    fn hit(&self, origin: Vec3, direction: Vec3) -> Option<Vec3> {
        let ray = khora_core::physics::Ray { origin, direction };
        self.provider
            .lock()
            .expect("provider mutex")
            .cast_ray(&ray, 40.0, true)
            .map(|hit| hit.position)
    }

    /// The height at which a ray cast straight down through `(x, z)` first
    /// hits a collider — where the solver believes a surface is.
    fn top_under(&self, x: f32, z: f32) -> Option<f32> {
        self.hit(Vec3::new(x, 20.0, z), Vec3::new(0.0, -1.0, 0.0))
            .map(|p| p.y)
    }

    /// The x at which a ray coming from +X along `(·, y, z)` first hits.
    fn side_x(&self, y: f32, z: f32) -> Option<f32> {
        self.hit(Vec3::new(20.0, y, z), Vec3::new(-1.0, 0.0, 0.0))
            .map(|p| p.x)
    }

    /// The z at which a ray coming from +Z along `(x, y, ·)` first hits.
    fn side_z(&self, x: f32, y: f32) -> Option<f32> {
        self.hit(Vec3::new(x, y, 20.0), Vec3::new(0.0, 0.0, -1.0))
            .map(|p| p.z)
    }
}

/// `collider` turned into a sensor.
fn sensor(mut collider: Collider) -> Collider {
    collider.is_sensor = true;
    collider
}

/// `collider` with the surface given.
fn surfaced(mut collider: Collider, friction: f32, restitution: f32) -> Collider {
    collider.friction = friction;
    collider.restitution = restitution;
    collider
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

/// The angle, in degrees, of the rotation taking `a` to `b`.
fn angle_between_degrees(a: Quat, b: Quat) -> f32 {
    let dot = a.normalize().dot(b.normalize()).abs().min(1.0);
    (2.0 * dot.acos()).to_degrees()
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

/// Whether `kinds` holds a `Started` followed, later, by a `Stopped`.
fn entered_then_left(kinds: &[CollisionKind]) -> bool {
    kinds
        .iter()
        .position(|k| *k == CollisionKind::Started)
        .is_some_and(|start| kinds[start..].contains(&CollisionKind::Stopped))
}

// ─── Surface ────────────────────────────────────────────────────────────────

/// The highest a crate dropped from 3 m climbs between frames 50 and 150 — it
/// meets the ground at about frame 45, so this is how high it comes back up —
/// when the crate's and the ground's `Collider` both carry `restitution`.
fn rebound_height(restitution: f32) -> f32 {
    let mut rig = Rig::new();
    rig.spawn_ground_with(surfaced(
        Collider::new_box(Vec3::new(40.0, 0.5, 40.0)),
        0.5,
        restitution,
    ));
    let crate_id = rig.spawn_body(
        Transform::from_translation(Vec3::new(0.0, 3.0, 0.0)),
        RigidBody::new_dynamic(8.0),
        surfaced(
            Collider::new_box(Vec3::new(HALF, HALF, HALF)),
            0.5,
            restitution,
        ),
    );
    rig.frames(50);
    let mut peak = f32::MIN;
    for _ in 50..150 {
        rig.frame();
        peak = peak.max(rig.pose(crate_id).0.y);
    }
    peak
}

/// **A collider's bounce is its `Collider.restitution`.** The sync read the
/// surface from a separate `PhysicsMaterial` and fell back to restitution 0,
/// so the field the inspector and scripts write did nothing: a crate and a
/// ground both authored fully elastic landed dead.
///
/// Dropped from 3 m (centre), an elastic pair comes back up most of the way;
/// a dead pair stays on the ground, centre at 0.3.
#[test]
fn a_crate_and_ground_with_full_restitution_bounce_back_up_where_dead_ones_stay_down() {
    let elastic = rebound_height(1.0);
    let dead = rebound_height(0.0);
    assert!(
        elastic > 1.5,
        "restitution 1.0 on both colliders brings a crate dropped from 3 m back above 1.5 m; \
         its highest point after the impact was {elastic:.3} (a dead landing stays at 0.3)"
    );
    assert!(
        dead < 0.5,
        "restitution 0.0 on both colliders does not bounce; the crate climbed back to {dead:.3}"
    );
}

/// How far a 0.8 × 0.2 × 0.8 m slab launched at 4 m/s along +X slides in one
/// second, the slab's and the ground's `Collider` both carrying `friction`.
fn slide_distance(friction: f32) -> f32 {
    let mut rig = Rig::new();
    rig.spawn_ground_with(surfaced(
        Collider::new_box(Vec3::new(40.0, 0.5, 40.0)),
        friction,
        0.0,
    ));
    let mut body = RigidBody::new_dynamic(1.0);
    body.initial_velocity = Vec3::new(4.0, 0.0, 0.0);
    let slab = rig.spawn_body(
        Transform::from_translation(Vec3::new(0.0, 0.101, 0.0)),
        body,
        surfaced(Collider::new_box(Vec3::new(0.4, 0.1, 0.4)), friction, 0.0),
    );
    rig.frames(60);
    rig.pose(slab).0.x
}

/// **A collider's grip is its `Collider.friction`.** Every collider used to
/// slide with friction 0.5 whatever it said — the value came from a
/// `PhysicsMaterial` component nothing attached.
///
/// At 4 m/s: frictionless, the slab covers about 4 m in a second; at friction
/// 1.0 it stops after about 0.8 m (at 0.5, the old constant, 1.6 m).
#[test]
fn a_slab_slides_far_on_frictionless_ground_and_stops_short_on_rough_ground() {
    let slick = slide_distance(0.0);
    let rough = slide_distance(1.0);
    assert!(
        slick > 3.5,
        "friction 0.0 on both colliders lets the slab coast about 4 m in a second; it covered \
         {slick:.3} m"
    );
    assert!(
        rough < 1.2,
        "friction 1.0 on both colliders stops the slab after about 0.8 m; it covered {rough:.3} m"
    );
}

// ─── Sensors ────────────────────────────────────────────────────────────────

/// **A sensor never pushes.** A crate dropped onto a standalone sensor box
/// falls through it to the ground. The flag never reached the provider, so a
/// trigger volume was a solid floor.
///
/// Sensor centred at y = 1, half-height 0.25: a crate resting on it would sit
/// at 1.55; through it, the crate rests on the ground at 0.3.
#[test]
fn a_dynamic_body_falls_through_a_standalone_sensor() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    rig.spawn_standalone(
        Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
        sensor(Collider::new_box(Vec3::new(2.0, 0.25, 2.0))),
    );
    let crate_id = rig.spawn_crate(Vec3::new(0.0, 3.0, 0.0));

    rig.frames(120);

    let (at, _) = rig.pose(crate_id);
    assert!(
        (at.y - HALF).abs() < 0.03,
        "the crate falls through the sensor onto the ground, centre at {HALF}; it is at {at:?} \
         (1.55 means the sensor held it up)"
    );
}

/// A sensor on a **child** of a dynamic body does not hold the body up: the
/// body's own box lands on the ground, as if the sensor sphere were not there.
///
/// Crate (half 0.3) with a sensor sphere of radius 1 around it: on the
/// ground the crate's centre is at 0.3; held up by the sphere it would be at 1.
#[test]
fn a_sensor_attached_to_a_dynamic_body_does_not_hold_it_up() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let body = rig.spawn_crate(Vec3::new(0.0, 2.0, 0.0));
    rig.world.spawn((
        Transform::default(),
        GlobalTransform::identity(),
        Parent(body),
        sensor(Collider::new_sphere(1.0)),
    ));

    rig.frames(120);

    let (at, _) = rig.pose(body);
    assert!(
        (at.y - HALF).abs() < 0.03,
        "the crate's own box rests on the ground, centre at {HALF}; it is at {at:?} \
         (1.0 means the sensor sphere held it up)"
    );
}

/// **A sensor weighs nothing.** A slab resting flat on the ground with a
/// sensor sphere held 3 m out to one side stays flat: were the sphere given
/// mass, the body's centre of mass would sit beyond the slab's edge and it
/// would tip over.
///
/// Slab of half-extents (0.5, 0.1, 0.5), sensor sphere of radius 0.5 at local
/// (3, 1.5, 0). With density 1 the sphere outweighs the slab 2.6 to 1.
#[test]
fn a_sensor_attached_off_centre_adds_no_weight_to_its_body() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let at = Vec3::new(0.0, 0.101, 0.0);
    let slab = rig.spawn_body(
        Transform::from_translation(at),
        RigidBody::new_dynamic(0.1),
        Collider::new_box(Vec3::new(0.5, 0.1, 0.5)),
    );
    rig.world.spawn((
        Transform::from_translation(Vec3::new(3.0, 1.5, 0.0)),
        GlobalTransform::identity(),
        Parent(slab),
        sensor(Collider::new_sphere(0.5)),
    ));

    rig.frames(120);

    let (now, rotation) = rig.pose(slab);
    let tilt = tilt_degrees(rotation);
    assert!(
        tilt < 3.0,
        "a weightless sensor leaves the slab flat; it leans {tilt:.2}° (it tipped toward the \
         sensor's side)"
    );
    assert!(
        (now - Vec3::new(at.x, 0.1, at.z)).length() < 0.05,
        "the slab stays where it rests, centre at (0, 0.1, 0); it is at {now:?}"
    );
}

/// A crate falling through a standalone sensor box placed high above the
/// ground, with or without `ActiveEvents` on either: the contacts the channel
/// carried between the two.
fn contacts_of_a_crate_falling_through_a_sensor(with_active_events: bool) -> Vec<CollisionKind> {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let trigger = rig.spawn_standalone(
        Transform::from_translation(Vec3::new(0.0, 3.0, 0.0)),
        sensor(Collider::new_box(Vec3::new(1.0, 0.25, 1.0))),
    );
    let crate_id = rig.spawn_crate(Vec3::new(0.0, 5.0, 0.0));
    if with_active_events {
        for entity in [trigger, crate_id] {
            rig.world
                .add_component(entity, ActiveEvents)
                .expect("ActiveEvents can be added");
        }
    }

    rig.frames(120);

    rig.contacts_between(trigger, crate_id)
}

/// **A sensor reports, whoever asked.** A crate falling through a standalone
/// sensor raises `Started` as it enters and `Stopped` as it leaves, on the
/// collision channel — with no `ActiveEvents` anywhere. A sensor that says
/// nothing has no purpose, so it never needs to be told to speak.
#[test]
fn a_body_falling_through_a_standalone_sensor_reports_entering_and_leaving() {
    let kinds = contacts_of_a_crate_falling_through_a_sensor(false);
    assert!(
        entered_then_left(&kinds),
        "a crate passing through a sensor with no ActiveEvents anywhere is reported entering \
         (Started) then leaving (Stopped); the channel carried {kinds:?}"
    );
}

/// The same fall with `ActiveEvents` on both entities reports the same pair —
/// asking for events does not change what a sensor reports.
#[test]
fn a_body_falling_through_a_sensor_with_active_events_reports_entering_and_leaving() {
    let kinds = contacts_of_a_crate_falling_through_a_sensor(true);
    assert!(
        entered_then_left(&kinds),
        "a crate passing through a sensor with ActiveEvents on both is reported entering \
         (Started) then leaving (Stopped); the channel carried {kinds:?}"
    );
}

/// A character at (0, 1, 0) walking 5 m along +X at 3 m/s towards a standalone
/// sensor wall at x = 2 (half-extents 0.25, 1, 1). Returns the rig, the
/// character and the wall after the walk.
fn a_character_walks_at_a_sensor_wall() -> (Rig, EntityId, EntityId) {
    let mut rig = Rig::new();
    let wall = rig.spawn_standalone(
        Transform::from_translation(Vec3::new(2.0, 1.0, 0.0)),
        sensor(Collider::new_box(Vec3::new(0.25, 1.0, 1.0))),
    );
    let character = rig.spawn_character(Vec3::new(0.0, 1.0, 0.0));
    rig.frame();

    rig.walk_for(character, Vec3::new(0.05, 0.0, 0.0), 100);

    (rig, character, wall)
}

/// **A character walks through a sensor.** Its controller's query does not
/// count sensors as obstacles: a trigger volume in a doorway does not close
/// the door.
///
/// A 5 m walk from x = 0 ends at x = 5; a solid wall would stop the character
/// against its face, near x = 1.45.
#[test]
fn a_character_walks_through_a_sensor_wall() {
    let (rig, character, _) = a_character_walks_at_a_sensor_wall();
    let at = rig.drawn_at(character);
    assert!(
        at.x > 4.5,
        "the character walks 5 m through the sensor wall to x ≈ 5; it is drawn at {at:?} \
         (≈ 1.45 means the wall stopped it)"
    );
}

/// A character entering a standalone sensor is reported like a body: `Started`
/// as it goes in, `Stopped` as it comes out — though neither has a rigid body
/// and neither carries `ActiveEvents`.
#[test]
fn a_character_walking_through_a_standalone_sensor_reports_entering_and_leaving() {
    let (rig, character, wall) = a_character_walks_at_a_sensor_wall();
    let kinds = rig.contacts_between(character, wall);
    assert!(
        entered_then_left(&kinds),
        "a character walking through a sensor is reported entering (Started) then leaving \
         (Stopped); the channel carried {kinds:?}"
    );
}

/// **Toggling `is_sensor` while running takes effect the next frame.** A crate
/// resting on a solid platform starts falling the moment the platform is made
/// a sensor, and comes to rest on the ground below.
///
/// Platform centred at y = 1, half-height 0.25: the crate rests at 1.55, then
/// falls to 0.3. Three frames after the toggle it is already falling at about
/// 0.5 m/s.
#[test]
fn a_platform_made_a_sensor_while_running_lets_go_of_what_rests_on_it() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let platform = rig.spawn_standalone(
        Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
        Collider::new_box(Vec3::new(2.0, 0.25, 2.0)),
    );
    let crate_id = rig.spawn_crate(Vec3::new(0.0, 1.6, 0.0));
    rig.frames(60);
    let (rested, _) = rig.pose(crate_id);
    assert!(
        (rested.y - 1.55).abs() < 0.03,
        "precondition: the crate rests on the solid platform, centre at 1.55; it is at {rested:?}"
    );

    rig.collider_mut(platform).is_sensor = true;
    rig.frames(3);

    let falling = rig.velocity(crate_id).y;
    assert!(
        falling < -0.2,
        "three frames after the platform became a sensor the crate is falling (≈ −0.5 m/s); its \
         vertical speed is {falling:.3} m/s"
    );
    rig.frames(90);
    let (landed, _) = rig.pose(crate_id);
    assert!(
        (landed.y - HALF).abs() < 0.03,
        "the crate comes to rest on the ground, centre at {HALF}; it is at {landed:?}"
    );
}

/// And the other way: a sensor platform lets a first crate through, then,
/// made solid, holds up a second one dropped on it.
#[test]
fn a_sensor_made_solid_while_running_holds_up_what_falls_on_it_afterwards() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let platform = rig.spawn_standalone(
        Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
        sensor(Collider::new_box(Vec3::new(2.0, 0.25, 2.0))),
    );
    let first = rig.spawn_crate(Vec3::new(0.0, 3.0, 0.0));
    rig.frames(120);
    let (through, _) = rig.pose(first);
    assert!(
        (through.y - HALF).abs() < 0.03,
        "the first crate falls through the sensor platform onto the ground, centre at {HALF}; it \
         is at {through:?}"
    );

    rig.collider_mut(platform).is_sensor = false;
    let second = rig.spawn_crate(Vec3::new(1.0, 3.0, 1.0));
    rig.frames(120);

    let (held, _) = rig.pose(second);
    assert!(
        (held.y - 1.55).abs() < 0.03,
        "the platform, solid now, holds the second crate up, centre at 1.55; it is at {held:?}"
    );
}

// ─── Scale ──────────────────────────────────────────────────────────────────

/// **A collider is authored in its entity's units.** A crate of half-extent
/// 0.3 drawn at scale 2 is a 1.2 m crate, and rests with its centre 0.6 above
/// the ground — not at 0.3, sunk halfway into the floor, as its unscaled
/// collider used to put it.
#[test]
fn a_crate_drawn_at_scale_two_rests_at_its_drawn_height() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let crate_id = rig.spawn_body(
        Transform::new(
            Vec3::new(0.0, 3.0, 0.0),
            Quat::IDENTITY,
            Vec3::new(2.0, 2.0, 2.0),
        ),
        RigidBody::new_dynamic(8.0),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    );

    rig.frames(120);

    let (at, _) = rig.pose(crate_id);
    assert!(
        (at.y - 2.0 * HALF).abs() < 0.03,
        "a crate of half-extent 0.3 at scale 2 rests with its centre at 0.6; it is at {at:?}"
    );
}

/// A box scales per axis, by the absolute value of each: half-extents 0.5 at
/// scale (−2, 3, 4) are (1, 1.5, 2) — a mirror does not shrink a box to
/// nothing or turn it inside out.
#[test]
fn a_scaled_box_reaches_the_provider_scaled_per_axis() {
    let mut rig = Rig::new();
    rig.spawn_standalone(
        Transform::new(Vec3::ZERO, Quat::IDENTITY, Vec3::new(-2.0, 3.0, 4.0)),
        Collider::new_box(Vec3::new(0.5, 0.5, 0.5)),
    );

    rig.frames(2);

    let top = rig.top_under(0.0, 0.0);
    let side_x = rig.side_x(0.0, 0.0);
    let side_z = rig.side_z(0.0, 0.0);
    assert!(
        top.is_some_and(|y| (y - 1.5).abs() < 0.01),
        "half-height 0.5 at scale 3 puts the box's top at 1.5; the ray hit {top:?}"
    );
    assert!(
        side_x.is_some_and(|x| (x - 1.0).abs() < 0.01),
        "half-width 0.5 at scale −2 puts the box's +X face at 1.0; the ray hit {side_x:?}"
    );
    assert!(
        side_z.is_some_and(|z| (z - 2.0).abs() < 0.01),
        "half-depth 0.5 at scale 4 puts the box's +Z face at 2.0; the ray hit {side_z:?}"
    );
}

/// A sphere stays a sphere: its radius scales by the largest absolute axis,
/// whichever axis that is. Radius 0.5 at scales (−3, 1, 0.5), (0.5, 3, 1) and
/// (1, 0.5, 3) is radius 1.5 each time.
#[test]
fn a_scaled_sphere_takes_its_largest_axis_as_its_radius_scale() {
    let mut rig = Rig::new();
    let placed = [
        (-10.0, Vec3::new(-3.0, 1.0, 0.5)),
        (0.0, Vec3::new(0.5, 3.0, 1.0)),
        (10.0, Vec3::new(1.0, 0.5, 3.0)),
    ];
    for (x, scale) in placed {
        rig.spawn_standalone(
            Transform::new(Vec3::new(x, 0.0, 0.0), Quat::IDENTITY, scale),
            Collider::new_sphere(0.5),
        );
    }

    rig.frames(2);

    for (x, scale) in placed {
        let top = rig.top_under(x, 0.0);
        assert!(
            top.is_some_and(|y| (y - 1.5).abs() < 0.01),
            "radius 0.5 at scale {scale:?} is radius 1.5, top at 1.5; the ray at x = {x} hit \
             {top:?}"
        );
        let side = rig.side_z(x, 0.0);
        assert!(
            side.is_some_and(|z| (z - 1.5).abs() < 0.01),
            "radius 0.5 at scale {scale:?} is radius 1.5 sideways too; the ray along z at \
             x = {x} hit {side:?}"
        );
    }
}

/// A capsule's half-height scales by the y axis and its radius by the larger
/// of x and z. Half-height 0.5, radius 0.25 at scale (2, 3, 4) or (4, 3, 2):
/// half-height 1.5, radius 1.0 — top at 2.5, sides at 1.0.
#[test]
fn a_scaled_capsule_takes_its_height_from_y_and_its_radius_from_x_and_z() {
    let mut rig = Rig::new();
    let placed = [
        (-10.0, Vec3::new(2.0, 3.0, 4.0)),
        (10.0, Vec3::new(4.0, 3.0, 2.0)),
    ];
    for (x, scale) in placed {
        rig.spawn_standalone(
            Transform::new(Vec3::new(x, 0.0, 0.0), Quat::IDENTITY, scale),
            Collider {
                shape: ColliderShape::Capsule(0.5, 0.25),
                ..Collider::default()
            },
        );
    }

    rig.frames(2);

    for (x, scale) in placed {
        let top = rig.top_under(x, 0.0);
        assert!(
            top.is_some_and(|y| (y - 2.5).abs() < 0.01),
            "half-height 0.5 · 3 + radius 0.25 · 4 puts the capsule's top at 2.5 at scale \
             {scale:?}; the ray at x = {x} hit {top:?}"
        );
        let side = rig.side_z(x, 0.0);
        assert!(
            side.is_some_and(|z| (z - 1.0).abs() < 0.01),
            "radius 0.25 · max(|x|, |z|) = 1.0 sideways at scale {scale:?}; the ray along z at \
             x = {x} hit {side:?}"
        );
    }
    let side_x = rig.side_x(0.0, 0.0);
    assert!(
        side_x.is_some_and(|hit| (hit - 11.0).abs() < 0.01),
        "the capsule at x = 10 has radius 1.0 along x too, face at 11; the ray along x hit \
         {side_x:?}"
    );
}

/// **A scale changed while running reaches the provider.** A crate resting at
/// scale 1 is moved by a script to (0, 1.5, 0) at scale 2: it falls and rests
/// as the 1.2 m crate it now is, centre at 0.6.
#[test]
fn a_crate_rescaled_while_running_rests_at_its_new_height() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let crate_id = rig.spawn_crate(Vec3::new(0.0, HALF + 0.01, 0.0));
    rig.frames(60);
    let (rested, _) = rig.pose(crate_id);
    assert!(
        (rested.y - HALF).abs() < 0.03,
        "precondition: the crate rests at {HALF}; it is at {rested:?}"
    );

    rig.teleport(
        crate_id,
        Transform::new(
            Vec3::new(0.0, 1.5, 0.0),
            Quat::IDENTITY,
            Vec3::new(2.0, 2.0, 2.0),
        ),
    );
    rig.frames(120);

    let (at, _) = rig.pose(crate_id);
    assert!(
        (at.y - 2.0 * HALF).abs() < 0.03,
        "rescaled to 2, the crate rests with its centre at 0.6; it is at {at:?}"
    );
}

/// A standalone box whose entity is rescaled while running is resized in the
/// provider: half-extent 0.5 at scale 3, top at 1.5.
#[test]
fn a_standalone_collider_rescaled_while_running_is_resized() {
    let mut rig = Rig::new();
    let entity = rig.spawn_standalone(
        Transform::default(),
        Collider::new_box(Vec3::new(0.5, 0.5, 0.5)),
    );
    rig.frames(2);

    rig.world
        .get_mut::<Transform>(entity)
        .expect("the entity has a Transform")
        .scale = Vec3::new(3.0, 3.0, 3.0);
    rig.frames(2);

    let top = rig.top_under(0.0, 0.0);
    assert!(
        top.is_some_and(|y| (y - 1.5).abs() < 0.01),
        "half-extent 0.5 at the new scale 3 puts the top at 1.5; the ray hit {top:?}"
    );
}

/// **A shape edited while running reaches the provider.** The sync updated a
/// collider's pose and surface every frame but never its shape, so an
/// inspector edit to a half-extent did nothing until the collider was rebuilt.
#[test]
fn a_collider_shape_edited_while_running_reaches_the_provider() {
    let mut rig = Rig::new();
    let entity = rig.spawn_standalone(
        Transform::default(),
        Collider::new_box(Vec3::new(0.5, 0.5, 0.5)),
    );
    rig.frames(2);

    rig.collider_mut(entity).shape = ColliderShape::Box(Vec3::new(1.0, 1.0, 1.0));
    rig.frames(2);

    let top = rig.top_under(0.0, 0.0);
    assert!(
        top.is_some_and(|y| (y - 1.0).abs() < 0.01),
        "the box edited to half-extent 1.0 has its top at 1.0; the ray hit {top:?}"
    );
}

// ─── Release ────────────────────────────────────────────────────────────────

/// A crate dropped from `placed` onto the ground and left 90 frames to settle,
/// then stripped of its `RigidBody`. Returns the rig, the crate, and the world
/// matrix it was drawn with on the frame before the body went.
fn a_settled_crate_loses_its_body(placed: Transform, collider: Collider) -> (Rig, EntityId, Mat4) {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let crate_id = rig.spawn_body(placed, RigidBody::new_dynamic(8.0), collider);
    rig.frames(90);
    let before = rig.drawn(crate_id);
    rig.world
        .remove_component::<RigidBody>(crate_id)
        .expect("the body can be removed");
    (rig, crate_id, before)
}

/// The crate the release tests drop: from (1, 2, −1), turned 30° about Y.
fn a_turned_crate() -> Transform {
    Transform::new(
        Vec3::new(1.0, 2.0, -1.0),
        Quat::from_axis_angle(Vec3::Y, 30f32.to_radians()),
        Vec3::new(1.0, 1.0, 1.0),
    )
}

/// **Nothing simulates an entity once its body is gone.** The frame after its
/// `RigidBody` is removed, the entity has no `SimulatedTransform` and no
/// `BodyMotion`: both say "something simulates this", and nothing does.
#[test]
fn a_body_whose_rigid_body_is_removed_is_no_longer_simulated() {
    let (mut rig, crate_id, _) = a_settled_crate_loses_its_body(
        a_turned_crate(),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    );

    rig.frame();

    assert!(
        rig.world.get::<SimulatedTransform>(crate_id).is_none(),
        "a crate with no rigid body has no simulated pose the frame after"
    );
    assert!(
        rig.world.get::<BodyMotion>(crate_id).is_none(),
        "a crate with no rigid body has no motion the frame after"
    );
}

/// **A released body stays where the simulation left it**, and its
/// `Transform` now says so. It is not snapped back to where it was authored
/// (2 m up), and its `GlobalTransform` does not jump.
#[test]
fn a_released_body_stays_drawn_where_it_last_was_and_its_transform_says_so() {
    let (mut rig, crate_id, before) = a_settled_crate_loses_its_body(
        a_turned_crate(),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    );

    for frame in 1..=5 {
        rig.frame();
        let gap = matrix_gap(rig.drawn(crate_id), before);
        assert!(
            gap < 1e-3,
            "the released crate is drawn where it last was; its world matrix moved by {gap} at \
             frame {frame} after the release"
        );
    }

    let drawn = rig
        .world
        .get::<GlobalTransform>(crate_id)
        .expect("the crate has a world pose")
        .0;
    let placed = *rig
        .world
        .get::<Transform>(crate_id)
        .expect("the crate has a Transform");
    assert!(
        (placed.translation - drawn.translation()).length() < 1e-3,
        "the released crate's Transform is where it is drawn, {:?}; it says {:?} (2 m up is \
         where it was authored)",
        drawn.translation(),
        placed.translation
    );
    let turn = angle_between_degrees(placed.rotation, drawn.rotation());
    assert!(
        turn < 0.1,
        "the released crate's Transform is turned as it is drawn; off by {turn:.3}°"
    );
}

/// A released body keeps its authored scale: its `Transform` is handed back at
/// the pose it is drawn at, at the size it was authored.
#[test]
fn a_released_scaled_body_keeps_its_authored_scale() {
    let scale = Vec3::new(2.0, 2.0, 2.0);
    let (mut rig, crate_id, _) = a_settled_crate_loses_its_body(
        Transform::new(Vec3::new(0.0, 3.0, 0.0), Quat::IDENTITY, scale),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    );

    rig.frames(3);

    let placed = *rig
        .world
        .get::<Transform>(crate_id)
        .expect("the crate has a Transform");
    let drawn = rig
        .world
        .get::<GlobalTransform>(crate_id)
        .expect("the crate has a world pose")
        .0;
    assert!(
        (placed.scale - scale).length() < 1e-3,
        "the released crate keeps its authored scale 2; its Transform says {:?}",
        placed.scale
    );
    assert!(
        (drawn.scale() - scale).length() < 1e-3,
        "the released crate is still drawn at scale 2; it is drawn at {:?}",
        drawn.scale()
    );
    assert!(
        (placed.translation - drawn.translation()).length() < 1e-3,
        "the released crate's Transform is where it is drawn, {:?}; it says {:?}",
        drawn.translation(),
        placed.translation
    );
}

/// **A released body answers to its `Transform` again.** Moved by setting its
/// `Transform` — no marker, nothing simulates it any more — it is drawn at
/// the new place. The stale simulated pose used to win every frame, so the
/// entity was frozen where its body had been.
#[test]
fn a_released_body_moves_when_its_transform_is_moved() {
    let (mut rig, crate_id, _) = a_settled_crate_loses_its_body(
        a_turned_crate(),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    );
    rig.frames(3);

    let to = Vec3::new(4.0, 2.0, 0.0);
    rig.world
        .get_mut::<Transform>(crate_id)
        .expect("the crate has a Transform")
        .translation = to;
    rig.frames(2);

    let at = rig.drawn_at(crate_id);
    assert!(
        (at - to).length() < 1e-3,
        "the released crate moved by its Transform is drawn at {to:?}; it is drawn at {at:?}"
    );
}

/// A released body moved the way a script or a gizmo moves it — `Transform`
/// and `Teleported` — stays where it was put, frames later.
#[test]
fn a_released_body_moved_by_a_script_stays_where_it_was_put() {
    let (mut rig, crate_id, _) = a_settled_crate_loses_its_body(
        a_turned_crate(),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    );
    rig.frames(3);

    let to = Vec3::new(4.0, 2.0, 0.0);
    rig.teleport(crate_id, Transform::from_translation(to));
    rig.frames(5);

    let at = rig.drawn_at(crate_id);
    assert!(
        (at - to).length() < 1e-3,
        "the released crate moved to {to:?} is still there five frames later; it is drawn at \
         {at:?}"
    );
}

/// **A released child is handed back in its parent's frame**, and follows its
/// parent afterwards.
///
/// Parent at (5, 0, 0), turned 90° about Y, scale 2; the child body, local
/// (0.5, 1, 0) — world (5, 2, −1) — falls to the ground. Released, its
/// `Transform` composed with its parent's world pose is exactly the pose it is
/// drawn at, at its authored scale 1; then the parent moves 3 m along +Z and
/// the child moves with it.
#[test]
fn a_released_child_is_handed_back_in_its_parents_frame_and_follows_its_parent() {
    let mut rig = Rig::new();
    rig.spawn_ground();
    let parent_placed = Transform::new(
        Vec3::new(5.0, 0.0, 0.0),
        Quat::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2),
        Vec3::new(2.0, 2.0, 2.0),
    );
    let parent = rig
        .world
        .spawn((parent_placed, GlobalTransform::new(parent_placed.to_mat4())));
    let child = rig.world.spawn((
        Transform::from_translation(Vec3::new(0.5, 1.0, 0.0)),
        GlobalTransform::identity(),
        Parent(parent),
        RigidBody::new_dynamic(8.0),
        Collider::new_box(Vec3::new(0.15, 0.15, 0.15)),
    ));
    rig.frames(120);
    let rested = rig.drawn(child);
    assert!(
        rig.drawn_at(child).y < 1.0,
        "precondition: the child body fell from y = 2; it is drawn at {:?}",
        rig.drawn_at(child)
    );

    rig.world
        .remove_component::<RigidBody>(child)
        .expect("the body can be removed");
    rig.frames(3);

    let gap = matrix_gap(rig.drawn(child), rested);
    assert!(
        gap < 1e-3,
        "the released child is drawn where it last was; its world matrix moved by {gap}"
    );
    let local = *rig
        .world
        .get::<Transform>(child)
        .expect("the child has a Transform");
    let composed = rig.drawn(parent) * local.to_mat4();
    let gap = matrix_gap(composed, rested);
    assert!(
        gap < 1e-3,
        "the child's Transform, composed with its parent's world pose, is the pose it is drawn \
         at; they differ by {gap} (its Transform says {local:?})"
    );
    assert!(
        (local.scale - Vec3::new(1.0, 1.0, 1.0)).length() < 1e-3,
        "the released child keeps its authored scale 1; its Transform says {:?}",
        local.scale
    );

    let before = rig.drawn_at(child);
    let parent_now = rig
        .world
        .get_mut::<Transform>(parent)
        .expect("the parent has a Transform");
    parent_now.translation = parent_now.translation + Vec3::new(0.0, 0.0, 3.0);
    rig.frames(2);

    let after = rig.drawn_at(child);
    let expected = before + Vec3::new(0.0, 0.0, 3.0);
    assert!(
        (after - expected).length() < 1e-3,
        "the parent moved 3 m along +Z and the released child moves with it, to {expected:?}; \
         it is drawn at {after:?}"
    );
}

/// **A body given back starts where its entity is.** Released, moved by its
/// `Transform`, then given a `RigidBody` again: the new body begins at the
/// moved place, not where the old body was.
#[test]
fn a_body_given_its_rigid_body_back_starts_where_it_is() {
    let (mut rig, crate_id, _) = a_settled_crate_loses_its_body(
        a_turned_crate(),
        Collider::new_box(Vec3::new(HALF, HALF, HALF)),
    );
    rig.frames(3);
    let to = Vec3::new(3.0, 2.0, 0.0);
    rig.world
        .get_mut::<Transform>(crate_id)
        .expect("the crate has a Transform")
        .translation = to;
    rig.frames(2);

    rig.world
        .add_component(crate_id, RigidBody::new_dynamic(8.0))
        .expect("a body can be added back");
    rig.frame();

    let (at, _) = rig.pose(crate_id);
    assert!(
        (at - to).length() < 0.02,
        "the body given back starts at {to:?}, where its entity was moved; after one frame it is \
         at {at:?}"
    );
}

/// A character is simulated without a rigid body: one whose `RigidBody` is
/// removed stays a character — it keeps its simulated pose every frame and
/// keeps walking.
#[test]
fn a_character_whose_rigid_body_is_removed_stays_a_character() {
    let mut rig = Rig::new();
    let character = rig.spawn_character(Vec3::new(0.0, 1.0, 0.0));
    let mut body = RigidBody::new_dynamic(1.0);
    body.body_type = khora_core::physics::BodyType::Kinematic;
    rig.world
        .add_component(character, body)
        .expect("a body can be added");
    rig.frames(3);
    rig.world
        .remove_component::<RigidBody>(character)
        .expect("the body can be removed");

    for frame in 1..=40 {
        rig.walk(character, Vec3::new(0.05, 0.0, 0.0));
        rig.frame();
        assert!(
            rig.world.get::<SimulatedTransform>(character).is_some(),
            "a character keeps its simulated pose with no rigid body, at frame {frame} after the \
             body went"
        );
    }
    // One frame with nothing asked, so the drawn pose catches up with the
    // last step: propagation runs before the write-back of the same frame.
    rig.frame();
    let at = rig.drawn_at(character);
    assert!(
        (at - Vec3::new(2.0, 1.0, 0.0)).length() < 0.05,
        "the character walked 2 m along +X to (2, 1, 0); it is drawn at {at:?}"
    );
}

// ─── Character teleport ─────────────────────────────────────────────────────

/// A character that walked to x = 0.5 and is then moved by a script to
/// (10, 1, 5), with nothing more asked of its controller.
fn a_teleported_character() -> (Rig, EntityId, Vec3) {
    let mut rig = Rig::new();
    let character = rig.spawn_character(Vec3::new(0.0, 1.0, 0.0));
    rig.walk_for(character, Vec3::new(0.05, 0.0, 0.0), 10);

    let to = Vec3::new(10.0, 1.0, 5.0);
    rig.teleport(character, Transform::from_translation(to));
    (rig, character, to)
}

/// **A character can be teleported.** Moved by its `Transform` with
/// `Teleported`, it is at the new place frames later. Its next pose was built
/// from its old simulated one, so it snapped back to where it had walked.
#[test]
fn a_teleported_character_stays_where_it_was_put() {
    let (mut rig, character, to) = a_teleported_character();

    for frame in 1..=10 {
        rig.frame();
        let at = rig.drawn_at(character);
        assert!(
            (at - to).length() < 0.01,
            "the character moved to {to:?} stays there; at frame {frame} after the move it is \
             drawn at {at:?}"
        );
    }
    let (simulated, _) = rig.pose(character);
    assert!(
        (simulated - to).length() < 0.01,
        "the character's simulated pose is the place it was moved to, {to:?}; it is {simulated:?}"
    );
}

/// After the teleport the character walks on from where it was put: 1 m along
/// +X from (10, 1, 5) ends at (11, 1, 5).
#[test]
fn a_teleported_character_walks_on_from_where_it_was_put() {
    let (mut rig, character, to) = a_teleported_character();
    rig.frames(2);

    rig.walk_for(character, Vec3::new(0.05, 0.0, 0.0), 20);
    // One frame with nothing asked, so the drawn pose catches up with the
    // last step: propagation runs before the write-back of the same frame.
    rig.frame();

    let at = rig.drawn_at(character);
    let expected = to + Vec3::new(1.0, 0.0, 0.0);
    assert!(
        (at - expected).length() < 0.05,
        "the teleported character walks 1 m on to {expected:?}; it is drawn at {at:?}"
    );
}
