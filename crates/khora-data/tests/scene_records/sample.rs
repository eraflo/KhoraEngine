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

//! A world holding every kind of component a scene records.

use khora_core::asset::{AssetUUID, StandardMaterial};
use khora_core::ecs::entity::EntityId;
use khora_core::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};
use khora_core::physics::{BodyType, ColliderShape};
use khora_core::script::{
    FrozenFrame, FrozenMachine, FrozenValue, PendingBody, PendingSequence, ScriptSnapshot,
    ScriptValue, SuspendedMachine, TimerRemaining,
};
use khora_core::ui::types::{UiFlexDirection, UiRect, UiVal};
use khora_data::ecs::{
    ActiveEvents, AudioListener, AudioSource, BodyMotion, Camera, Collider, GlobalTransform,
    KinematicCharacterController, Light, MaterialRef, MeshRef, Name, PhysicsMaterial,
    ProceduralMeshKind, RigidBody, Script, Tag, Transform, World,
};
use khora_data::ui::{
    UiBorder, UiColor, UiImage, UiInteraction, UiInteractionState, UiNode, UiStyle, UiText,
    UiTransform,
};

/// The entities of the sample world, by role.
pub struct Sample {
    pub root: EntityId,
    pub child: EntityId,
    pub grandchild: EntityId,
    pub body: EntityId,
    pub guard: EntityId,
    pub panel: EntityId,
}

/// A behavior instance part-way through a sequence, frozen with `target` in
/// one of its registers.
pub fn suspended_snapshot(target: EntityId) -> ScriptSnapshot {
    ScriptSnapshot {
        fields: vec![("speed".into(), ScriptValue::Float(3.0))],
        state: Some("Patrol".into()),
        state_fields: vec![("waypoint".into(), ScriptValue::Int(2))],
        timers: vec![TimerRemaining {
            repeating: true,
            interval: 0.5,
            state: None,
            ordinal: 0,
            remaining: Some(0.25),
        }],
        pending: Some(PendingSequence {
            fingerprint: 0xfeed_beef_dead_c0de,
            remaining: 1.5,
            machine: SuspendedMachine::Frozen(FrozenMachine {
                body: PendingBody::Update,
                registers: vec![
                    FrozenValue::Int(-7),
                    FrozenValue::Entity(target),
                    FrozenValue::Float(1.25),
                    FrozenValue::Literal("hello".into()),
                    FrozenValue::Null,
                ],
                frames: vec![
                    FrozenFrame {
                        function: "on_update".into(),
                        base: 0,
                        return_pc: 0,
                        result: 0,
                    },
                    FrozenFrame {
                        function: "wind_up".into(),
                        base: 4,
                        return_pc: 12,
                        result: 1,
                    },
                ],
                program_counter: 42,
            }),
        }),
    }
}

/// Adds `component` to `entity`, which must accept it.
fn add<C: khora_data::ecs::Component>(world: &mut World, entity: EntityId, component: C) {
    world
        .add_component(entity, component)
        .unwrap_or_else(|e| panic!("cannot add {}: {e:?}", std::any::type_name::<C>()));
}

/// A world carrying at least one of every component a scene records — and a
/// few the engine computes, which a scene must leave out.
///
/// Entities are spawned and despawned first so that the sample's entity ids are
/// not the ones a fresh world hands out: a load that kept raw ids instead of
/// remapping them would then point at the wrong entities.
pub fn sample_world() -> (World, Sample) {
    let mut world = World::new();
    let wall = AssetUUID::new_v5("textures/wall.png");
    let font = AssetUUID::new_v5("fonts/body.ttf");

    let scratch: Vec<EntityId> = (0..5).map(|_| world.spawn(Transform::identity())).collect();
    for entity in scratch {
        world.despawn(entity);
    }
    let filler = world.spawn(Transform::identity());
    world.despawn(filler);

    let mut tags = Tag::new();
    tags.insert("enemy");
    tags.insert("boss");
    let root = world.spawn((
        Transform::new(
            Vec3::new(1.5, -2.0, 0.1),
            Quaternion::from_axis_angle(Vec3::Y, 0.75),
            Vec3::new(2.0, 2.0, 2.0),
        ),
        // Derived: recomputed by the engine, never recorded.
        GlobalTransform::identity(),
        Name::new("Root"),
        tags,
        Camera::new_perspective(std::f32::consts::FRAC_PI_3, 1.5, 0.05, 500.0),
    ));
    add(&mut world, root, AudioListener);

    let child = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
        Name::new("Child"),
        Light::point(),
        MeshRef::procedural(ProceduralMeshKind::Sphere, [0.5, 16.0, 16.0, 0.0]),
    ));
    add(
        &mut world,
        child,
        MaterialRef::inline(Box::new(StandardMaterial {
            base_color: LinearRgba::new(0.8, 0.1, 0.1, 1.0),
            base_color_texture: Some(wall),
            metallic: 0.25,
            roughness: 0.6,
            ..StandardMaterial::default()
        })),
    );
    world.set_parent(child, Some(root));

    let grandchild = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 0.0, 3.0)),
        Name::new("Grandchild"),
        Light::directional(),
        MeshRef::Asset(AssetUUID::new_v5("meshes/rock.gltf")),
        MaterialRef::Asset(AssetUUID::new_v5("materials/rock.kmat")),
    ));
    add(
        &mut world,
        grandchild,
        AudioSource {
            volume: 0.5,
            looping: true,
            autoplay: false,
            ..AudioSource::default()
        },
    );
    world.set_parent(grandchild, Some(child));

    let body = world.spawn((
        Transform::from_translation(Vec3::new(4.0, 0.0, -4.0)),
        RigidBody {
            body_type: BodyType::Dynamic,
            mass: 2.5,
            ccd_enabled: true,
            initial_velocity: Vec3::new(0.0, 3.0, 0.0),
            ..RigidBody::default()
        },
        Collider {
            shape: ColliderShape::Capsule(1.0, 0.25),
            friction: 0.9,
            restitution: 0.1,
            is_sensor: false,
            ..Collider::default()
        },
        PhysicsMaterial {
            friction: 0.3,
            restitution: 0.7,
        },
        ActiveEvents,
    ));
    add(
        &mut world,
        body,
        KinematicCharacterController {
            desired_translation: Vec3::new(0.1, 0.0, 0.2),
            ..KinematicCharacterController::default()
        },
    );
    // Runtime: observed by the physics writeback, never recorded.
    add(
        &mut world,
        body,
        BodyMotion {
            linear: Vec3::new(1.0, 0.0, -1.0),
            angular: Vec3::ZERO,
        },
    );

    let guard = world.spawn((
        Transform::identity(),
        Name::new("Guard"),
        Script {
            module: "ai/guard.erg".into(),
            behavior: "Guard".into(),
            fields: vec![
                ("speed".into(), ScriptValue::Float(0.1)),
                ("target".into(), ScriptValue::Entity(child)),
                (
                    "patrol".into(),
                    ScriptValue::Array(vec![ScriptValue::Entity(root), ScriptValue::Entity(body)]),
                ),
                (
                    "memory".into(),
                    ScriptValue::Struct(vec![
                        ("last_seen".into(), ScriptValue::Entity(grandchild)),
                        (
                            "tint".into(),
                            ScriptValue::Color(LinearRgba::new(0.1, 0.2, 0.3, 0.4)),
                        ),
                        ("aim".into(), ScriptValue::Vec2(Vec2::new(1.0, -2.0))),
                    ]),
                ),
            ],
            runtime: suspended_snapshot(child),
        },
    ));

    let panel = world.spawn((
        UiNode {
            width: UiVal::Px(100.0),
            height: UiVal::Percent(50.0),
            min_width: UiVal::Auto,
            min_height: UiVal::Px(10.0),
            max_width: UiVal::Percent(90.0),
            max_height: UiVal::Auto,
            padding: UiRect::all(UiVal::Px(2.0)),
            margin: UiRect::all(UiVal::Auto),
            flex_direction: UiFlexDirection::Row,
            flex_grow: 1.0,
            flex_shrink: 0.5,
        },
        UiTransform {
            pos: Vec2::new(10.0, 20.0),
            size: Vec2::new(300.0, 40.0),
            z_index: -3,
        },
        UiStyle {
            texture_id: Some(42),
            border_width: 2.0,
            ..UiStyle::default()
        },
        UiColor(Vec4::new(0.2, 0.4, 0.6, 0.8)),
        UiImage { texture: wall },
    ));
    add(
        &mut world,
        panel,
        UiBorder {
            width: UiRect::all(1.0),
            color: UiColor(Vec4::new(0.0, 0.0, 0.0, 1.0)),
            radius: 4.0,
        },
    );
    add(
        &mut world,
        panel,
        UiInteraction {
            state: UiInteractionState::Hovered,
            focusable: true,
            blocks_input: false,
        },
    );
    add(
        &mut world,
        panel,
        UiText {
            content: "Start".into(),
            font,
            size: 18.0,
            color: Vec4::new(1.0, 1.0, 1.0, 1.0),
        },
    );

    (
        world,
        Sample {
            root,
            child,
            grandchild,
            body,
            guard,
            panel,
        },
    )
}
