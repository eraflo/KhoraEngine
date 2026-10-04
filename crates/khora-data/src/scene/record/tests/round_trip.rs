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

//! Every type the engine persists survives a record.

use std::collections::BTreeSet;

use khora_core::asset::{
    AlphaMode, AssetUUID, EmissiveMaterial, StandardMaterial, UnlitMaterial, WireframeMaterial,
};
use khora_core::math::{LinearRgba, Mat4, Quaternion, Vec2, Vec3, Vec4};
use khora_core::physics::{BodyType, ColliderShape};
use khora_core::renderer::light::{DirectionalLight, LightType, PointLight, SpotLight};
use khora_core::script::{
    FrozenFrame, FrozenMachine, FrozenValue, PendingBody, PendingSequence, ScriptSnapshot,
    ScriptValue, TimerRemaining,
};
use khora_core::ui::types::{UiFlexDirection, UiRect, UiVal};

use crate::ecs::{
    ActiveEvents, AudioListener, AudioSource, BodyMotion, Camera, Children, Collider,
    GlobalTransform, KinematicCharacterController, Light, Name, Parent, PhysicsDebugData,
    PhysicsMaterial, ProceduralMeshKind, ProjectionType, RigidBody, Script, ScriptState,
    SerializableActiveEvents, SerializableAudioListener, SerializableAudioSource,
    SerializableBodyMotion, SerializableCamera, SerializableChildren, SerializableCollider,
    SerializableGlobalTransform, SerializableKinematicCharacterController, SerializableLight,
    SerializableName, SerializableParent, SerializablePhysicsDebugData,
    SerializablePhysicsMaterial, SerializableRigidBody, SerializableScript,
    SerializableScriptState, SerializableSimulatedTransform, SerializableTag,
    SerializableTeleported, SerializableTransform, SimulatedTransform, Tag, Teleported, Transform,
};
use crate::ui::{
    SerializableUiBorder, SerializableUiColor, SerializableUiImage, SerializableUiInteraction,
    SerializableUiNode, SerializableUiStyle, SerializableUiText, SerializableUiTransform, UiBorder,
    UiColor, UiImage, UiInteraction, UiInteractionState, UiNode, UiStyle, UiText, UiTransform,
};

use super::*;

/// One of every script value, the containers nesting the others.
fn every_script_value(target: EntityId) -> Vec<ScriptValue> {
    vec![
        ScriptValue::Unit,
        ScriptValue::Bool(true),
        ScriptValue::Int(-42),
        ScriptValue::Int(i64::MIN),
        ScriptValue::Float(0.1),
        ScriptValue::Str("a line".into()),
        ScriptValue::Vec2(Vec2::new(1.0, -2.0)),
        ScriptValue::Vec3(Vec3::new(0.5, 1.5, -2.5)),
        ScriptValue::Vec4(Vec4::new(1.0, 2.0, 3.0, 4.0)),
        ScriptValue::Quat(Quaternion::from_axis_angle(Vec3::Y, 0.75)),
        ScriptValue::Color(LinearRgba::new(0.1, 0.2, 0.3, 0.4)),
        ScriptValue::Entity(target),
        ScriptValue::Array(vec![
            ScriptValue::Int(1),
            ScriptValue::Array(vec![ScriptValue::Str("nested".into())]),
            ScriptValue::Struct(vec![("inner".into(), ScriptValue::Float(2.5))]),
        ]),
        ScriptValue::Struct(vec![
            ("flag".into(), ScriptValue::Bool(false)),
            (
                "list".into(),
                ScriptValue::Array(vec![ScriptValue::Entity(target), ScriptValue::Unit]),
            ),
        ]),
    ]
}

/// One of every frozen register.
fn every_frozen_value(target: EntityId) -> Vec<FrozenValue> {
    vec![
        FrozenValue::Unit,
        FrozenValue::Int(-7),
        FrozenValue::Float(1.25),
        FrozenValue::Bool(true),
        FrozenValue::Entity(target),
        FrozenValue::Literal("hello".into()),
        FrozenValue::Expired,
        FrozenValue::Vec2(Vec2::new(3.0, 4.0)),
        FrozenValue::Vec3(Vec3::new(-1.0, 0.0, 1.0)),
        FrozenValue::Vec4(Vec4::new(0.0, 0.25, 0.5, 0.75)),
        FrozenValue::Quat(Quaternion::from_axis_angle(Vec3::X, -0.5)),
        FrozenValue::Color(LinearRgba::new(1.0, 0.5, 0.25, 1.0)),
        FrozenValue::Null,
    ]
}

/// A machine stopped part-way down a two-frame stack.
fn frozen_machine(target: EntityId) -> FrozenMachine {
    FrozenMachine {
        body: PendingBody::Timer {
            timer: "Guard.__every(0.5)".to_owned(),
            rearm: FrozenValue::Float(0.5),
        },
        registers: every_frozen_value(target),
        frames: vec![
            FrozenFrame {
                function: "on_hit".into(),
                base: 0,
                return_pc: 0,
                result: 0,
                site: String::new(),
                fingerprint: 0,
                locals: Vec::new(),
                temporaries: Vec::new(),
            },
            FrozenFrame {
                function: "wind_up".into(),
                base: 6,
                return_pc: 12,
                result: 3,
                site: String::new(),
                fingerprint: 0,
                locals: Vec::new(),
                temporaries: Vec::new(),
            },
        ],
        program_counter: 42,
        arguments: Vec::new(),
    }
}

/// A behavior instance mid-way through everything it can be doing.
fn busy_snapshot(target: EntityId) -> ScriptSnapshot {
    ScriptSnapshot {
        fields: vec![
            ("speed".into(), ScriptValue::Float(3.0)),
            ("target".into(), ScriptValue::Entity(target)),
        ],
        state: Some("Patrol".into()),
        state_fields: vec![("waypoint".into(), ScriptValue::Int(2))],
        timers: vec![
            TimerRemaining {
                repeating: true,
                interval: 0.5,
                state: None,
                ordinal: 0,
                remaining: Some(0.25),
            },
            TimerRemaining {
                repeating: false,
                interval: 2.0,
                state: Some("Chase".into()),
                ordinal: 1,
                remaining: None,
            },
        ],
        pending: Some(PendingSequence {
            fingerprint: 0xfeed_beef_dead_c0de,
            remaining: 1.5,
            machine: frozen_machine(target),
        }),
        lifecycle: khora_core::script::InstanceLifecycle {
            spawned: true,
            fault: Some(khora_core::script::RecordedFault {
                fingerprint: 0xfeed_beef_dead_c0de,
                reason: "DivideByZero in `on_hit`".into(),
            }),
        },
    }
}

/// A save is only as good as its worst type: every component the engine
/// persists — through the mirror its derive generates, which is what a scene
/// holds — and every type nested in one must come back from a record exactly
/// as it went in, entities and assets included.
#[test]
fn every_persisted_type_round_trips() {
    let target = entity(5, 2);
    let other = entity(9, 0);
    let texture = AssetUUID::new_v5("textures/wall.png");
    let font = AssetUUID::new_v5("fonts/body.ttf");

    // Audio.
    assert_round_trips(&SerializableAudioListener::from(AudioListener));
    assert_round_trips(&SerializableAudioSource::from(AudioSource {
        volume: 0.5,
        looping: true,
        autoplay: false,
        ..AudioSource::default()
    }));

    // Render.
    assert_round_trips(&SerializableCamera::from(Camera::default_perspective()));
    assert_round_trips(&SerializableCamera::from(Camera::new_orthographic(
        640.0, 480.0, -1.0, 250.0,
    )));
    for light_type in [
        LightType::Directional(DirectionalLight {
            intensity: 3.5,
            shadow_enabled: true,
            ..DirectionalLight::default()
        }),
        LightType::Point(PointLight {
            range: 12.0,
            color: LinearRgba::new(1.0, 0.8, 0.6, 1.0),
            ..PointLight::default()
        }),
        LightType::Spot(SpotLight {
            inner_cone_angle: 0.2,
            outer_cone_angle: 0.4,
            ..SpotLight::default()
        }),
    ] {
        assert_round_trips(&light_type);
        assert_round_trips(&SerializableLight::from(Light {
            light_type,
            enabled: false,
        }));
    }
    assert_round_trips(&DirectionalLight::default());
    assert_round_trips(&PointLight::default());
    assert_round_trips(&SpotLight::default());
    for projection in [
        ProjectionType::Perspective { fov_y_radians: 1.2 },
        ProjectionType::Orthographic {
            width: 16.0,
            height: 9.0,
        },
    ] {
        assert_round_trips(&projection);
    }

    // Spatial.
    assert_round_trips(&SerializableName::from(Name::new("Hero")));
    assert_round_trips(&SerializableParent::from(Parent(target)));
    assert_round_trips(&SerializableChildren::from(Children(vec![target, other])));
    assert_round_trips(&SerializableChildren::from(Children(Vec::new())));
    assert_round_trips(&SerializableTag::from(Tag(BTreeSet::from([
        "enemy".to_owned(),
        "boss".to_owned(),
    ]))));
    assert_round_trips(&SerializableTransform::from(Transform::new(
        Vec3::new(1.0, 2.0, 3.0),
        Quaternion::from_axis_angle(Vec3::Z, 0.3),
        Vec3::new(2.0, 2.0, 0.5),
    )));
    assert_round_trips(&SerializableGlobalTransform::from(GlobalTransform::new(
        Mat4::from_translation(Vec3::new(-4.0, 0.5, 8.0)),
    )));
    assert_round_trips(&SerializableSimulatedTransform::from(
        SimulatedTransform::from_parts(
            Vec3::new(0.0, 1.0, 0.0),
            Quaternion::from_axis_angle(Vec3::Y, 1.1),
        ),
    ));
    assert_round_trips(&SerializableTeleported::from(Teleported));

    // Physics.
    assert_round_trips(&SerializableActiveEvents::from(ActiveEvents));
    assert_round_trips(&SerializableBodyMotion::from(BodyMotion {
        linear: Vec3::new(1.0, 0.0, -1.0),
        angular: Vec3::new(0.0, 0.25, 0.0),
    }));
    for shape in [
        ColliderShape::Box(Vec3::new(0.5, 1.0, 1.5)),
        ColliderShape::Sphere(0.75),
        ColliderShape::Capsule(1.0, 0.25),
    ] {
        assert_round_trips(&shape);
        assert_round_trips(&SerializableCollider::from(Collider {
            shape,
            friction: 0.9,
            restitution: 0.1,
            is_sensor: true,
            ..Collider::default()
        }));
    }
    for body_type in [BodyType::Dynamic, BodyType::Static, BodyType::Kinematic] {
        assert_round_trips(&body_type);
        assert_round_trips(&SerializableRigidBody::from(RigidBody {
            body_type,
            mass: 2.5,
            ccd_enabled: true,
            initial_velocity: Vec3::new(0.0, 3.0, 0.0),
            ..RigidBody::default()
        }));
    }
    assert_round_trips(&SerializableKinematicCharacterController::from(
        KinematicCharacterController {
            desired_translation: Vec3::new(0.1, 0.0, 0.2),
            is_grounded: true,
            ..KinematicCharacterController::default()
        },
    ));
    assert_round_trips(&SerializablePhysicsDebugData::from(PhysicsDebugData {
        vertices: vec![Vec3::ZERO, Vec3::ONE, Vec3::X],
        indices: vec![[0, 1], [1, 2]],
        enabled: true,
    }));
    assert_round_trips(&SerializablePhysicsMaterial::from(PhysicsMaterial {
        friction: 0.3,
        restitution: 0.7,
    }));

    // Script, and everything it nests.
    for value in every_script_value(target) {
        assert_round_trips(&value);
    }
    for value in every_frozen_value(target) {
        assert_round_trips(&value);
    }
    for body in [
        PendingBody::Sequence,
        PendingBody::Spawn,
        PendingBody::Update,
        PendingBody::Timer {
            timer: "Guard.Patrol.__after(1.5)#2".to_owned(),
            rearm: FrozenValue::Int(3),
        },
    ] {
        assert_round_trips(&body);
    }
    assert_round_trips(&frozen_machine(target));
    assert_round_trips(&ScriptSnapshot::default());
    assert_round_trips(&busy_snapshot(target));
    // A machine stopped before it held anything: no register, no frame.
    assert_round_trips(&ScriptSnapshot {
        pending: Some(PendingSequence {
            fingerprint: 1,
            remaining: 0.0,
            machine: FrozenMachine {
                body: PendingBody::Spawn,
                registers: Vec::new(),
                frames: Vec::new(),
                program_counter: 0,
                arguments: Vec::new(),
            },
        }),
        ..ScriptSnapshot::default()
    });
    assert_round_trips(&SerializableScript::from(Script {
        module: "scripts/guard.erg".into(),
        behavior: "Guard".into(),
        fields: every_script_value(target)
            .into_iter()
            .enumerate()
            .map(|(i, value)| (format!("field_{i}"), value))
            .collect(),
    }));
    assert_round_trips(&SerializableScriptState::from(ScriptState {
        behavior: "Guard".into(),
        snapshot: busy_snapshot(other),
    }));

    // UI.
    for value in [UiVal::Px(10.0), UiVal::Percent(50.0), UiVal::Auto] {
        assert_round_trips(&value);
    }
    assert_round_trips(&UiRect {
        left: UiVal::Px(1.0),
        right: UiVal::Percent(2.0),
        top: UiVal::Auto,
        bottom: UiVal::Px(4.0),
    });
    assert_round_trips(&UiRect::all(2.5_f32));
    assert_round_trips(&SerializableUiNode::from(UiNode {
        width: UiVal::Px(100.0),
        height: UiVal::Percent(50.0),
        min_width: UiVal::Auto,
        min_height: UiVal::Px(10.0),
        max_width: UiVal::Percent(90.0),
        max_height: UiVal::Auto,
        padding: UiRect::all(UiVal::Px(2.0)),
        margin: UiRect {
            left: UiVal::Auto,
            right: UiVal::Px(3.0),
            top: UiVal::Percent(1.0),
            bottom: UiVal::Auto,
        },
        flex_direction: UiFlexDirection::Row,
        flex_grow: 1.0,
        flex_shrink: 0.5,
    }));
    assert_round_trips(&SerializableUiTransform::from(UiTransform {
        pos: Vec2::new(10.0, 20.0),
        size: Vec2::new(300.0, 40.0),
        z_index: -3,
    }));
    assert_round_trips(&SerializableUiStyle::from(UiStyle {
        texture_id: Some(42),
        border_width: 2.0,
        ..UiStyle::default()
    }));
    assert_round_trips(&SerializableUiStyle::from(UiStyle::default()));
    assert_round_trips(&SerializableUiColor::from(UiColor(Vec4::new(
        0.2, 0.4, 0.6, 0.8,
    ))));
    assert_round_trips(&SerializableUiImage::from(UiImage { texture }));
    assert_round_trips(&SerializableUiBorder::from(UiBorder {
        width: UiRect::all(1.0),
        color: UiColor(Vec4::new(0.0, 0.0, 0.0, 1.0)),
        radius: 4.0,
    }));
    for state in [
        UiInteractionState::Normal,
        UiInteractionState::Hovered,
        UiInteractionState::Pressed,
        UiInteractionState::Focused,
    ] {
        assert_round_trips(&SerializableUiInteraction::from(UiInteraction {
            state,
            focusable: true,
            blocks_input: false,
        }));
    }
    assert_round_trips(&SerializableUiText::from(UiText {
        content: "Start".into(),
        font,
        size: 18.0,
        color: Vec4::new(1.0, 1.0, 1.0, 1.0),
    }));

    // Meshes and materials, as their authored references carry them.
    for kind in [
        ProceduralMeshKind::Cube,
        ProceduralMeshKind::Sphere,
        ProceduralMeshKind::Plane,
    ] {
        assert_round_trips(&kind);
    }
    assert_round_trips(&[1.0_f32, 0.5, 0.0, 0.0]);
    assert_round_trips(&StandardMaterial {
        base_color_texture: Some(texture),
        normal_map: Some(font),
        alpha_mode: AlphaMode::Mask(0.3),
        double_sided: true,
        ..StandardMaterial::default()
    });
    assert_round_trips(&UnlitMaterial {
        alpha_mode: AlphaMode::Blend,
        ..UnlitMaterial::default()
    });
    assert_round_trips(&EmissiveMaterial::default());
    assert_round_trips(&WireframeMaterial::default());

    // The two references on their own.
    assert_round_trips(&target);
    assert_round_trips(&texture);
}
