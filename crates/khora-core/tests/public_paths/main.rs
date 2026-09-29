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

//! Compile-level guard over `khora_core`'s public surface — every top module
//! except `math`, `renderer` and `ui`, which have their own modules
//! (`math.rs`, `renderer.rs`, `ui.rs` beside this file), plus the exported
//! macros and the
//! paths other crates of the workspace use.
//!
//! Every `pub` item reachable through a `pub` path is named here at its full
//! path: each `pub mod`, the `pub use` re-exports, their types, free functions,
//! constants, statics, type aliases, public fields, enum variants (matched
//! exhaustively), inherent `pub` methods (turbofished where generic), trait
//! items, and the std / serde / bincode / bytemuck / operator trait impls. A
//! reorganisation that moves code between files must keep every one of these
//! paths valid, so this file stops compiling the moment one disappears.
//!
//! Re-exports are also checked for *identity*: a `pub use` that starts pointing
//! at a different item with the same name is a compile error (`same_type` for
//! types, `same_item` for functions, bound forwarding for traits, `ptr::eq`
//! for statics).
//!
//! The list was generated from `khora_core`'s rustdoc JSON, so it is complete
//! for the tree it was written against. Nothing is constructed; the tests only
//! have to type-check.

mod light_caps;
mod math;
mod math_layout;
mod renderer;
mod ui;

use std::any::type_name;
use std::marker::PhantomData;

/// Compiles only when both arguments name the same type.
fn same_type<T: ?Sized>(_: PhantomData<T>, _: PhantomData<T>) {}

/// Compiles only when both references point at the same item (two distinct
/// `fn` items never share a type, and `&fn-item` does not coerce).
fn same_item<T>(_: &T, _: &T) {}

fn is_asset<T: khora_core::asset::Asset>() {}
fn is_borrow_decode<T: bincode::BorrowDecode<'static, ()>>() {}
fn is_clone<T: Clone>() {}
fn is_copy<T: Copy>() {}
fn is_debug<T: std::fmt::Debug>() {}
fn is_decode<T: bincode::Decode<()>>() {}
fn is_default<T: Default>() {}
fn is_deref<T: std::ops::Deref>() {}
fn is_deref_mut<T: std::ops::DerefMut>() {}
fn is_deserialize_owned<T: serde::de::DeserializeOwned>() {}
fn is_display<T: std::fmt::Display>() {}
fn is_encode<T: bincode::Encode>() {}
fn is_eq<T: Eq>() {}
fn is_error<T: std::error::Error>() {}
fn is_extend_script_state_update<T: Extend<khora_core::script::writeback::ScriptStateUpdate>>() {}
fn is_extend_world_command<T: Extend<khora_core::script::command::WorldCommand>>() {}
fn is_from_str<T: From<&'static str>>() {}
fn is_from_string<T: From<String>>() {}
fn is_hash<T: std::hash::Hash>() {}
fn is_into_iterator<T: IntoIterator>() {}
fn is_material<T: khora_core::asset::Material>() {}
fn is_ord<T: Ord>() {}
fn is_partial_eq<T: PartialEq>() {}
fn is_partial_ord<T: PartialOrd>() {}
fn is_registry_kind<T: khora_core::runtime::typed_registry::RegistryKind>() {}
fn is_serialize<T: serde::Serialize>() {}
fn is_supersedes<T: khora_core::event::Supersedes>() {}

// ---------------------------------------------------------------------------
// Every `pub mod`, down to the leaves.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod every_pub_mod {
    use khora_core::agent as _;
    use khora_core::agent::completion as _;
    use khora_core::agent::contention as _;
    use khora_core::agent::dependency as _;
    use khora_core::agent::execution_phase as _;
    use khora_core::agent::gorna as _;
    use khora_core::agent::mode as _;
    use khora_core::agent::timing as _;
    use khora_core::asset as _;
    use khora_core::asset::font as _;
    use khora_core::asset::script as _;
    use khora_core::audio as _;
    use khora_core::audio::device as _;
    use khora_core::audio::mix_bus as _;
    use khora_core::ecs as _;
    use khora_core::ecs::entity as _;
    use khora_core::engine_context as _;
    use khora_core::event as _;
    use khora_core::graph as _;
    use khora_core::interpolation as _;
    use khora_core::lane as _;
    use khora_core::lane::bus as _;
    use khora_core::lane::context_keys as _;
    use khora_core::lane::deck as _;
    use khora_core::lane::lock as _;
    use khora_core::lane::slot as _;
    use khora_core::memory as _;
    use khora_core::physics as _;
    use khora_core::platform as _;
    use khora_core::platform::input as _;
    use khora_core::platform::input_map as _;
    use khora_core::platform::window as _;
    use khora_core::runtime as _;
    use khora_core::runtime::typed_registry as _;
    use khora_core::scene as _;
    use khora_core::script as _;
    use khora_core::script::buffer as _;
    use khora_core::script::command as _;
    use khora_core::script::event as _;
    use khora_core::script::snapshot as _;
    use khora_core::script::table as _;
    use khora_core::script::value as _;
    use khora_core::script::writeback as _;
    use khora_core::telemetry as _;
    use khora_core::telemetry::event as _;
    use khora_core::telemetry::metrics as _;
    use khora_core::telemetry::monitoring as _;
    use khora_core::time as _;
    use khora_core::util as _;
    use khora_core::util::any_map as _;
    use khora_core::util::bitflags as _;
    use khora_core::util::stopwatch as _;
}

// ---------------------------------------------------------------------------
// Public fields (read through a reference) and enum variants (matched
// exhaustively). Nothing is constructed.
// ---------------------------------------------------------------------------

fn agent_access_variants(x: &khora_core::agent::AgentAccess) {
    match x {
        khora_core::agent::AgentAccess::Exclusive => {}
        khora_core::agent::AgentAccess::Isolated => {}
        khora_core::agent::AgentAccess::SharedWorld => {}
    }
}

fn completion_outcome_variants(x: &khora_core::agent::completion::CompletionOutcome) {
    match x {
        khora_core::agent::completion::CompletionOutcome::Completed => {}
        khora_core::agent::completion::CompletionOutcome::Skipped => {}
    }
}

fn contention_fields(x: &khora_core::agent::contention::Contention) {
    let _ = (&x.deck, &x.reads, &x.writes);
}

fn agent_dependency_fields(x: &khora_core::agent::dependency::AgentDependency) {
    let _ = (&x.target, &x.kind, &x.condition);
}

fn dependency_condition_variants(x: &khora_core::agent::dependency::DependencyCondition) {
    match x {
        khora_core::agent::dependency::DependencyCondition::IfTargetActive => {}
        khora_core::agent::dependency::DependencyCondition::IfBudgetAbove(..) => {}
        khora_core::agent::dependency::DependencyCondition::IfEngineMode(..) => {}
    }
}

fn dependency_kind_variants(x: &khora_core::agent::dependency::DependencyKind) {
    match x {
        khora_core::agent::dependency::DependencyKind::Hard => {}
        khora_core::agent::dependency::DependencyKind::Soft => {}
        khora_core::agent::dependency::DependencyKind::Parallel => {}
    }
}

fn engine_mode_variants(x: &khora_core::agent::mode::EngineMode) {
    match x {
        khora_core::agent::mode::EngineMode::Playing => {}
        khora_core::agent::mode::EngineMode::Custom(..) => {}
    }
}

fn agent_importance_variants(x: &khora_core::agent::timing::AgentImportance) {
    match x {
        khora_core::agent::timing::AgentImportance::Critical => {}
        khora_core::agent::timing::AgentImportance::Important => {}
        khora_core::agent::timing::AgentImportance::Optional => {}
    }
}

fn execution_timing_fields(x: &khora_core::agent::timing::ExecutionTiming) {
    let _ = (
        &x.allowed_phases,
        &x.default_phase,
        &x.priority,
        &x.importance,
        &x.fixed_timestep,
        &x.dependencies,
    );
}

fn alpha_mode_variants(x: &khora_core::asset::AlphaMode) {
    match x {
        khora_core::asset::AlphaMode::Opaque => {}
        khora_core::asset::AlphaMode::Mask(..) => {}
        khora_core::asset::AlphaMode::Blend => {}
    }
}

fn asset_metadata_fields(x: &khora_core::asset::AssetMetadata) {
    let _ = (
        &x.uuid,
        &x.source_path,
        &x.asset_type_name,
        &x.dependencies,
        &x.variants,
        &x.tags,
    );
}

fn asset_source_variants(x: &khora_core::asset::AssetSource) {
    match x {
        khora_core::asset::AssetSource::Path(..) => {}
        khora_core::asset::AssetSource::Packed { .. } => {}
    }
}

fn compression_kind_variants(x: &khora_core::asset::CompressionKind) {
    match x {
        khora_core::asset::CompressionKind::None => {}
        khora_core::asset::CompressionKind::Lz4 => {}
    }
}

fn emissive_material_fields(x: &khora_core::asset::EmissiveMaterial) {
    let _ = (&x.emissive_color, &x.intensity, &x.alpha_mode);
}

fn standard_material_fields(x: &khora_core::asset::StandardMaterial) {
    let _ = (
        &x.base_color,
        &x.base_color_texture,
        &x.metallic,
        &x.roughness,
        &x.metallic_roughness_texture,
        &x.normal_map,
        &x.occlusion_map,
        &x.emissive,
        &x.emissive_texture,
        &x.alpha_mode,
        &x.alpha_cutoff,
        &x.double_sided,
    );
}

fn unlit_material_fields(x: &khora_core::asset::UnlitMaterial) {
    let _ = (&x.base_color, &x.alpha_mode, &x.alpha_cutoff);
}

fn wireframe_material_fields(x: &khora_core::asset::WireframeMaterial) {
    let _ = (&x.color, &x.line_width);
}

fn font_fields(x: &khora_core::asset::font::Font) {
    let _ = (&x.name, &x.data);
}

fn script_module_fields(x: &khora_core::asset::script::ScriptModule) {
    let _ = (&x.source, &x.imports);
}

fn stream_info_fields(x: &khora_core::audio::device::StreamInfo) {
    let _ = (&x.channels, &x.sample_rate);
}

fn engine_context_fields(x: &khora_core::engine_context::EngineContext<'static>) {
    let _ = (&x.world, &x.runtime, &x.bus, &x.deck);
}

fn world_access_variants(x: &khora_core::engine_context::WorldAccess<'static>) {
    match x {
        khora_core::engine_context::WorldAccess::None => {}
        khora_core::engine_context::WorldAccess::Shared(..) => {}
        khora_core::engine_context::WorldAccess::Exclusive(..) => {}
    }
}

fn adaptation_mode_variants(x: &khora_core::agent::gorna::AdaptationMode) {
    match x {
        khora_core::agent::gorna::AdaptationMode::Learning => {}
        khora_core::agent::gorna::AdaptationMode::Manual(..) => {}
        khora_core::agent::gorna::AdaptationMode::Stable => {}
        khora_core::agent::gorna::AdaptationMode::Bounded { .. } => {}
    }
}

fn agent_frame_status_fields(x: &khora_core::agent::gorna::AgentFrameStatus) {
    let _ = (&x.measured_time_ms,);
}

fn agent_hints_fields(x: &khora_core::agent::gorna::AgentHints) {
    let _ = (&x.cap_ms, &x.priority);
}

fn agent_id_variants(x: &khora_core::agent::gorna::AgentId) {
    match x {
        khora_core::agent::gorna::AgentId::Renderer => {}
        khora_core::agent::gorna::AgentId::ShadowRenderer => {}
        khora_core::agent::gorna::AgentId::Overlay => {}
        khora_core::agent::gorna::AgentId::Skybox => {}
        khora_core::agent::gorna::AgentId::Physics => {}
        khora_core::agent::gorna::AgentId::Ecs => {}
        khora_core::agent::gorna::AgentId::Ui => {}
        khora_core::agent::gorna::AgentId::Audio => {}
        khora_core::agent::gorna::AgentId::Asset => {}
        khora_core::agent::gorna::AgentId::Script => {}
    }
}

fn agent_status_fields(x: &khora_core::agent::gorna::AgentStatus) {
    let _ = (
        &x.agent_id,
        &x.current_strategy,
        &x.health_score,
        &x.is_stalled,
        &x.message,
    );
}

fn decision_trace_fields(x: &khora_core::agent::gorna::DecisionTrace) {
    let _ = (&x.ticks,);
}

fn engine_hint_variants(x: &khora_core::agent::gorna::EngineHint) {
    match x {
        khora_core::agent::gorna::EngineHint::Cap { .. } => {}
        khora_core::agent::gorna::EngineHint::Prioritize { .. } => {}
    }
}

fn negotiation_request_fields(x: &khora_core::agent::gorna::NegotiationRequest) {
    let _ = (
        &x.target_latency,
        &x.priority_weight,
        &x.constraints,
        &x.current_mode,
        &x.agent_timing,
    );
}

fn negotiation_response_fields(x: &khora_core::agent::gorna::NegotiationResponse) {
    let _ = (&x.strategies, &x.timing_adjustment);
}

fn resource_budget_fields(x: &khora_core::agent::gorna::ResourceBudget) {
    let _ = (
        &x.strategy_id,
        &x.time_limit,
        &x.memory_limit,
        &x.extra_params,
    );
}

fn resource_constraints_fields(x: &khora_core::agent::gorna::ResourceConstraints) {
    let _ = (&x.max_vram_bytes, &x.max_memory_bytes, &x.must_run);
}

fn strategy_id_variants(x: &khora_core::agent::gorna::StrategyId) {
    match x {
        khora_core::agent::gorna::StrategyId::LowPower => {}
        khora_core::agent::gorna::StrategyId::Balanced => {}
        khora_core::agent::gorna::StrategyId::HighPerformance => {}
        khora_core::agent::gorna::StrategyId::Custom(..) => {}
    }
}

fn strategy_option_fields(x: &khora_core::agent::gorna::StrategyOption) {
    let _ = (&x.id, &x.estimated_time, &x.estimated_vram);
}

fn timing_adjustment_fields(x: &khora_core::agent::gorna::TimingAdjustment) {
    let _ = (&x.importance_override,);
}

fn entity_id_fields(x: &khora_core::ecs::entity::EntityId) {
    let _ = (&x.index, &x.generation);
}

fn when_full_variants(x: &khora_core::event::WhenFull) {
    match x {
        khora_core::event::WhenFull::DropOldest => {}
        khora_core::event::WhenFull::Reject => {}
    }
}

fn lane_error_variants(x: &khora_core::lane::LaneError) {
    match x {
        khora_core::lane::LaneError::NotInitialized => {}
        khora_core::lane::LaneError::InvalidContext { .. } => {}
        khora_core::lane::LaneError::LockPoisoned { .. } => {}
        khora_core::lane::LaneError::MissingResource { .. } => {}
        khora_core::lane::LaneError::MissingAsset { .. } => {}
        khora_core::lane::LaneError::ExecutionFailed(..) => {}
        khora_core::lane::LaneError::InitializationFailed(..) => {}
    }
}

fn lane_kind_variants(x: &khora_core::lane::LaneKind) {
    match x {
        khora_core::lane::LaneKind::Render => {}
        khora_core::lane::LaneKind::Shadow => {}
        khora_core::lane::LaneKind::Physics => {}
        khora_core::lane::LaneKind::Audio => {}
        khora_core::lane::LaneKind::Asset => {}
        khora_core::lane::LaneKind::Scene => {}
        khora_core::lane::LaneKind::Ecs => {}
        khora_core::lane::LaneKind::Ui => {}
        khora_core::lane::LaneKind::Script => {}
    }
}

fn clear_color_fields(x: &khora_core::lane::context_keys::ClearColor) {
    let _ = (&x.0,);
}

fn color_target_fields(x: &khora_core::lane::context_keys::ColorTarget) {
    let _ = (&x.0,);
}

fn depth_target_fields(x: &khora_core::lane::context_keys::DepthTarget) {
    let _ = (&x.0,);
}

fn physics_delta_time_fields(x: &khora_core::lane::context_keys::PhysicsDeltaTime) {
    let _ = (&x.0,);
}

fn shadow_atlas_cube_view_fields(x: &khora_core::lane::context_keys::ShadowAtlasCubeView) {
    let _ = (&x.0,);
}

fn shadow_atlas_view_fields(x: &khora_core::lane::context_keys::ShadowAtlasView) {
    let _ = (&x.0,);
}

fn shadow_comparison_sampler_fields(x: &khora_core::lane::context_keys::ShadowComparisonSampler) {
    let _ = (&x.0,);
}

fn extended_memory_stats_fields(x: &khora_core::memory::ExtendedMemoryStats) {
    let _ = (
        &x.current_allocated_bytes,
        &x.peak_allocated_bytes,
        &x.total_allocations,
        &x.total_deallocations,
        &x.total_reallocations,
        &x.net_allocations,
        &x.bytes_allocated_lifetime,
        &x.bytes_deallocated_lifetime,
        &x.bytes_net_lifetime,
        &x.large_allocations,
        &x.large_allocation_bytes,
        &x.small_allocations,
        &x.small_allocation_bytes,
        &x.medium_allocations,
        &x.medium_allocation_bytes,
        &x.average_allocation_size,
        &x.fragmentation_ratio,
        &x.allocation_efficiency,
    );
}

fn body_type_variants(x: &khora_core::physics::BodyType) {
    match x {
        khora_core::physics::BodyType::Dynamic => {}
        khora_core::physics::BodyType::Static => {}
        khora_core::physics::BodyType::Kinematic => {}
    }
}

fn character_controller_options_fields(x: &khora_core::physics::CharacterControllerOptions) {
    let _ = (
        &x.autostep_height,
        &x.autostep_min_width,
        &x.autostep_enabled,
        &x.max_slope_climb_angle,
        &x.min_slope_slide_angle,
        &x.offset,
    );
}

fn collider_desc_fields(x: &khora_core::physics::ColliderDesc) {
    let _ = (
        &x.owner,
        &x.parent_body,
        &x.position,
        &x.rotation,
        &x.shape,
        &x.active_events,
        &x.friction,
        &x.restitution,
    );
}

fn collider_handle_fields(x: &khora_core::physics::ColliderHandle) {
    let _ = (&x.0,);
}

fn collider_shape_variants(x: &khora_core::physics::ColliderShape) {
    match x {
        khora_core::physics::ColliderShape::Box(..) => {}
        khora_core::physics::ColliderShape::Sphere(..) => {}
        khora_core::physics::ColliderShape::Capsule(..) => {}
    }
}

fn collision_fields(x: &khora_core::physics::Collision) {
    let _ = (&x.kind, &x.a, &x.b);
}

fn collision_event_variants(x: &khora_core::physics::CollisionEvent) {
    match x {
        khora_core::physics::CollisionEvent::Started(..) => {}
        khora_core::physics::CollisionEvent::Stopped(..) => {}
    }
}

fn collision_kind_variants(x: &khora_core::physics::CollisionKind) {
    match x {
        khora_core::physics::CollisionKind::Started => {}
        khora_core::physics::CollisionKind::Stopped => {}
    }
}

fn contact_batch_fields(x: &khora_core::physics::ContactBatch) {
    let _ = (&x.contacts,);
}

fn debug_line_fields(x: &khora_core::physics::DebugLine) {
    let _ = (&x.start, &x.end, &x.color);
}

fn raycast_hit_fields(x: &khora_core::physics::RaycastHit) {
    let _ = (&x.collider, &x.distance, &x.normal, &x.position);
}

fn rigid_body_desc_fields(x: &khora_core::physics::RigidBodyDesc) {
    let _ = (
        &x.position,
        &x.rotation,
        &x.body_type,
        &x.linear_velocity,
        &x.angular_velocity,
        &x.mass,
        &x.ccd_enabled,
    );
}

fn rigid_body_handle_fields(x: &khora_core::physics::RigidBodyHandle) {
    let _ = (&x.0,);
}

fn physics_slot_fields(x: &khora_core::physics::SlotId) {
    let _ = (&x.index, &x.generation);
}

fn battery_level_variants(x: &khora_core::platform::BatteryLevel) {
    match x {
        khora_core::platform::BatteryLevel::Mains => {}
        khora_core::platform::BatteryLevel::High => {}
        khora_core::platform::BatteryLevel::Low => {}
        khora_core::platform::BatteryLevel::Critical => {}
    }
}

fn thermal_status_variants(x: &khora_core::platform::ThermalStatus) {
    match x {
        khora_core::platform::ThermalStatus::Cool => {}
        khora_core::platform::ThermalStatus::Warm => {}
        khora_core::platform::ThermalStatus::Throttling => {}
        khora_core::platform::ThermalStatus::Critical => {}
    }
}

fn input_event_variants(x: &khora_core::platform::input::InputEvent) {
    match x {
        khora_core::platform::input::InputEvent::KeyPressed { .. } => {}
        khora_core::platform::input::InputEvent::KeyReleased { .. } => {}
        khora_core::platform::input::InputEvent::MouseButtonPressed { .. } => {}
        khora_core::platform::input::InputEvent::MouseButtonReleased { .. } => {}
        khora_core::platform::input::InputEvent::MouseMoved { .. } => {}
        khora_core::platform::input::InputEvent::MouseWheelScrolled { .. } => {}
        khora_core::platform::input::InputEvent::FocusLost => {}
    }
}

fn key_code_variants(x: &khora_core::platform::input::KeyCode) {
    match x {
        khora_core::platform::input::KeyCode::Backquote => {}
        khora_core::platform::input::KeyCode::Backslash => {}
        khora_core::platform::input::KeyCode::BracketLeft => {}
        khora_core::platform::input::KeyCode::BracketRight => {}
        khora_core::platform::input::KeyCode::Comma => {}
        khora_core::platform::input::KeyCode::Digit0 => {}
        khora_core::platform::input::KeyCode::Digit1 => {}
        khora_core::platform::input::KeyCode::Digit2 => {}
        khora_core::platform::input::KeyCode::Digit3 => {}
        khora_core::platform::input::KeyCode::Digit4 => {}
        khora_core::platform::input::KeyCode::Digit5 => {}
        khora_core::platform::input::KeyCode::Digit6 => {}
        khora_core::platform::input::KeyCode::Digit7 => {}
        khora_core::platform::input::KeyCode::Digit8 => {}
        khora_core::platform::input::KeyCode::Digit9 => {}
        khora_core::platform::input::KeyCode::Equal => {}
        khora_core::platform::input::KeyCode::IntlBackslash => {}
        khora_core::platform::input::KeyCode::IntlRo => {}
        khora_core::platform::input::KeyCode::IntlYen => {}
        khora_core::platform::input::KeyCode::KeyA => {}
        khora_core::platform::input::KeyCode::KeyB => {}
        khora_core::platform::input::KeyCode::KeyC => {}
        khora_core::platform::input::KeyCode::KeyD => {}
        khora_core::platform::input::KeyCode::KeyE => {}
        khora_core::platform::input::KeyCode::KeyF => {}
        khora_core::platform::input::KeyCode::KeyG => {}
        khora_core::platform::input::KeyCode::KeyH => {}
        khora_core::platform::input::KeyCode::KeyI => {}
        khora_core::platform::input::KeyCode::KeyJ => {}
        khora_core::platform::input::KeyCode::KeyK => {}
        khora_core::platform::input::KeyCode::KeyL => {}
        khora_core::platform::input::KeyCode::KeyM => {}
        khora_core::platform::input::KeyCode::KeyN => {}
        khora_core::platform::input::KeyCode::KeyO => {}
        khora_core::platform::input::KeyCode::KeyP => {}
        khora_core::platform::input::KeyCode::KeyQ => {}
        khora_core::platform::input::KeyCode::KeyR => {}
        khora_core::platform::input::KeyCode::KeyS => {}
        khora_core::platform::input::KeyCode::KeyT => {}
        khora_core::platform::input::KeyCode::KeyU => {}
        khora_core::platform::input::KeyCode::KeyV => {}
        khora_core::platform::input::KeyCode::KeyW => {}
        khora_core::platform::input::KeyCode::KeyX => {}
        khora_core::platform::input::KeyCode::KeyY => {}
        khora_core::platform::input::KeyCode::KeyZ => {}
        khora_core::platform::input::KeyCode::Minus => {}
        khora_core::platform::input::KeyCode::Period => {}
        khora_core::platform::input::KeyCode::Quote => {}
        khora_core::platform::input::KeyCode::Semicolon => {}
        khora_core::platform::input::KeyCode::Slash => {}
        khora_core::platform::input::KeyCode::AltLeft => {}
        khora_core::platform::input::KeyCode::AltRight => {}
        khora_core::platform::input::KeyCode::Backspace => {}
        khora_core::platform::input::KeyCode::CapsLock => {}
        khora_core::platform::input::KeyCode::ContextMenu => {}
        khora_core::platform::input::KeyCode::ControlLeft => {}
        khora_core::platform::input::KeyCode::ControlRight => {}
        khora_core::platform::input::KeyCode::Enter => {}
        khora_core::platform::input::KeyCode::SuperLeft => {}
        khora_core::platform::input::KeyCode::SuperRight => {}
        khora_core::platform::input::KeyCode::ShiftLeft => {}
        khora_core::platform::input::KeyCode::ShiftRight => {}
        khora_core::platform::input::KeyCode::Space => {}
        khora_core::platform::input::KeyCode::Tab => {}
        khora_core::platform::input::KeyCode::Convert => {}
        khora_core::platform::input::KeyCode::KanaMode => {}
        khora_core::platform::input::KeyCode::Lang1 => {}
        khora_core::platform::input::KeyCode::Lang2 => {}
        khora_core::platform::input::KeyCode::Lang3 => {}
        khora_core::platform::input::KeyCode::Lang4 => {}
        khora_core::platform::input::KeyCode::Lang5 => {}
        khora_core::platform::input::KeyCode::NonConvert => {}
        khora_core::platform::input::KeyCode::Delete => {}
        khora_core::platform::input::KeyCode::End => {}
        khora_core::platform::input::KeyCode::Help => {}
        khora_core::platform::input::KeyCode::Home => {}
        khora_core::platform::input::KeyCode::Insert => {}
        khora_core::platform::input::KeyCode::PageDown => {}
        khora_core::platform::input::KeyCode::PageUp => {}
        khora_core::platform::input::KeyCode::ArrowDown => {}
        khora_core::platform::input::KeyCode::ArrowLeft => {}
        khora_core::platform::input::KeyCode::ArrowRight => {}
        khora_core::platform::input::KeyCode::ArrowUp => {}
        khora_core::platform::input::KeyCode::NumLock => {}
        khora_core::platform::input::KeyCode::Numpad0 => {}
        khora_core::platform::input::KeyCode::Numpad1 => {}
        khora_core::platform::input::KeyCode::Numpad2 => {}
        khora_core::platform::input::KeyCode::Numpad3 => {}
        khora_core::platform::input::KeyCode::Numpad4 => {}
        khora_core::platform::input::KeyCode::Numpad5 => {}
        khora_core::platform::input::KeyCode::Numpad6 => {}
        khora_core::platform::input::KeyCode::Numpad7 => {}
        khora_core::platform::input::KeyCode::Numpad8 => {}
        khora_core::platform::input::KeyCode::Numpad9 => {}
        khora_core::platform::input::KeyCode::NumpadAdd => {}
        khora_core::platform::input::KeyCode::NumpadBackspace => {}
        khora_core::platform::input::KeyCode::NumpadClear => {}
        khora_core::platform::input::KeyCode::NumpadClearEntry => {}
        khora_core::platform::input::KeyCode::NumpadComma => {}
        khora_core::platform::input::KeyCode::NumpadDecimal => {}
        khora_core::platform::input::KeyCode::NumpadDivide => {}
        khora_core::platform::input::KeyCode::NumpadEnter => {}
        khora_core::platform::input::KeyCode::NumpadEqual => {}
        khora_core::platform::input::KeyCode::NumpadHash => {}
        khora_core::platform::input::KeyCode::NumpadMemoryAdd => {}
        khora_core::platform::input::KeyCode::NumpadMemoryClear => {}
        khora_core::platform::input::KeyCode::NumpadMemoryRecall => {}
        khora_core::platform::input::KeyCode::NumpadMemoryStore => {}
        khora_core::platform::input::KeyCode::NumpadMemorySubtract => {}
        khora_core::platform::input::KeyCode::NumpadMultiply => {}
        khora_core::platform::input::KeyCode::NumpadParenLeft => {}
        khora_core::platform::input::KeyCode::NumpadParenRight => {}
        khora_core::platform::input::KeyCode::NumpadStar => {}
        khora_core::platform::input::KeyCode::NumpadSubtract => {}
        khora_core::platform::input::KeyCode::Escape => {}
        khora_core::platform::input::KeyCode::Fn => {}
        khora_core::platform::input::KeyCode::FnLock => {}
        khora_core::platform::input::KeyCode::PrintScreen => {}
        khora_core::platform::input::KeyCode::ScrollLock => {}
        khora_core::platform::input::KeyCode::Pause => {}
        khora_core::platform::input::KeyCode::BrowserBack => {}
        khora_core::platform::input::KeyCode::BrowserFavorites => {}
        khora_core::platform::input::KeyCode::BrowserForward => {}
        khora_core::platform::input::KeyCode::BrowserHome => {}
        khora_core::platform::input::KeyCode::BrowserRefresh => {}
        khora_core::platform::input::KeyCode::BrowserSearch => {}
        khora_core::platform::input::KeyCode::BrowserStop => {}
        khora_core::platform::input::KeyCode::Eject => {}
        khora_core::platform::input::KeyCode::LaunchApp1 => {}
        khora_core::platform::input::KeyCode::LaunchApp2 => {}
        khora_core::platform::input::KeyCode::LaunchMail => {}
        khora_core::platform::input::KeyCode::MediaPlayPause => {}
        khora_core::platform::input::KeyCode::MediaSelect => {}
        khora_core::platform::input::KeyCode::MediaStop => {}
        khora_core::platform::input::KeyCode::MediaTrackNext => {}
        khora_core::platform::input::KeyCode::MediaTrackPrevious => {}
        khora_core::platform::input::KeyCode::Power => {}
        khora_core::platform::input::KeyCode::Sleep => {}
        khora_core::platform::input::KeyCode::AudioVolumeDown => {}
        khora_core::platform::input::KeyCode::AudioVolumeMute => {}
        khora_core::platform::input::KeyCode::AudioVolumeUp => {}
        khora_core::platform::input::KeyCode::WakeUp => {}
        khora_core::platform::input::KeyCode::Meta => {}
        khora_core::platform::input::KeyCode::Hyper => {}
        khora_core::platform::input::KeyCode::Turbo => {}
        khora_core::platform::input::KeyCode::Abort => {}
        khora_core::platform::input::KeyCode::Resume => {}
        khora_core::platform::input::KeyCode::Suspend => {}
        khora_core::platform::input::KeyCode::Again => {}
        khora_core::platform::input::KeyCode::Copy => {}
        khora_core::platform::input::KeyCode::Cut => {}
        khora_core::platform::input::KeyCode::Find => {}
        khora_core::platform::input::KeyCode::Open => {}
        khora_core::platform::input::KeyCode::Paste => {}
        khora_core::platform::input::KeyCode::Props => {}
        khora_core::platform::input::KeyCode::Select => {}
        khora_core::platform::input::KeyCode::Undo => {}
        khora_core::platform::input::KeyCode::Hiragana => {}
        khora_core::platform::input::KeyCode::Katakana => {}
        khora_core::platform::input::KeyCode::F1 => {}
        khora_core::platform::input::KeyCode::F2 => {}
        khora_core::platform::input::KeyCode::F3 => {}
        khora_core::platform::input::KeyCode::F4 => {}
        khora_core::platform::input::KeyCode::F5 => {}
        khora_core::platform::input::KeyCode::F6 => {}
        khora_core::platform::input::KeyCode::F7 => {}
        khora_core::platform::input::KeyCode::F8 => {}
        khora_core::platform::input::KeyCode::F9 => {}
        khora_core::platform::input::KeyCode::F10 => {}
        khora_core::platform::input::KeyCode::F11 => {}
        khora_core::platform::input::KeyCode::F12 => {}
        khora_core::platform::input::KeyCode::F13 => {}
        khora_core::platform::input::KeyCode::F14 => {}
        khora_core::platform::input::KeyCode::F15 => {}
        khora_core::platform::input::KeyCode::F16 => {}
        khora_core::platform::input::KeyCode::F17 => {}
        khora_core::platform::input::KeyCode::F18 => {}
        khora_core::platform::input::KeyCode::F19 => {}
        khora_core::platform::input::KeyCode::F20 => {}
        khora_core::platform::input::KeyCode::F21 => {}
        khora_core::platform::input::KeyCode::F22 => {}
        khora_core::platform::input::KeyCode::F23 => {}
        khora_core::platform::input::KeyCode::F24 => {}
        khora_core::platform::input::KeyCode::F25 => {}
        khora_core::platform::input::KeyCode::F26 => {}
        khora_core::platform::input::KeyCode::F27 => {}
        khora_core::platform::input::KeyCode::F28 => {}
        khora_core::platform::input::KeyCode::F29 => {}
        khora_core::platform::input::KeyCode::F30 => {}
        khora_core::platform::input::KeyCode::F31 => {}
        khora_core::platform::input::KeyCode::F32 => {}
        khora_core::platform::input::KeyCode::F33 => {}
        khora_core::platform::input::KeyCode::F34 => {}
        khora_core::platform::input::KeyCode::F35 => {}
        khora_core::platform::input::KeyCode::Unidentified => {}
    }
}

fn mouse_button_variants(x: &khora_core::platform::input::MouseButton) {
    match x {
        khora_core::platform::input::MouseButton::Left => {}
        khora_core::platform::input::MouseButton::Right => {}
        khora_core::platform::input::MouseButton::Middle => {}
        khora_core::platform::input::MouseButton::Back => {}
        khora_core::platform::input::MouseButton::Forward => {}
        khora_core::platform::input::MouseButton::Other(..) => {}
    }
}

fn action_fields(x: &khora_core::platform::input_map::Action) {
    let _ = (&x.0,);
}

fn input_binding_variants(x: &khora_core::platform::input_map::InputBinding) {
    match x {
        khora_core::platform::input_map::InputBinding::Key(..) => {}
        khora_core::platform::input_map::InputBinding::Mouse(..) => {}
    }
}

fn input_snapshot_fields(x: &khora_core::platform::input_map::InputSnapshot) {
    let _ = (&x.pressed, &x.just_pressed);
}

fn runtime_fields(x: &khora_core::runtime::Runtime) {
    let _ = (&x.services, &x.backends, &x.resources);
}

fn scene_file_fields(x: &khora_core::scene::SceneFile) {
    let _ = (&x.header, &x.payload);
}

fn scene_file_error_variants(x: &khora_core::scene::SceneFileError) {
    match x {
        khora_core::scene::SceneFileError::TooShort => {}
        khora_core::scene::SceneFileError::InvalidMagicBytes => {}
    }
}

fn scene_header_fields(x: &khora_core::scene::SceneHeader) {
    let _ = (
        &x.magic_bytes,
        &x.format_version,
        &x.strategy_id,
        &x.payload_length,
    );
}

fn serialization_goal_variants(x: &khora_core::scene::SerializationGoal) {
    match x {
        khora_core::scene::SerializationGoal::FastestLoad => {}
        khora_core::scene::SerializationGoal::SmallestFileSize => {}
        khora_core::scene::SerializationGoal::HumanReadableDebug => {}
        khora_core::scene::SerializationGoal::LongTermStability => {}
        khora_core::scene::SerializationGoal::EditorInterchange => {}
        khora_core::scene::SerializationGoal::PortableBinary => {}
    }
}

fn conflict_fields(x: &khora_core::script::buffer::Conflict) {
    let _ = (&x.entity, &x.target, &x.writes);
}

fn world_command_variants(x: &khora_core::script::command::WorldCommand) {
    match x {
        khora_core::script::command::WorldCommand::SetTranslation { .. } => {}
        khora_core::script::command::WorldCommand::Translate { .. } => {}
        khora_core::script::command::WorldCommand::SetRotation { .. } => {}
        khora_core::script::command::WorldCommand::SetScale { .. } => {}
        khora_core::script::command::WorldCommand::SetParent { .. } => {}
        khora_core::script::command::WorldCommand::Spawn { .. } => {}
        khora_core::script::command::WorldCommand::Despawn { .. } => {}
        khora_core::script::command::WorldCommand::SetComponent { .. } => {}
        khora_core::script::command::WorldCommand::AddComponent { .. } => {}
        khora_core::script::command::WorldCommand::RemoveComponent { .. } => {}
    }
}

fn write_target_variants(x: &khora_core::script::command::WriteTarget) {
    match x {
        khora_core::script::command::WriteTarget::Translation => {}
        khora_core::script::command::WriteTarget::Rotation => {}
        khora_core::script::command::WriteTarget::Scale => {}
        khora_core::script::command::WriteTarget::Parent => {}
        khora_core::script::command::WriteTarget::Existence => {}
        khora_core::script::command::WriteTarget::Component(..) => {}
    }
}

fn script_event_fields(x: &khora_core::script::event::ScriptEvent) {
    let _ = (&x.target, &x.name, &x.args);
}

fn pending_sequence_fields(x: &khora_core::script::snapshot::PendingSequence) {
    let _ = (&x.fingerprint, &x.remaining, &x.machine);
}

fn script_snapshot_fields(x: &khora_core::script::snapshot::ScriptSnapshot) {
    let _ = (&x.fields, &x.state, &x.state_fields, &x.timers, &x.pending);
}

fn timer_remaining_fields(x: &khora_core::script::snapshot::TimerRemaining) {
    let _ = (
        &x.repeating,
        &x.interval,
        &x.state,
        &x.ordinal,
        &x.remaining,
    );
}

fn script_value_variants(x: &khora_core::script::value::ScriptValue) {
    match x {
        khora_core::script::value::ScriptValue::Unit => {}
        khora_core::script::value::ScriptValue::Bool(..) => {}
        khora_core::script::value::ScriptValue::Int(..) => {}
        khora_core::script::value::ScriptValue::Float(..) => {}
        khora_core::script::value::ScriptValue::Str(..) => {}
        khora_core::script::value::ScriptValue::Vec2(..) => {}
        khora_core::script::value::ScriptValue::Vec3(..) => {}
        khora_core::script::value::ScriptValue::Vec4(..) => {}
        khora_core::script::value::ScriptValue::Quat(..) => {}
        khora_core::script::value::ScriptValue::Color(..) => {}
        khora_core::script::value::ScriptValue::Entity(..) => {}
        khora_core::script::value::ScriptValue::Array(..) => {}
        khora_core::script::value::ScriptValue::Struct(..) => {}
    }
}

fn script_state_update_fields(x: &khora_core::script::writeback::ScriptStateUpdate) {
    let _ = (&x.entity, &x.behavior, &x.snapshot);
}

fn telemetry_event_variants(x: &khora_core::telemetry::event::TelemetryEvent) {
    match x {
        khora_core::telemetry::event::TelemetryEvent::MetricUpdate { .. } => {}
        khora_core::telemetry::event::TelemetryEvent::ResourceReport(..) => {}
        khora_core::telemetry::event::TelemetryEvent::HardwareReport(..) => {}
        khora_core::telemetry::event::TelemetryEvent::GpuReport(..) => {}
        khora_core::telemetry::event::TelemetryEvent::PhaseChange(..) => {}
        khora_core::telemetry::event::TelemetryEvent::AgentCost { .. } => {}
        khora_core::telemetry::event::TelemetryEvent::WavePlan { .. } => {}
        khora_core::telemetry::event::TelemetryEvent::ComponentAccess { .. } => {}
    }
}

fn metric_fields(x: &khora_core::telemetry::metrics::Metric) {
    let _ = (&x.metadata, &x.value);
}

fn metric_id_fields(x: &khora_core::telemetry::metrics::MetricId) {
    let _ = (&x.namespace, &x.name, &x.labels);
}

fn metric_metadata_fields(x: &khora_core::telemetry::metrics::MetricMetadata) {
    let _ = (
        &x.id,
        &x.metric_type,
        &x.description,
        &x.unit,
        &x.created_at,
        &x.last_updated,
    );
}

fn metric_type_variants(x: &khora_core::telemetry::metrics::MetricType) {
    match x {
        khora_core::telemetry::metrics::MetricType::Counter => {}
        khora_core::telemetry::metrics::MetricType::Gauge => {}
        khora_core::telemetry::metrics::MetricType::Histogram => {}
    }
}

fn metric_value_variants(x: &khora_core::telemetry::metrics::MetricValue) {
    match x {
        khora_core::telemetry::metrics::MetricValue::Counter(..) => {}
        khora_core::telemetry::metrics::MetricValue::Gauge(..) => {}
        khora_core::telemetry::metrics::MetricValue::Histogram { .. } => {}
    }
}

fn metrics_error_variants(x: &khora_core::telemetry::metrics::MetricsError) {
    match x {
        khora_core::telemetry::metrics::MetricsError::MetricNotFound(..) => {}
        khora_core::telemetry::metrics::MetricsError::TypeMismatch { .. } => {}
        khora_core::telemetry::metrics::MetricsError::StorageError(..) => {}
        khora_core::telemetry::metrics::MetricsError::InvalidOperation(..) => {}
    }
}

fn gpu_report_fields(x: &khora_core::telemetry::monitoring::GpuReport) {
    let _ = (
        &x.frame_number,
        &x.hook_timings_us,
        &x.cpu_preparation_time_us,
        &x.cpu_submission_time_us,
        &x.draw_calls,
        &x.triangles_rendered,
    );
}

fn hardware_report_fields(x: &khora_core::telemetry::monitoring::HardwareReport) {
    let _ = (
        &x.thermal,
        &x.battery,
        &x.cpu_load,
        &x.gpu_load,
        &x.gpu_timings,
    );
}

fn memory_report_fields(x: &khora_core::telemetry::monitoring::MemoryReport) {
    let _ = (
        &x.current_usage_bytes,
        &x.peak_usage_bytes,
        &x.allocation_delta_bytes,
        &x.sample_count,
        &x.total_allocations,
        &x.total_deallocations,
        &x.total_reallocations,
        &x.bytes_allocated_lifetime,
        &x.bytes_deallocated_lifetime,
        &x.large_allocations,
        &x.large_allocation_bytes,
        &x.small_allocations,
        &x.small_allocation_bytes,
        &x.fragmentation_ratio,
        &x.allocation_efficiency,
        &x.average_allocation_size,
    );
}

fn monitored_resource_type_variants(x: &khora_core::telemetry::monitoring::MonitoredResourceType) {
    match x {
        khora_core::telemetry::monitoring::MonitoredResourceType::Vram => {}
        khora_core::telemetry::monitoring::MonitoredResourceType::SystemRam => {}
        khora_core::telemetry::monitoring::MonitoredResourceType::Gpu => {}
        khora_core::telemetry::monitoring::MonitoredResourceType::Hardware => {}
    }
}

fn resource_usage_report_fields(x: &khora_core::telemetry::monitoring::ResourceUsageReport) {
    let _ = (&x.current_bytes, &x.peak_bytes, &x.total_capacity_bytes);
}

fn vram_report_fields(x: &khora_core::telemetry::monitoring::VramReport) {
    let _ = (
        &x.current_usage_bytes,
        &x.peak_usage_bytes,
        &x.total_capacity_bytes,
    );
}

fn time_fields(x: &khora_core::time::Time) {
    let _ = (
        &x.delta_seconds,
        &x.fixed_delta_seconds,
        &x.interpolation_alpha,
        &x.frame,
    );
}

// ---------------------------------------------------------------------------
// Trait items, named through a generic parameter. A generic body is
// type-checked where it is written, so these need no implementor and are
// never instantiated. A re-exported trait is checked for identity by
// forwarding a bound both ways.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
mod trait_items {
    fn agent_trait_items<T: khora_core::agent::Agent>() {
        let _ = <T as khora_core::agent::Agent>::id;
        let _ = <T as khora_core::agent::Agent>::negotiate;
        let _ = <T as khora_core::agent::Agent>::apply_budget;
        let _ = <T as khora_core::agent::Agent>::report_status;
        let _ = <T as khora_core::agent::Agent>::on_initialize;
        let _ = <T as khora_core::agent::Agent>::execute;
        let _ = <T as khora_core::agent::Agent>::execution_timing;
        let _ = <T as khora_core::agent::Agent>::access;
        let _ = <T as khora_core::agent::Agent>::contention;
        let _ = <T as khora_core::agent::Agent>::as_any;
        let _ = <T as khora_core::agent::Agent>::as_any_mut;
    }

    fn as_any_trait_items<T: khora_core::asset::AsAny>() {
        let _ = <T as khora_core::asset::AsAny>::as_any;
    }

    fn asset_trait_items<T: khora_core::asset::Asset>() {}

    fn material_trait_items<T: khora_core::asset::Material>() {
        let _ = <T as khora_core::asset::Material>::base_color;
        let _ = <T as khora_core::asset::Material>::emissive_color;
        let _ = <T as khora_core::asset::Material>::specular_power;
        let _ = <T as khora_core::asset::Material>::ambient_color;
        let _ = <T as khora_core::asset::Material>::metallic;
        let _ = <T as khora_core::asset::Material>::roughness;
        let _ = <T as khora_core::asset::Material>::base_color_texture;
        let _ = <T as khora_core::asset::Material>::metallic_roughness_texture;
        let _ = <T as khora_core::asset::Material>::normal_map;
        let _ = <T as khora_core::asset::Material>::emissive_texture;
        let _ = <T as khora_core::asset::Material>::occlusion_map;
        let _ = <T as khora_core::asset::Material>::alpha_mode;
        let _ = <T as khora_core::asset::Material>::double_sided;
    }

    fn material_clone_trait_items<T: khora_core::asset::MaterialClone>() {
        let _ = <T as khora_core::asset::MaterialClone>::clone_box;
    }

    fn audio_device_trait_items<T: khora_core::audio::device::AudioDevice>() {
        let _ = <T as khora_core::audio::device::AudioDevice>::open;
    }

    fn audio_device_trait_identity<T: khora_core::audio::AudioDevice>() {
        audio_device_trait_items::<T>();
    }

    fn audio_device_trait_identity_rev<T: khora_core::audio::device::AudioDevice>() {
        audio_device_trait_identity::<T>();
    }

    fn audio_stream_trait_items<T: khora_core::audio::device::AudioStream>() {
        let _ = <T as khora_core::audio::device::AudioStream>::stream_info;
    }

    fn audio_stream_trait_identity<T: khora_core::audio::AudioStream>() {
        audio_stream_trait_items::<T>();
    }

    fn audio_stream_trait_identity_rev<T: khora_core::audio::device::AudioStream>() {
        audio_stream_trait_identity::<T>();
    }

    fn audio_mix_bus_trait_items<T: khora_core::audio::mix_bus::AudioMixBus>() {
        let _ = <T as khora_core::audio::mix_bus::AudioMixBus>::stream_info;
        let _ = <T as khora_core::audio::mix_bus::AudioMixBus>::write_block;
        let _ = <T as khora_core::audio::mix_bus::AudioMixBus>::pull;
    }

    fn audio_mix_bus_trait_identity<T: khora_core::audio::AudioMixBus>() {
        audio_mix_bus_trait_items::<T>();
    }

    fn audio_mix_bus_trait_identity_rev<T: khora_core::audio::mix_bus::AudioMixBus>() {
        audio_mix_bus_trait_identity::<T>();
    }

    fn supersedes_trait_items<T: khora_core::event::Supersedes>() {
        let _ = <T as khora_core::event::Supersedes>::COALESCES;
        let _ = <T as khora_core::event::Supersedes>::supersedes;
    }

    fn lane_trait_items<T: khora_core::lane::Lane>() {
        let _ = <T as khora_core::lane::Lane>::strategy_name;
        let _ = <T as khora_core::lane::Lane>::lane_kind;
        let _ = <T as khora_core::lane::Lane>::estimate_cost;
        let _ = <T as khora_core::lane::Lane>::on_initialize;
        let _ = <T as khora_core::lane::Lane>::execute;
        let _ = <T as khora_core::lane::Lane>::on_shutdown;
        let _ = <T as khora_core::lane::Lane>::as_any;
        let _ = <T as khora_core::lane::Lane>::as_any_mut;
    }

    fn physics_provider_trait_items<T: khora_core::physics::PhysicsProvider>() {
        let _ = <T as khora_core::physics::PhysicsProvider>::step;
        let _ = <T as khora_core::physics::PhysicsProvider>::set_gravity;
        let _ = <T as khora_core::physics::PhysicsProvider>::add_body;
        let _ = <T as khora_core::physics::PhysicsProvider>::remove_body;
        let _ = <T as khora_core::physics::PhysicsProvider>::add_collider;
        let _ = <T as khora_core::physics::PhysicsProvider>::remove_collider;
        let _ = <T as khora_core::physics::PhysicsProvider>::get_body_transform;
        let _ = <T as khora_core::physics::PhysicsProvider>::get_body_velocity;
        let _ = <T as khora_core::physics::PhysicsProvider>::set_body_transform;
        let _ = <T as khora_core::physics::PhysicsProvider>::get_all_bodies;
        let _ = <T as khora_core::physics::PhysicsProvider>::get_all_colliders;
        let _ = <T as khora_core::physics::PhysicsProvider>::update_body_properties;
        let _ = <T as khora_core::physics::PhysicsProvider>::update_collider_properties;
        let _ = <T as khora_core::physics::PhysicsProvider>::get_debug_render_data;
        let _ = <T as khora_core::physics::PhysicsProvider>::cast_ray;
        let _ = <T as khora_core::physics::PhysicsProvider>::take_collision_events;
        let _ = <T as khora_core::physics::PhysicsProvider>::entity_of;
        let _ = <T as khora_core::physics::PhysicsProvider>::move_character;
    }

    fn hardware_monitor_trait_items<T: khora_core::platform::HardwareMonitor>() {
        let _ = <T as khora_core::platform::HardwareMonitor>::thermal_status;
        let _ = <T as khora_core::platform::HardwareMonitor>::battery_level;
        let _ = <T as khora_core::platform::HardwareMonitor>::cpu_load;
    }

    fn khora_window_trait_items<T: khora_core::platform::window::KhoraWindow>() {
        let _ = <T as khora_core::platform::window::KhoraWindow>::inner_size;
        let _ = <T as khora_core::platform::window::KhoraWindow>::scale_factor;
        let _ = <T as khora_core::platform::window::KhoraWindow>::request_redraw;
        let _ = <T as khora_core::platform::window::KhoraWindow>::clone_handle_arc;
        let _ = <T as khora_core::platform::window::KhoraWindow>::id;
    }

    fn khora_window_trait_identity<T: khora_core::platform::KhoraWindow>() {
        khora_window_trait_items::<T>();
    }

    fn khora_window_trait_identity_rev<T: khora_core::platform::window::KhoraWindow>() {
        khora_window_trait_identity::<T>();
    }

    fn window_handle_trait_items<T: khora_core::platform::window::WindowHandle>() {}

    fn window_handle_trait_identity<T: khora_core::platform::WindowHandle>() {
        window_handle_trait_items::<T>();
    }

    fn window_handle_trait_identity_rev<T: khora_core::platform::window::WindowHandle>() {
        window_handle_trait_identity::<T>();
    }

    fn registry_kind_trait_items<T: khora_core::runtime::typed_registry::RegistryKind>() {
        let _ = <T as khora_core::runtime::typed_registry::RegistryKind>::NAME;
        let _ = <T as khora_core::runtime::typed_registry::RegistryKind>::NOUN;
    }

    fn registry_kind_trait_identity<T: khora_core::runtime::RegistryKind>() {
        registry_kind_trait_items::<T>();
    }

    fn registry_kind_trait_identity_rev<T: khora_core::runtime::typed_registry::RegistryKind>() {
        registry_kind_trait_identity::<T>();
    }

    fn resource_monitor_trait_items<T: khora_core::telemetry::monitoring::ResourceMonitor>() {
        let _ = <T as khora_core::telemetry::monitoring::ResourceMonitor>::monitor_id;
        let _ = <T as khora_core::telemetry::monitoring::ResourceMonitor>::resource_type;
        let _ = <T as khora_core::telemetry::monitoring::ResourceMonitor>::get_usage_report;
        let _ = <T as khora_core::telemetry::monitoring::ResourceMonitor>::get_gpu_report;
        let _ = <T as khora_core::telemetry::monitoring::ResourceMonitor>::get_hardware_report;
        let _ = <T as khora_core::telemetry::monitoring::ResourceMonitor>::get_metrics;
        let _ = <T as khora_core::telemetry::monitoring::ResourceMonitor>::as_any;
        let _ = <T as khora_core::telemetry::monitoring::ResourceMonitor>::update;
    }

    fn resource_monitor_trait_identity<T: khora_core::telemetry::ResourceMonitor>() {
        resource_monitor_trait_items::<T>();
    }

    fn resource_monitor_trait_identity_rev<
        T: khora_core::telemetry::monitoring::ResourceMonitor,
    >() {
        resource_monitor_trait_identity::<T>();
    }

    fn vram_provider_trait_items<T: khora_core::telemetry::monitoring::VramProvider>() {
        let _ = <T as khora_core::telemetry::monitoring::VramProvider>::get_vram_usage_mb;
        let _ = <T as khora_core::telemetry::monitoring::VramProvider>::get_vram_peak_mb;
        let _ = <T as khora_core::telemetry::monitoring::VramProvider>::get_vram_capacity_mb;
    }

    fn vram_provider_trait_identity<T: khora_core::telemetry::VramProvider>() {
        vram_provider_trait_items::<T>();
    }

    fn vram_provider_trait_identity_rev<T: khora_core::telemetry::monitoring::VramProvider>() {
        vram_provider_trait_identity::<T>();
    }
}

#[test]
fn module_crate_root_paths_still_resolve() {
    let _ = type_name::<khora_core::runtime::Backends>();
    let _ = type_name::<khora_core::Backends>();
    same_type(
        PhantomData::<khora_core::runtime::Backends>,
        PhantomData::<khora_core::Backends>,
    );
    let _ = type_name::<khora_core::runtime::Resources>();
    let _ = type_name::<khora_core::Resources>();
    same_type(
        PhantomData::<khora_core::runtime::Resources>,
        PhantomData::<khora_core::Resources>,
    );
    let _ = type_name::<khora_core::runtime::Services>();
    let _ = type_name::<khora_core::Services>();
    same_type(
        PhantomData::<khora_core::runtime::Services>,
        PhantomData::<khora_core::Services>,
    );
}

#[test]
fn module_agent_paths_still_resolve() {
    // trait `khora_core::agent::Agent`: see `agent_trait_items`
    let _ = type_name::<khora_core::agent::AgentAccess>();
    let _ = agent_access_variants as fn(&khora_core::agent::AgentAccess);
    is_debug::<khora_core::agent::AgentAccess>();
    is_clone::<khora_core::agent::AgentAccess>();
    is_copy::<khora_core::agent::AgentAccess>();
    is_partial_eq::<khora_core::agent::AgentAccess>();
    is_eq::<khora_core::agent::AgentAccess>();
    is_default::<khora_core::agent::AgentAccess>();
    let _ = type_name::<khora_core::agent::completion::AgentCompletionMap>();
    let _ = type_name::<khora_core::agent::AgentCompletionMap>();
    same_type(
        PhantomData::<khora_core::agent::AgentCompletionMap>,
        PhantomData::<khora_core::agent::completion::AgentCompletionMap>,
    );
    let _ = khora_core::agent::completion::AgentCompletionMap::new;
    let _ = khora_core::agent::completion::AgentCompletionMap::mark;
    let _ = khora_core::agent::completion::AgentCompletionMap::outcome;
    let _ = khora_core::agent::completion::AgentCompletionMap::is_done;
    let _ = khora_core::agent::completion::AgentCompletionMap::wait;
    let _ = khora_core::agent::completion::AgentCompletionMap::known_ids;
    let _ = type_name::<khora_core::agent::completion::AgentDone>();
    let _ = type_name::<khora_core::agent::AgentDone>();
    same_type(
        PhantomData::<khora_core::agent::AgentDone>,
        PhantomData::<khora_core::agent::completion::AgentDone>,
    );
    let _ = type_name::<khora_core::agent::completion::CompletionOutcome>();
    let _ = type_name::<khora_core::agent::CompletionOutcome>();
    same_type(
        PhantomData::<khora_core::agent::CompletionOutcome>,
        PhantomData::<khora_core::agent::completion::CompletionOutcome>,
    );
    let _ = completion_outcome_variants as fn(&khora_core::agent::completion::CompletionOutcome);
    is_debug::<khora_core::agent::completion::CompletionOutcome>();
    is_clone::<khora_core::agent::completion::CompletionOutcome>();
    is_copy::<khora_core::agent::completion::CompletionOutcome>();
    is_partial_eq::<khora_core::agent::completion::CompletionOutcome>();
    is_eq::<khora_core::agent::completion::CompletionOutcome>();
    let _ = type_name::<khora_core::agent::contention::Contention>();
    let _ = type_name::<khora_core::agent::Contention>();
    same_type(
        PhantomData::<khora_core::agent::Contention>,
        PhantomData::<khora_core::agent::contention::Contention>,
    );
    let _ = contention_fields as fn(&khora_core::agent::contention::Contention);
    let _ = khora_core::agent::contention::Contention::none;
    let _: fn(
        khora_core::agent::contention::Contention,
        Vec<std::any::TypeId>,
    ) -> khora_core::agent::contention::Contention =
        khora_core::agent::contention::Contention::writing_deck;
    let _ = khora_core::agent::contention::Contention::reading::<u32>;
    let _ = khora_core::agent::contention::Contention::locking::<u32>;
    let _ = khora_core::agent::contention::Contention::may_reach;
    let _ = khora_core::agent::contention::Contention::may_lock;
    let _ = khora_core::agent::contention::Contention::conflicts_with;
    is_debug::<khora_core::agent::contention::Contention>();
    is_clone::<khora_core::agent::contention::Contention>();
    is_default::<khora_core::agent::contention::Contention>();
    is_partial_eq::<khora_core::agent::contention::Contention>();
    is_eq::<khora_core::agent::contention::Contention>();
    let _ = type_name::<khora_core::agent::dependency::AgentDependency>();
    let _ = type_name::<khora_core::agent::AgentDependency>();
    same_type(
        PhantomData::<khora_core::agent::AgentDependency>,
        PhantomData::<khora_core::agent::dependency::AgentDependency>,
    );
    let _ = agent_dependency_fields as fn(&khora_core::agent::dependency::AgentDependency);
    is_debug::<khora_core::agent::dependency::AgentDependency>();
    is_clone::<khora_core::agent::dependency::AgentDependency>();
    let _ = type_name::<khora_core::agent::dependency::DependencyCondition>();
    let _ = type_name::<khora_core::agent::DependencyCondition>();
    same_type(
        PhantomData::<khora_core::agent::DependencyCondition>,
        PhantomData::<khora_core::agent::dependency::DependencyCondition>,
    );
    let _ =
        dependency_condition_variants as fn(&khora_core::agent::dependency::DependencyCondition);
    is_debug::<khora_core::agent::dependency::DependencyCondition>();
    is_clone::<khora_core::agent::dependency::DependencyCondition>();
    let _ = type_name::<khora_core::agent::dependency::DependencyKind>();
    let _ = type_name::<khora_core::agent::DependencyKind>();
    same_type(
        PhantomData::<khora_core::agent::DependencyKind>,
        PhantomData::<khora_core::agent::dependency::DependencyKind>,
    );
    let _ = dependency_kind_variants as fn(&khora_core::agent::dependency::DependencyKind);
    is_debug::<khora_core::agent::dependency::DependencyKind>();
    is_clone::<khora_core::agent::dependency::DependencyKind>();
    let _ = type_name::<khora_core::agent::execution_phase::ExecutionPhase>();
    let _ = type_name::<khora_core::agent::ExecutionPhase>();
    same_type(
        PhantomData::<khora_core::agent::ExecutionPhase>,
        PhantomData::<khora_core::agent::execution_phase::ExecutionPhase>,
    );
    let _ = khora_core::agent::execution_phase::ExecutionPhase::INIT;
    let _ = khora_core::agent::execution_phase::ExecutionPhase::OBSERVE;
    let _ = khora_core::agent::execution_phase::ExecutionPhase::TRANSFORM;
    let _ = khora_core::agent::execution_phase::ExecutionPhase::MUTATE;
    let _ = khora_core::agent::execution_phase::ExecutionPhase::OUTPUT;
    let _ = khora_core::agent::execution_phase::ExecutionPhase::FINALIZE;
    let _ = khora_core::agent::execution_phase::ExecutionPhase::DEFAULT_ORDER;
    let _ = khora_core::agent::execution_phase::ExecutionPhase::custom;
    let _ = khora_core::agent::execution_phase::ExecutionPhase::id;
    let _ = khora_core::agent::execution_phase::ExecutionPhase::is_builtin;
    is_debug::<khora_core::agent::execution_phase::ExecutionPhase>();
    is_clone::<khora_core::agent::execution_phase::ExecutionPhase>();
    is_copy::<khora_core::agent::execution_phase::ExecutionPhase>();
    is_partial_eq::<khora_core::agent::execution_phase::ExecutionPhase>();
    is_eq::<khora_core::agent::execution_phase::ExecutionPhase>();
    is_hash::<khora_core::agent::execution_phase::ExecutionPhase>();
    is_display::<khora_core::agent::execution_phase::ExecutionPhase>();
    let _ = type_name::<khora_core::agent::mode::EngineMode>();
    let _ = type_name::<khora_core::agent::EngineMode>();
    same_type(
        PhantomData::<khora_core::agent::EngineMode>,
        PhantomData::<khora_core::agent::mode::EngineMode>,
    );
    let _ = engine_mode_variants as fn(&khora_core::agent::mode::EngineMode);
    let _ = khora_core::agent::mode::EngineMode::from_name;
    let _ = khora_core::agent::mode::EngineMode::name;
    is_debug::<khora_core::agent::mode::EngineMode>();
    is_clone::<khora_core::agent::mode::EngineMode>();
    is_partial_eq::<khora_core::agent::mode::EngineMode>();
    is_eq::<khora_core::agent::mode::EngineMode>();
    is_hash::<khora_core::agent::mode::EngineMode>();
    let _ = type_name::<khora_core::agent::timing::AgentImportance>();
    let _ = type_name::<khora_core::agent::AgentImportance>();
    same_type(
        PhantomData::<khora_core::agent::AgentImportance>,
        PhantomData::<khora_core::agent::timing::AgentImportance>,
    );
    let _ = agent_importance_variants as fn(&khora_core::agent::timing::AgentImportance);
    let _ = khora_core::agent::timing::AgentImportance::is_negotiable;
    is_debug::<khora_core::agent::timing::AgentImportance>();
    is_clone::<khora_core::agent::timing::AgentImportance>();
    is_copy::<khora_core::agent::timing::AgentImportance>();
    is_partial_eq::<khora_core::agent::timing::AgentImportance>();
    is_eq::<khora_core::agent::timing::AgentImportance>();
    is_partial_ord::<khora_core::agent::timing::AgentImportance>();
    is_ord::<khora_core::agent::timing::AgentImportance>();
    let _ = type_name::<khora_core::agent::timing::ExecutionTiming>();
    let _ = type_name::<khora_core::agent::ExecutionTiming>();
    same_type(
        PhantomData::<khora_core::agent::ExecutionTiming>,
        PhantomData::<khora_core::agent::timing::ExecutionTiming>,
    );
    let _ = execution_timing_fields as fn(&khora_core::agent::timing::ExecutionTiming);
    is_debug::<khora_core::agent::timing::ExecutionTiming>();
    is_clone::<khora_core::agent::timing::ExecutionTiming>();
    is_default::<khora_core::agent::timing::ExecutionTiming>();
}

#[test]
fn module_asset_paths_still_resolve() {
    let _ = type_name::<khora_core::asset::AlphaMode>();
    let _ = alpha_mode_variants as fn(&khora_core::asset::AlphaMode);
    is_debug::<khora_core::asset::AlphaMode>();
    is_clone::<khora_core::asset::AlphaMode>();
    is_copy::<khora_core::asset::AlphaMode>();
    is_partial_eq::<khora_core::asset::AlphaMode>();
    is_default::<khora_core::asset::AlphaMode>();
    is_serialize::<khora_core::asset::AlphaMode>();
    is_deserialize_owned::<khora_core::asset::AlphaMode>();
    is_encode::<khora_core::asset::AlphaMode>();
    is_decode::<khora_core::asset::AlphaMode>();
    is_borrow_decode::<khora_core::asset::AlphaMode>();
    // trait `khora_core::asset::AsAny`: see `as_any_trait_items`
    // trait `khora_core::asset::Asset`: see `asset_trait_items`
    let _ = type_name::<khora_core::asset::Handle<khora_core::asset::EmissiveMaterial>>();
    let _ = type_name::<khora_core::asset::AssetHandle<khora_core::asset::EmissiveMaterial>>();
    same_type(
        PhantomData::<khora_core::asset::Handle<khora_core::asset::EmissiveMaterial>>,
        PhantomData::<khora_core::asset::AssetHandle<khora_core::asset::EmissiveMaterial>>,
    );
    let _ = <khora_core::asset::AssetHandle<khora_core::asset::EmissiveMaterial>>::new;
    let _ = <khora_core::asset::AssetHandle<khora_core::asset::EmissiveMaterial>>::dangling;
    is_debug::<khora_core::asset::AssetHandle<khora_core::asset::EmissiveMaterial>>();
    is_clone::<khora_core::asset::AssetHandle<khora_core::asset::EmissiveMaterial>>();
    is_deref::<khora_core::asset::AssetHandle<khora_core::asset::EmissiveMaterial>>();
    is_partial_eq::<khora_core::asset::AssetHandle<khora_core::asset::EmissiveMaterial>>();
    is_eq::<khora_core::asset::AssetHandle<khora_core::asset::EmissiveMaterial>>();
    is_default::<khora_core::asset::AssetHandle<khora_core::asset::EmissiveMaterial>>();
    let _ = type_name::<khora_core::asset::AssetMetadata>();
    let _ = asset_metadata_fields as fn(&khora_core::asset::AssetMetadata);
    is_debug::<khora_core::asset::AssetMetadata>();
    is_clone::<khora_core::asset::AssetMetadata>();
    is_serialize::<khora_core::asset::AssetMetadata>();
    is_deserialize_owned::<khora_core::asset::AssetMetadata>();
    let _ = type_name::<khora_core::asset::AssetSource>();
    let _ = asset_source_variants as fn(&khora_core::asset::AssetSource);
    is_debug::<khora_core::asset::AssetSource>();
    is_clone::<khora_core::asset::AssetSource>();
    is_serialize::<khora_core::asset::AssetSource>();
    is_deserialize_owned::<khora_core::asset::AssetSource>();
    let _ = type_name::<khora_core::asset::AssetUUID>();
    let _ = khora_core::asset::AssetUUID::new;
    let _ = khora_core::asset::AssetUUID::new_v5;
    is_debug::<khora_core::asset::AssetUUID>();
    is_clone::<khora_core::asset::AssetUUID>();
    is_copy::<khora_core::asset::AssetUUID>();
    is_partial_eq::<khora_core::asset::AssetUUID>();
    is_eq::<khora_core::asset::AssetUUID>();
    is_partial_ord::<khora_core::asset::AssetUUID>();
    is_ord::<khora_core::asset::AssetUUID>();
    is_hash::<khora_core::asset::AssetUUID>();
    is_serialize::<khora_core::asset::AssetUUID>();
    is_deserialize_owned::<khora_core::asset::AssetUUID>();
    is_encode::<khora_core::asset::AssetUUID>();
    is_decode::<khora_core::asset::AssetUUID>();
    is_borrow_decode::<khora_core::asset::AssetUUID>();
    is_default::<khora_core::asset::AssetUUID>();
    let _ = type_name::<khora_core::asset::CompressionKind>();
    let _ = compression_kind_variants as fn(&khora_core::asset::CompressionKind);
    is_debug::<khora_core::asset::CompressionKind>();
    is_clone::<khora_core::asset::CompressionKind>();
    is_copy::<khora_core::asset::CompressionKind>();
    is_default::<khora_core::asset::CompressionKind>();
    is_partial_eq::<khora_core::asset::CompressionKind>();
    is_eq::<khora_core::asset::CompressionKind>();
    is_serialize::<khora_core::asset::CompressionKind>();
    is_deserialize_owned::<khora_core::asset::CompressionKind>();
    let _ = type_name::<khora_core::asset::EmissiveMaterial>();
    let _ = emissive_material_fields as fn(&khora_core::asset::EmissiveMaterial);
    is_clone::<khora_core::asset::EmissiveMaterial>();
    is_debug::<khora_core::asset::EmissiveMaterial>();
    is_serialize::<khora_core::asset::EmissiveMaterial>();
    is_deserialize_owned::<khora_core::asset::EmissiveMaterial>();
    is_encode::<khora_core::asset::EmissiveMaterial>();
    is_decode::<khora_core::asset::EmissiveMaterial>();
    is_borrow_decode::<khora_core::asset::EmissiveMaterial>();
    is_default::<khora_core::asset::EmissiveMaterial>();
    is_asset::<khora_core::asset::EmissiveMaterial>();
    is_material::<khora_core::asset::EmissiveMaterial>();
    // trait `khora_core::asset::Material`: see `material_trait_items`
    // trait `khora_core::asset::MaterialClone`: see `material_clone_trait_items`
    let _ = type_name::<khora_core::asset::StandardMaterial>();
    let _ = standard_material_fields as fn(&khora_core::asset::StandardMaterial);
    is_clone::<khora_core::asset::StandardMaterial>();
    is_debug::<khora_core::asset::StandardMaterial>();
    is_serialize::<khora_core::asset::StandardMaterial>();
    is_deserialize_owned::<khora_core::asset::StandardMaterial>();
    is_encode::<khora_core::asset::StandardMaterial>();
    is_decode::<khora_core::asset::StandardMaterial>();
    is_borrow_decode::<khora_core::asset::StandardMaterial>();
    is_default::<khora_core::asset::StandardMaterial>();
    is_asset::<khora_core::asset::StandardMaterial>();
    is_material::<khora_core::asset::StandardMaterial>();
    let _ = type_name::<khora_core::asset::UnlitMaterial>();
    let _ = unlit_material_fields as fn(&khora_core::asset::UnlitMaterial);
    is_clone::<khora_core::asset::UnlitMaterial>();
    is_debug::<khora_core::asset::UnlitMaterial>();
    is_serialize::<khora_core::asset::UnlitMaterial>();
    is_deserialize_owned::<khora_core::asset::UnlitMaterial>();
    is_encode::<khora_core::asset::UnlitMaterial>();
    is_decode::<khora_core::asset::UnlitMaterial>();
    is_borrow_decode::<khora_core::asset::UnlitMaterial>();
    is_default::<khora_core::asset::UnlitMaterial>();
    is_asset::<khora_core::asset::UnlitMaterial>();
    is_material::<khora_core::asset::UnlitMaterial>();
    let _ = type_name::<khora_core::asset::WireframeMaterial>();
    let _ = wireframe_material_fields as fn(&khora_core::asset::WireframeMaterial);
    is_clone::<khora_core::asset::WireframeMaterial>();
    is_debug::<khora_core::asset::WireframeMaterial>();
    is_serialize::<khora_core::asset::WireframeMaterial>();
    is_deserialize_owned::<khora_core::asset::WireframeMaterial>();
    is_encode::<khora_core::asset::WireframeMaterial>();
    is_decode::<khora_core::asset::WireframeMaterial>();
    is_borrow_decode::<khora_core::asset::WireframeMaterial>();
    is_default::<khora_core::asset::WireframeMaterial>();
    is_asset::<khora_core::asset::WireframeMaterial>();
    is_material::<khora_core::asset::WireframeMaterial>();
    let _ = khora_core::asset::asset_key;
    let _ = type_name::<khora_core::asset::font::Font>();
    let _ = font_fields as fn(&khora_core::asset::font::Font);
    is_debug::<khora_core::asset::font::Font>();
    is_clone::<khora_core::asset::font::Font>();
    is_serialize::<khora_core::asset::font::Font>();
    is_deserialize_owned::<khora_core::asset::font::Font>();
    is_encode::<khora_core::asset::font::Font>();
    is_decode::<khora_core::asset::font::Font>();
    is_borrow_decode::<khora_core::asset::font::Font>();
    is_asset::<khora_core::asset::font::Font>();
    let _ = type_name::<khora_core::asset::script::ScriptModule>();
    let _ = type_name::<khora_core::asset::ScriptModule>();
    same_type(
        PhantomData::<khora_core::asset::ScriptModule>,
        PhantomData::<khora_core::asset::script::ScriptModule>,
    );
    let _ = script_module_fields as fn(&khora_core::asset::script::ScriptModule);
    let _: fn(String, Vec<String>) -> khora_core::asset::script::ScriptModule =
        khora_core::asset::script::ScriptModule::new;
    let _ = khora_core::asset::script::ScriptModule::is_leaf;
    is_debug::<khora_core::asset::script::ScriptModule>();
    is_clone::<khora_core::asset::script::ScriptModule>();
    is_default::<khora_core::asset::script::ScriptModule>();
    is_partial_eq::<khora_core::asset::script::ScriptModule>();
    is_eq::<khora_core::asset::script::ScriptModule>();
    is_asset::<khora_core::asset::script::ScriptModule>();
}

#[test]
fn module_audio_paths_still_resolve() {
    // trait `khora_core::audio::device::AudioDevice`: see `audio_device_trait_items`
    // trait `khora_core::audio::AudioDevice`: see `audio_device_trait_items`
    // trait `khora_core::audio::device::AudioStream`: see `audio_stream_trait_items`
    // trait `khora_core::audio::AudioStream`: see `audio_stream_trait_items`
    let _ = type_name::<khora_core::audio::device::StreamInfo>();
    let _ = type_name::<khora_core::audio::StreamInfo>();
    same_type(
        PhantomData::<khora_core::audio::StreamInfo>,
        PhantomData::<khora_core::audio::device::StreamInfo>,
    );
    let _ = stream_info_fields as fn(&khora_core::audio::device::StreamInfo);
    is_debug::<khora_core::audio::device::StreamInfo>();
    is_clone::<khora_core::audio::device::StreamInfo>();
    is_copy::<khora_core::audio::device::StreamInfo>();
    // trait `khora_core::audio::mix_bus::AudioMixBus`: see `audio_mix_bus_trait_items`
    // trait `khora_core::audio::AudioMixBus`: see `audio_mix_bus_trait_items`
}

#[test]
fn module_engine_context_paths_still_resolve() {
    let _ = type_name::<khora_core::engine_context::EngineContext<'static>>();
    let _ = type_name::<khora_core::EngineContext<'static>>();
    same_type(
        PhantomData::<khora_core::EngineContext<'static>>,
        PhantomData::<khora_core::engine_context::EngineContext<'static>>,
    );
    let _ = engine_context_fields as fn(&khora_core::engine_context::EngineContext<'static>);
    let _ = khora_core::engine_context::EngineContext::for_agent;
    let _ = khora_core::engine_context::EngineContext::for_initialisation;
    let _ = khora_core::engine_context::EngineContext::resource::<u32>;
    let _ = khora_core::engine_context::EngineContext::locked::<u32>;
    let _ = khora_core::engine_context::EngineContext::world_ref;
    let _ = khora_core::engine_context::EngineContext::world_mut;
    let _ = type_name::<khora_core::engine_context::WorldAccess<'static>>();
    let _ = type_name::<khora_core::WorldAccess<'static>>();
    same_type(
        PhantomData::<khora_core::WorldAccess<'static>>,
        PhantomData::<khora_core::engine_context::WorldAccess<'static>>,
    );
    let _ = world_access_variants as fn(&khora_core::engine_context::WorldAccess<'static>);
}

#[test]
fn module_agent_gorna_paths_still_resolve() {
    let _ = type_name::<khora_core::agent::gorna::AdaptationMode>();
    let _ = adaptation_mode_variants as fn(&khora_core::agent::gorna::AdaptationMode);
    is_debug::<khora_core::agent::gorna::AdaptationMode>();
    is_clone::<khora_core::agent::gorna::AdaptationMode>();
    is_copy::<khora_core::agent::gorna::AdaptationMode>();
    is_partial_eq::<khora_core::agent::gorna::AdaptationMode>();
    is_eq::<khora_core::agent::gorna::AdaptationMode>();
    is_default::<khora_core::agent::gorna::AdaptationMode>();
    let _ = type_name::<khora_core::agent::gorna::AgentFrameStatus>();
    let _ = agent_frame_status_fields as fn(&khora_core::agent::gorna::AgentFrameStatus);
    is_debug::<khora_core::agent::gorna::AgentFrameStatus>();
    is_clone::<khora_core::agent::gorna::AgentFrameStatus>();
    is_copy::<khora_core::agent::gorna::AgentFrameStatus>();
    is_default::<khora_core::agent::gorna::AgentFrameStatus>();
    let _ = type_name::<khora_core::agent::gorna::AgentFrameStatusMap>();
    let _ = type_name::<khora_core::agent::gorna::AgentHints>();
    let _ = agent_hints_fields as fn(&khora_core::agent::gorna::AgentHints);
    let _ = khora_core::agent::gorna::AgentHints::apply;
    is_debug::<khora_core::agent::gorna::AgentHints>();
    is_clone::<khora_core::agent::gorna::AgentHints>();
    is_copy::<khora_core::agent::gorna::AgentHints>();
    is_default::<khora_core::agent::gorna::AgentHints>();
    is_partial_eq::<khora_core::agent::gorna::AgentHints>();
    let _ = type_name::<khora_core::agent::gorna::AgentId>();
    let _ = agent_id_variants as fn(&khora_core::agent::gorna::AgentId);
    is_debug::<khora_core::agent::gorna::AgentId>();
    is_clone::<khora_core::agent::gorna::AgentId>();
    is_copy::<khora_core::agent::gorna::AgentId>();
    is_partial_eq::<khora_core::agent::gorna::AgentId>();
    is_eq::<khora_core::agent::gorna::AgentId>();
    is_hash::<khora_core::agent::gorna::AgentId>();
    is_serialize::<khora_core::agent::gorna::AgentId>();
    is_deserialize_owned::<khora_core::agent::gorna::AgentId>();
    is_partial_ord::<khora_core::agent::gorna::AgentId>();
    is_ord::<khora_core::agent::gorna::AgentId>();
    is_display::<khora_core::agent::gorna::AgentId>();
    let _ = type_name::<khora_core::agent::gorna::AgentStatus>();
    let _ = agent_status_fields as fn(&khora_core::agent::gorna::AgentStatus);
    is_debug::<khora_core::agent::gorna::AgentStatus>();
    is_clone::<khora_core::agent::gorna::AgentStatus>();
    let _ = type_name::<khora_core::agent::gorna::DecisionTrace>();
    let _ = decision_trace_fields as fn(&khora_core::agent::gorna::DecisionTrace);
    is_debug::<khora_core::agent::gorna::DecisionTrace>();
    is_clone::<khora_core::agent::gorna::DecisionTrace>();
    is_default::<khora_core::agent::gorna::DecisionTrace>();
    let _ = type_name::<khora_core::agent::gorna::EngineHint>();
    let _ = engine_hint_variants as fn(&khora_core::agent::gorna::EngineHint);
    let _ = khora_core::agent::gorna::EngineHint::agent;
    is_debug::<khora_core::agent::gorna::EngineHint>();
    is_clone::<khora_core::agent::gorna::EngineHint>();
    is_copy::<khora_core::agent::gorna::EngineHint>();
    is_partial_eq::<khora_core::agent::gorna::EngineHint>();
    let _ = type_name::<khora_core::agent::gorna::NegotiationRequest>();
    let _ = negotiation_request_fields as fn(&khora_core::agent::gorna::NegotiationRequest);
    is_debug::<khora_core::agent::gorna::NegotiationRequest>();
    is_clone::<khora_core::agent::gorna::NegotiationRequest>();
    let _ = type_name::<khora_core::agent::gorna::NegotiationResponse>();
    let _ = negotiation_response_fields as fn(&khora_core::agent::gorna::NegotiationResponse);
    is_debug::<khora_core::agent::gorna::NegotiationResponse>();
    is_clone::<khora_core::agent::gorna::NegotiationResponse>();
    let _ = type_name::<khora_core::agent::gorna::ResourceBudget>();
    let _ = resource_budget_fields as fn(&khora_core::agent::gorna::ResourceBudget);
    is_debug::<khora_core::agent::gorna::ResourceBudget>();
    is_clone::<khora_core::agent::gorna::ResourceBudget>();
    let _ = type_name::<khora_core::agent::gorna::ResourceConstraints>();
    let _ = resource_constraints_fields as fn(&khora_core::agent::gorna::ResourceConstraints);
    is_debug::<khora_core::agent::gorna::ResourceConstraints>();
    is_clone::<khora_core::agent::gorna::ResourceConstraints>();
    is_default::<khora_core::agent::gorna::ResourceConstraints>();
    let _ = type_name::<khora_core::agent::gorna::StrategyId>();
    let _ = strategy_id_variants as fn(&khora_core::agent::gorna::StrategyId);
    is_debug::<khora_core::agent::gorna::StrategyId>();
    is_clone::<khora_core::agent::gorna::StrategyId>();
    is_copy::<khora_core::agent::gorna::StrategyId>();
    is_partial_eq::<khora_core::agent::gorna::StrategyId>();
    is_eq::<khora_core::agent::gorna::StrategyId>();
    is_hash::<khora_core::agent::gorna::StrategyId>();
    is_serialize::<khora_core::agent::gorna::StrategyId>();
    is_deserialize_owned::<khora_core::agent::gorna::StrategyId>();
    let _ = type_name::<khora_core::agent::gorna::StrategyOption>();
    let _ = strategy_option_fields as fn(&khora_core::agent::gorna::StrategyOption);
    is_debug::<khora_core::agent::gorna::StrategyOption>();
    is_clone::<khora_core::agent::gorna::StrategyOption>();
    let _ = type_name::<khora_core::agent::gorna::TickDecisions>();
    let _ = type_name::<khora_core::agent::gorna::TimingAdjustment>();
    let _ = timing_adjustment_fields as fn(&khora_core::agent::gorna::TimingAdjustment);
    is_debug::<khora_core::agent::gorna::TimingAdjustment>();
    is_clone::<khora_core::agent::gorna::TimingAdjustment>();
    let _ = khora_core::agent::gorna::measured_frame_time_ms;
}

#[test]
fn module_ecs_paths_still_resolve() {
    let _ = type_name::<khora_core::ecs::entity::EntityId>();
    let _ = entity_id_fields as fn(&khora_core::ecs::entity::EntityId);
    is_debug::<khora_core::ecs::entity::EntityId>();
    is_clone::<khora_core::ecs::entity::EntityId>();
    is_copy::<khora_core::ecs::entity::EntityId>();
    is_partial_eq::<khora_core::ecs::entity::EntityId>();
    is_eq::<khora_core::ecs::entity::EntityId>();
    is_hash::<khora_core::ecs::entity::EntityId>();
    is_serialize::<khora_core::ecs::entity::EntityId>();
    is_deserialize_owned::<khora_core::ecs::entity::EntityId>();
    is_encode::<khora_core::ecs::entity::EntityId>();
    is_decode::<khora_core::ecs::entity::EntityId>();
    is_borrow_decode::<khora_core::ecs::entity::EntityId>();
}

#[test]
fn module_event_paths_still_resolve() {
    let _ = type_name::<khora_core::event::Channel<khora_core::physics::Collision>>();
    let _ = <khora_core::event::Channel<khora_core::physics::Collision>>::bounded;
    let _ = <khora_core::event::Channel<khora_core::physics::Collision>>::send;
    let _ = <khora_core::event::Channel<khora_core::physics::Collision>>::read;
    let _ = <khora_core::event::Channel<khora_core::physics::Collision>>::read_for;
    let _ = <khora_core::event::Channel<khora_core::physics::Collision>>::drain;
    let _ = <khora_core::event::Channel<khora_core::physics::Collision>>::dropped;
    let _ = <khora_core::event::Channel<khora_core::physics::Collision>>::len;
    let _ = <khora_core::event::Channel<khora_core::physics::Collision>>::is_empty;
    is_clone::<khora_core::event::Channel<khora_core::physics::Collision>>();
    is_debug::<khora_core::event::Channel<khora_core::physics::Collision>>();
    let _ = type_name::<khora_core::event::Cursor>();
    is_debug::<khora_core::event::Cursor>();
    is_clone::<khora_core::event::Cursor>();
    is_copy::<khora_core::event::Cursor>();
    is_default::<khora_core::event::Cursor>();
    is_partial_eq::<khora_core::event::Cursor>();
    is_eq::<khora_core::event::Cursor>();
    // trait `khora_core::event::Supersedes`: see `supersedes_trait_items`
    let _ = type_name::<khora_core::event::WhenFull>();
    let _ = when_full_variants as fn(&khora_core::event::WhenFull);
    is_debug::<khora_core::event::WhenFull>();
    is_clone::<khora_core::event::WhenFull>();
    is_copy::<khora_core::event::WhenFull>();
    is_partial_eq::<khora_core::event::WhenFull>();
    is_eq::<khora_core::event::WhenFull>();
}

#[test]
fn module_graph_paths_still_resolve() {
    let _ = type_name::<khora_core::graph::CycleError>();
    is_debug::<khora_core::graph::CycleError>();
    is_clone::<khora_core::graph::CycleError>();
    is_partial_eq::<khora_core::graph::CycleError>();
    is_eq::<khora_core::graph::CycleError>();
    #[allow(clippy::type_complexity)]
    let _: fn(Vec<u32>, Vec<(u32, u32)>) -> Result<Vec<u32>, khora_core::graph::CycleError> =
        khora_core::graph::topological_sort;
}

#[test]
fn module_interpolation_paths_still_resolve() {
    let _ = type_name::<khora_core::interpolation::SharedTransformInterpolation>();
    let _ = type_name::<khora_core::interpolation::TransformInterpolation>();
    let _ = khora_core::interpolation::TransformInterpolation::new;
    let _ = khora_core::interpolation::TransformInterpolation::previous;
    let _ = khora_core::interpolation::TransformInterpolation::record;
    let _ = khora_core::interpolation::TransformInterpolation::retain_live;
    let _ = khora_core::interpolation::TransformInterpolation::len;
    let _ = khora_core::interpolation::TransformInterpolation::is_empty;
    is_debug::<khora_core::interpolation::TransformInterpolation>();
    is_default::<khora_core::interpolation::TransformInterpolation>();
}

#[test]
fn module_lane_paths_still_resolve() {
    // trait `khora_core::lane::Lane`: see `lane_trait_items`
    let _ = type_name::<khora_core::lane::LaneContext>();
    let _ = khora_core::lane::LaneContext::new;
    let _ = khora_core::lane::LaneContext::insert::<u32>;
    let _ = khora_core::lane::LaneContext::get::<u32>;
    let _ = khora_core::lane::LaneContext::get_mut::<u32>;
    let _ = khora_core::lane::LaneContext::contains::<u32>;
    let _ = khora_core::lane::LaneContext::remove::<u32>;
    let _ = khora_core::lane::LaneContext::insert_slot::<u32>;
    let _ = khora_core::lane::LaneContext::insert_slot_as::<u8, u32>;
    let _ = khora_core::lane::LaneContext::slot::<u32>;
    let _ = khora_core::lane::LaneContext::slot_as::<u8, u32>;
    let _ = khora_core::lane::LaneContext::insert_ref::<u32>;
    let _ = khora_core::lane::LaneContext::get_ref::<u32>;
    is_default::<khora_core::lane::LaneContext>();
    is_debug::<khora_core::lane::LaneContext>();
    let _ = type_name::<khora_core::lane::LaneError>();
    let _ = lane_error_variants as fn(&khora_core::lane::LaneError);
    let _ = khora_core::lane::LaneError::missing;
    let _ = khora_core::lane::LaneError::lock_poisoned;
    let _: fn(&'static str, String) -> khora_core::lane::LaneError =
        khora_core::lane::LaneError::missing_resource;
    let _: fn(&'static str, &'static str) -> khora_core::lane::LaneError =
        khora_core::lane::LaneError::missing_asset;
    is_debug::<khora_core::lane::LaneError>();
    is_display::<khora_core::lane::LaneError>();
    is_error::<khora_core::lane::LaneError>();
    let _ = type_name::<khora_core::lane::LaneKind>();
    let _ = lane_kind_variants as fn(&khora_core::lane::LaneKind);
    is_debug::<khora_core::lane::LaneKind>();
    is_clone::<khora_core::lane::LaneKind>();
    is_copy::<khora_core::lane::LaneKind>();
    is_partial_eq::<khora_core::lane::LaneKind>();
    is_eq::<khora_core::lane::LaneKind>();
    is_hash::<khora_core::lane::LaneKind>();
    is_display::<khora_core::lane::LaneKind>();
    let _ = type_name::<khora_core::lane::LaneRegistry>();
    let _ = khora_core::lane::LaneRegistry::new;
    let _ = khora_core::lane::LaneRegistry::register;
    let _ = khora_core::lane::LaneRegistry::get;
    let _ = khora_core::lane::LaneRegistry::find_by_kind;
    let _ = khora_core::lane::LaneRegistry::all;
    let _ = khora_core::lane::LaneRegistry::len;
    let _ = khora_core::lane::LaneRegistry::is_empty;
    is_default::<khora_core::lane::LaneRegistry>();
    let _ = type_name::<khora_core::lane::bus::LaneBus>();
    let _ = type_name::<khora_core::lane::LaneBus>();
    same_type(
        PhantomData::<khora_core::lane::LaneBus>,
        PhantomData::<khora_core::lane::bus::LaneBus>,
    );
    let _ = khora_core::lane::bus::LaneBus::new;
    let _ = khora_core::lane::bus::LaneBus::publish::<u32>;
    let _ = khora_core::lane::bus::LaneBus::get::<u32>;
    let _ = khora_core::lane::bus::LaneBus::contains::<u32>;
    let _ = khora_core::lane::bus::LaneBus::len;
    let _ = khora_core::lane::bus::LaneBus::is_empty;
    let _ = khora_core::lane::bus::LaneBus::clear;
    is_default::<khora_core::lane::bus::LaneBus>();
    is_debug::<khora_core::lane::bus::LaneBus>();
    let _ = type_name::<khora_core::lane::context_keys::ClearColor>();
    let _ = type_name::<khora_core::lane::ClearColor>();
    same_type(
        PhantomData::<khora_core::lane::ClearColor>,
        PhantomData::<khora_core::lane::context_keys::ClearColor>,
    );
    let _ = clear_color_fields as fn(&khora_core::lane::context_keys::ClearColor);
    is_debug::<khora_core::lane::context_keys::ClearColor>();
    is_clone::<khora_core::lane::context_keys::ClearColor>();
    is_copy::<khora_core::lane::context_keys::ClearColor>();
    let _ = type_name::<khora_core::lane::context_keys::ColorTarget>();
    let _ = type_name::<khora_core::lane::ColorTarget>();
    same_type(
        PhantomData::<khora_core::lane::ColorTarget>,
        PhantomData::<khora_core::lane::context_keys::ColorTarget>,
    );
    let _ = color_target_fields as fn(&khora_core::lane::context_keys::ColorTarget);
    is_debug::<khora_core::lane::context_keys::ColorTarget>();
    is_clone::<khora_core::lane::context_keys::ColorTarget>();
    is_copy::<khora_core::lane::context_keys::ColorTarget>();
    let _ = type_name::<khora_core::lane::context_keys::DepthTarget>();
    let _ = type_name::<khora_core::lane::DepthTarget>();
    same_type(
        PhantomData::<khora_core::lane::DepthTarget>,
        PhantomData::<khora_core::lane::context_keys::DepthTarget>,
    );
    let _ = depth_target_fields as fn(&khora_core::lane::context_keys::DepthTarget);
    is_debug::<khora_core::lane::context_keys::DepthTarget>();
    is_clone::<khora_core::lane::context_keys::DepthTarget>();
    is_copy::<khora_core::lane::context_keys::DepthTarget>();
    let _ = type_name::<khora_core::lane::context_keys::PhysicsDeltaTime>();
    let _ = type_name::<khora_core::lane::PhysicsDeltaTime>();
    same_type(
        PhantomData::<khora_core::lane::PhysicsDeltaTime>,
        PhantomData::<khora_core::lane::context_keys::PhysicsDeltaTime>,
    );
    let _ = physics_delta_time_fields as fn(&khora_core::lane::context_keys::PhysicsDeltaTime);
    is_debug::<khora_core::lane::context_keys::PhysicsDeltaTime>();
    is_clone::<khora_core::lane::context_keys::PhysicsDeltaTime>();
    is_copy::<khora_core::lane::context_keys::PhysicsDeltaTime>();
    let _ = type_name::<khora_core::lane::context_keys::ShadowAtlasCubeView>();
    let _ = type_name::<khora_core::lane::ShadowAtlasCubeView>();
    same_type(
        PhantomData::<khora_core::lane::ShadowAtlasCubeView>,
        PhantomData::<khora_core::lane::context_keys::ShadowAtlasCubeView>,
    );
    let _ =
        shadow_atlas_cube_view_fields as fn(&khora_core::lane::context_keys::ShadowAtlasCubeView);
    is_debug::<khora_core::lane::context_keys::ShadowAtlasCubeView>();
    is_clone::<khora_core::lane::context_keys::ShadowAtlasCubeView>();
    is_copy::<khora_core::lane::context_keys::ShadowAtlasCubeView>();
    let _ = type_name::<khora_core::lane::context_keys::ShadowAtlasView>();
    let _ = type_name::<khora_core::lane::ShadowAtlasView>();
    same_type(
        PhantomData::<khora_core::lane::ShadowAtlasView>,
        PhantomData::<khora_core::lane::context_keys::ShadowAtlasView>,
    );
    let _ = shadow_atlas_view_fields as fn(&khora_core::lane::context_keys::ShadowAtlasView);
    is_debug::<khora_core::lane::context_keys::ShadowAtlasView>();
    is_clone::<khora_core::lane::context_keys::ShadowAtlasView>();
    is_copy::<khora_core::lane::context_keys::ShadowAtlasView>();
    let _ = type_name::<khora_core::lane::context_keys::ShadowComparisonSampler>();
    let _ = type_name::<khora_core::lane::ShadowComparisonSampler>();
    same_type(
        PhantomData::<khora_core::lane::ShadowComparisonSampler>,
        PhantomData::<khora_core::lane::context_keys::ShadowComparisonSampler>,
    );
    let _ = shadow_comparison_sampler_fields
        as fn(&khora_core::lane::context_keys::ShadowComparisonSampler);
    is_debug::<khora_core::lane::context_keys::ShadowComparisonSampler>();
    is_clone::<khora_core::lane::context_keys::ShadowComparisonSampler>();
    is_copy::<khora_core::lane::context_keys::ShadowComparisonSampler>();
    let _ = type_name::<khora_core::lane::deck::OutputDeck>();
    let _ = type_name::<khora_core::lane::OutputDeck>();
    same_type(
        PhantomData::<khora_core::lane::OutputDeck>,
        PhantomData::<khora_core::lane::deck::OutputDeck>,
    );
    let _ = khora_core::lane::deck::OutputDeck::new;
    let _ = khora_core::lane::deck::OutputDeck::slot::<u32>;
    let _ = khora_core::lane::deck::OutputDeck::take::<u32>;
    let _ = khora_core::lane::deck::OutputDeck::contains::<u32>;
    let _ = khora_core::lane::deck::OutputDeck::len;
    let _ = khora_core::lane::deck::OutputDeck::is_empty;
    let _ = khora_core::lane::deck::OutputDeck::clear;
    let _ = khora_core::lane::deck::OutputDeck::merge_from;
    is_default::<khora_core::lane::deck::OutputDeck>();
    is_debug::<khora_core::lane::deck::OutputDeck>();
    let _ = khora_core::lane::lock::mutex_lock::<u32>;
    let _ = khora_core::lane::mutex_lock::<u32>;
    same_item(
        &khora_core::lane::mutex_lock::<u32>,
        &khora_core::lane::lock::mutex_lock::<u32>,
    );
    let _ = khora_core::lane::lock::mutex_lock_render::<u32>;
    let _ = khora_core::lane::mutex_lock_render::<u32>;
    same_item(
        &khora_core::lane::mutex_lock_render::<u32>,
        &khora_core::lane::lock::mutex_lock_render::<u32>,
    );
    let _ = khora_core::lane::lock::read_lock::<u32>;
    let _ = khora_core::lane::read_lock::<u32>;
    same_item(
        &khora_core::lane::read_lock::<u32>,
        &khora_core::lane::lock::read_lock::<u32>,
    );
    let _ = khora_core::lane::lock::read_lock_render::<u32>;
    let _ = khora_core::lane::read_lock_render::<u32>;
    same_item(
        &khora_core::lane::read_lock_render::<u32>,
        &khora_core::lane::lock::read_lock_render::<u32>,
    );
    let _ = khora_core::lane::lock::write_lock::<u32>;
    let _ = khora_core::lane::write_lock::<u32>;
    same_item(
        &khora_core::lane::write_lock::<u32>,
        &khora_core::lane::lock::write_lock::<u32>,
    );
    let _ = khora_core::lane::lock::write_lock_render::<u32>;
    let _ = khora_core::lane::write_lock_render::<u32>;
    same_item(
        &khora_core::lane::write_lock_render::<u32>,
        &khora_core::lane::lock::write_lock_render::<u32>,
    );
    let _ = type_name::<khora_core::lane::slot::SlotGuard<'static, u32>>();
    let _ = type_name::<khora_core::lane::SlotGuard<'static, u32>>();
    same_type(
        PhantomData::<khora_core::lane::SlotGuard<'static, u32>>,
        PhantomData::<khora_core::lane::slot::SlotGuard<'static, u32>>,
    );
    is_deref::<khora_core::lane::slot::SlotGuard<'static, u32>>();
    is_deref_mut::<khora_core::lane::slot::SlotGuard<'static, u32>>();
}

#[test]
fn module_memory_paths_still_resolve() {
    let _ = &khora_core::memory::BYTES_ALLOCATED_LIFETIME;
    let _ = &khora_core::memory::BYTES_DEALLOCATED_LIFETIME;
    let _ = &khora_core::memory::CURRENTLY_ALLOCATED_BYTES;
    let _ = type_name::<khora_core::memory::ExtendedMemoryStats>();
    let _ = extended_memory_stats_fields as fn(&khora_core::memory::ExtendedMemoryStats);
    let _ = khora_core::memory::ExtendedMemoryStats::calculate_derived_metrics;
    is_debug::<khora_core::memory::ExtendedMemoryStats>();
    is_clone::<khora_core::memory::ExtendedMemoryStats>();
    is_copy::<khora_core::memory::ExtendedMemoryStats>();
    is_default::<khora_core::memory::ExtendedMemoryStats>();
    let _ = &khora_core::memory::LARGE_ALLOCATIONS;
    let _ = &khora_core::memory::LARGE_ALLOCATION_BYTES;
    let _ = &khora_core::memory::PEAK_ALLOCATED_BYTES;
    let _ = &khora_core::memory::SMALL_ALLOCATIONS;
    let _ = &khora_core::memory::SMALL_ALLOCATION_BYTES;
    let _ = &khora_core::memory::TOTAL_ALLOCATIONS;
    let _ = &khora_core::memory::TOTAL_DEALLOCATIONS;
    let _ = &khora_core::memory::TOTAL_REALLOCATIONS;
    let _ = khora_core::memory::get_currently_allocated_bytes;
    let _ = khora_core::memory::get_extended_memory_stats;
}

#[test]
fn module_physics_paths_still_resolve() {
    let _ = type_name::<khora_core::physics::BodyType>();
    let _ = body_type_variants as fn(&khora_core::physics::BodyType);
    is_debug::<khora_core::physics::BodyType>();
    is_clone::<khora_core::physics::BodyType>();
    is_copy::<khora_core::physics::BodyType>();
    is_partial_eq::<khora_core::physics::BodyType>();
    is_eq::<khora_core::physics::BodyType>();
    is_serialize::<khora_core::physics::BodyType>();
    is_deserialize_owned::<khora_core::physics::BodyType>();
    is_encode::<khora_core::physics::BodyType>();
    is_decode::<khora_core::physics::BodyType>();
    is_borrow_decode::<khora_core::physics::BodyType>();
    let _ = khora_core::physics::COLLISION_BACKLOG;
    let _ = type_name::<khora_core::physics::CharacterControllerOptions>();
    let _ =
        character_controller_options_fields as fn(&khora_core::physics::CharacterControllerOptions);
    is_debug::<khora_core::physics::CharacterControllerOptions>();
    is_clone::<khora_core::physics::CharacterControllerOptions>();
    is_copy::<khora_core::physics::CharacterControllerOptions>();
    is_serialize::<khora_core::physics::CharacterControllerOptions>();
    is_deserialize_owned::<khora_core::physics::CharacterControllerOptions>();
    is_encode::<khora_core::physics::CharacterControllerOptions>();
    is_decode::<khora_core::physics::CharacterControllerOptions>();
    is_borrow_decode::<khora_core::physics::CharacterControllerOptions>();
    let _ = type_name::<khora_core::physics::ColliderDesc>();
    let _ = collider_desc_fields as fn(&khora_core::physics::ColliderDesc);
    is_debug::<khora_core::physics::ColliderDesc>();
    is_clone::<khora_core::physics::ColliderDesc>();
    is_serialize::<khora_core::physics::ColliderDesc>();
    is_deserialize_owned::<khora_core::physics::ColliderDesc>();
    let _ = type_name::<khora_core::physics::ColliderHandle>();
    let _ = collider_handle_fields as fn(&khora_core::physics::ColliderHandle);
    let _ = khora_core::physics::ColliderHandle::slot;
    is_debug::<khora_core::physics::ColliderHandle>();
    is_clone::<khora_core::physics::ColliderHandle>();
    is_copy::<khora_core::physics::ColliderHandle>();
    is_partial_eq::<khora_core::physics::ColliderHandle>();
    is_eq::<khora_core::physics::ColliderHandle>();
    is_hash::<khora_core::physics::ColliderHandle>();
    is_serialize::<khora_core::physics::ColliderHandle>();
    is_deserialize_owned::<khora_core::physics::ColliderHandle>();
    is_encode::<khora_core::physics::ColliderHandle>();
    is_decode::<khora_core::physics::ColliderHandle>();
    is_borrow_decode::<khora_core::physics::ColliderHandle>();
    let _ = type_name::<khora_core::physics::ColliderShape>();
    let _ = collider_shape_variants as fn(&khora_core::physics::ColliderShape);
    let _ = khora_core::physics::ColliderShape::compute_aabb;
    is_debug::<khora_core::physics::ColliderShape>();
    is_clone::<khora_core::physics::ColliderShape>();
    is_serialize::<khora_core::physics::ColliderShape>();
    is_deserialize_owned::<khora_core::physics::ColliderShape>();
    is_encode::<khora_core::physics::ColliderShape>();
    is_decode::<khora_core::physics::ColliderShape>();
    is_borrow_decode::<khora_core::physics::ColliderShape>();
    let _ = type_name::<khora_core::physics::Collision>();
    let _ = collision_fields as fn(&khora_core::physics::Collision);
    is_debug::<khora_core::physics::Collision>();
    is_clone::<khora_core::physics::Collision>();
    is_copy::<khora_core::physics::Collision>();
    is_partial_eq::<khora_core::physics::Collision>();
    is_eq::<khora_core::physics::Collision>();
    is_supersedes::<khora_core::physics::Collision>();
    let _ = type_name::<khora_core::physics::CollisionEvent>();
    let _ = collision_event_variants as fn(&khora_core::physics::CollisionEvent);
    is_debug::<khora_core::physics::CollisionEvent>();
    is_clone::<khora_core::physics::CollisionEvent>();
    is_copy::<khora_core::physics::CollisionEvent>();
    is_serialize::<khora_core::physics::CollisionEvent>();
    is_deserialize_owned::<khora_core::physics::CollisionEvent>();
    is_encode::<khora_core::physics::CollisionEvent>();
    is_decode::<khora_core::physics::CollisionEvent>();
    is_borrow_decode::<khora_core::physics::CollisionEvent>();
    let _ = type_name::<khora_core::physics::CollisionKind>();
    let _ = collision_kind_variants as fn(&khora_core::physics::CollisionKind);
    is_debug::<khora_core::physics::CollisionKind>();
    is_clone::<khora_core::physics::CollisionKind>();
    is_copy::<khora_core::physics::CollisionKind>();
    is_partial_eq::<khora_core::physics::CollisionKind>();
    is_eq::<khora_core::physics::CollisionKind>();
    let _ = type_name::<khora_core::physics::ContactBatch>();
    let _ = contact_batch_fields as fn(&khora_core::physics::ContactBatch);
    is_debug::<khora_core::physics::ContactBatch>();
    is_clone::<khora_core::physics::ContactBatch>();
    is_default::<khora_core::physics::ContactBatch>();
    let _ = type_name::<khora_core::physics::DebugLine>();
    let _ = debug_line_fields as fn(&khora_core::physics::DebugLine);
    is_debug::<khora_core::physics::DebugLine>();
    is_clone::<khora_core::physics::DebugLine>();
    is_copy::<khora_core::physics::DebugLine>();
    // trait `khora_core::physics::PhysicsProvider`: see `physics_provider_trait_items`
    let _ = type_name::<khora_core::physics::RaycastHit>();
    let _ = raycast_hit_fields as fn(&khora_core::physics::RaycastHit);
    is_debug::<khora_core::physics::RaycastHit>();
    is_clone::<khora_core::physics::RaycastHit>();
    is_copy::<khora_core::physics::RaycastHit>();
    is_serialize::<khora_core::physics::RaycastHit>();
    is_deserialize_owned::<khora_core::physics::RaycastHit>();
    is_encode::<khora_core::physics::RaycastHit>();
    is_decode::<khora_core::physics::RaycastHit>();
    is_borrow_decode::<khora_core::physics::RaycastHit>();
    let _ = type_name::<khora_core::physics::RigidBodyDesc>();
    let _ = rigid_body_desc_fields as fn(&khora_core::physics::RigidBodyDesc);
    is_debug::<khora_core::physics::RigidBodyDesc>();
    is_clone::<khora_core::physics::RigidBodyDesc>();
    is_serialize::<khora_core::physics::RigidBodyDesc>();
    is_deserialize_owned::<khora_core::physics::RigidBodyDesc>();
    let _ = type_name::<khora_core::physics::RigidBodyHandle>();
    let _ = rigid_body_handle_fields as fn(&khora_core::physics::RigidBodyHandle);
    let _ = khora_core::physics::RigidBodyHandle::slot;
    is_debug::<khora_core::physics::RigidBodyHandle>();
    is_clone::<khora_core::physics::RigidBodyHandle>();
    is_copy::<khora_core::physics::RigidBodyHandle>();
    is_partial_eq::<khora_core::physics::RigidBodyHandle>();
    is_eq::<khora_core::physics::RigidBodyHandle>();
    is_hash::<khora_core::physics::RigidBodyHandle>();
    is_serialize::<khora_core::physics::RigidBodyHandle>();
    is_deserialize_owned::<khora_core::physics::RigidBodyHandle>();
    is_encode::<khora_core::physics::RigidBodyHandle>();
    is_decode::<khora_core::physics::RigidBodyHandle>();
    is_borrow_decode::<khora_core::physics::RigidBodyHandle>();
    let _ = type_name::<khora_core::physics::SlotId>();
    let _ = physics_slot_fields as fn(&khora_core::physics::SlotId);
    let _ = khora_core::physics::SlotId::pack;
    let _ = khora_core::physics::SlotId::unpack;
    is_debug::<khora_core::physics::SlotId>();
    is_clone::<khora_core::physics::SlotId>();
    is_copy::<khora_core::physics::SlotId>();
    is_partial_eq::<khora_core::physics::SlotId>();
    is_eq::<khora_core::physics::SlotId>();
    let _ = khora_core::physics::collision_channel;
}

#[test]
fn module_platform_paths_still_resolve() {
    let _ = type_name::<khora_core::platform::BatteryLevel>();
    let _ = battery_level_variants as fn(&khora_core::platform::BatteryLevel);
    is_debug::<khora_core::platform::BatteryLevel>();
    is_clone::<khora_core::platform::BatteryLevel>();
    is_copy::<khora_core::platform::BatteryLevel>();
    is_partial_eq::<khora_core::platform::BatteryLevel>();
    is_eq::<khora_core::platform::BatteryLevel>();
    is_default::<khora_core::platform::BatteryLevel>();
    // trait `khora_core::platform::HardwareMonitor`: see `hardware_monitor_trait_items`
    let _ = type_name::<khora_core::platform::ThermalStatus>();
    let _ = thermal_status_variants as fn(&khora_core::platform::ThermalStatus);
    is_debug::<khora_core::platform::ThermalStatus>();
    is_clone::<khora_core::platform::ThermalStatus>();
    is_copy::<khora_core::platform::ThermalStatus>();
    is_partial_eq::<khora_core::platform::ThermalStatus>();
    is_eq::<khora_core::platform::ThermalStatus>();
    is_default::<khora_core::platform::ThermalStatus>();
    let _ = khora_core::platform::input::INPUT_BACKLOG;
    let _ = khora_core::platform::INPUT_BACKLOG;
    let _ = type_name::<khora_core::platform::input::InputEvent>();
    let _ = type_name::<khora_core::platform::InputEvent>();
    same_type(
        PhantomData::<khora_core::platform::InputEvent>,
        PhantomData::<khora_core::platform::input::InputEvent>,
    );
    let _ = input_event_variants as fn(&khora_core::platform::input::InputEvent);
    is_debug::<khora_core::platform::input::InputEvent>();
    is_clone::<khora_core::platform::input::InputEvent>();
    is_partial_eq::<khora_core::platform::input::InputEvent>();
    is_supersedes::<khora_core::platform::input::InputEvent>();
    let _ = type_name::<khora_core::platform::input::KeyCode>();
    let _ = type_name::<khora_core::platform::KeyCode>();
    same_type(
        PhantomData::<khora_core::platform::KeyCode>,
        PhantomData::<khora_core::platform::input::KeyCode>,
    );
    let _ = key_code_variants as fn(&khora_core::platform::input::KeyCode);
    is_debug::<khora_core::platform::input::KeyCode>();
    is_clone::<khora_core::platform::input::KeyCode>();
    is_copy::<khora_core::platform::input::KeyCode>();
    is_partial_eq::<khora_core::platform::input::KeyCode>();
    is_eq::<khora_core::platform::input::KeyCode>();
    is_hash::<khora_core::platform::input::KeyCode>();
    is_partial_ord::<khora_core::platform::input::KeyCode>();
    is_ord::<khora_core::platform::input::KeyCode>();
    let _ = type_name::<khora_core::platform::input::MouseButton>();
    let _ = type_name::<khora_core::platform::MouseButton>();
    same_type(
        PhantomData::<khora_core::platform::MouseButton>,
        PhantomData::<khora_core::platform::input::MouseButton>,
    );
    let _ = mouse_button_variants as fn(&khora_core::platform::input::MouseButton);
    is_debug::<khora_core::platform::input::MouseButton>();
    is_clone::<khora_core::platform::input::MouseButton>();
    is_copy::<khora_core::platform::input::MouseButton>();
    is_partial_eq::<khora_core::platform::input::MouseButton>();
    is_eq::<khora_core::platform::input::MouseButton>();
    is_hash::<khora_core::platform::input::MouseButton>();
    let _ = khora_core::platform::input::input_channel;
    let _ = khora_core::platform::input_channel;
    same_item(
        &khora_core::platform::input_channel,
        &khora_core::platform::input::input_channel,
    );
    let _ = type_name::<khora_core::platform::input_map::Action>();
    let _ = type_name::<khora_core::platform::Action>();
    same_type(
        PhantomData::<khora_core::platform::Action>,
        PhantomData::<khora_core::platform::input_map::Action>,
    );
    let _ = action_fields as fn(&khora_core::platform::input_map::Action);
    let _ = khora_core::platform::input_map::Action::as_str;
    is_debug::<khora_core::platform::input_map::Action>();
    is_clone::<khora_core::platform::input_map::Action>();
    is_partial_eq::<khora_core::platform::input_map::Action>();
    is_eq::<khora_core::platform::input_map::Action>();
    is_hash::<khora_core::platform::input_map::Action>();
    is_from_str::<khora_core::platform::input_map::Action>();
    is_from_string::<khora_core::platform::input_map::Action>();
    let _ = type_name::<khora_core::platform::input_map::InputBinding>();
    let _ = type_name::<khora_core::platform::InputBinding>();
    same_type(
        PhantomData::<khora_core::platform::InputBinding>,
        PhantomData::<khora_core::platform::input_map::InputBinding>,
    );
    let _ = input_binding_variants as fn(&khora_core::platform::input_map::InputBinding);
    is_debug::<khora_core::platform::input_map::InputBinding>();
    is_clone::<khora_core::platform::input_map::InputBinding>();
    is_copy::<khora_core::platform::input_map::InputBinding>();
    is_partial_eq::<khora_core::platform::input_map::InputBinding>();
    is_eq::<khora_core::platform::input_map::InputBinding>();
    is_hash::<khora_core::platform::input_map::InputBinding>();
    let _ = type_name::<khora_core::platform::input_map::InputMap>();
    let _ = type_name::<khora_core::platform::InputMap>();
    same_type(
        PhantomData::<khora_core::platform::InputMap>,
        PhantomData::<khora_core::platform::input_map::InputMap>,
    );
    let _ = khora_core::platform::input_map::InputMap::new;
    let _: fn(
        &mut khora_core::platform::input_map::InputMap,
        khora_core::platform::input_map::Action,
        khora_core::platform::input_map::InputBinding,
    ) = khora_core::platform::input_map::InputMap::bind;
    let _ = khora_core::platform::input_map::InputMap::unbind;
    let _ = khora_core::platform::input_map::InputMap::clear_action;
    let _ = khora_core::platform::input_map::InputMap::is_pressed;
    let _ = khora_core::platform::input_map::InputMap::just_pressed;
    let _ = khora_core::platform::input_map::InputMap::just_released;
    let _ = khora_core::platform::input_map::InputMap::snapshot;
    let _ = khora_core::platform::input_map::InputMap::update;
    is_debug::<khora_core::platform::input_map::InputMap>();
    is_default::<khora_core::platform::input_map::InputMap>();
    let _ = type_name::<khora_core::platform::input_map::InputSnapshot>();
    let _ = type_name::<khora_core::platform::InputSnapshot>();
    same_type(
        PhantomData::<khora_core::platform::InputSnapshot>,
        PhantomData::<khora_core::platform::input_map::InputSnapshot>,
    );
    let _ = input_snapshot_fields as fn(&khora_core::platform::input_map::InputSnapshot);
    let _ = khora_core::platform::input_map::InputSnapshot::is_pressed;
    let _ = khora_core::platform::input_map::InputSnapshot::just_pressed;
    is_debug::<khora_core::platform::input_map::InputSnapshot>();
    is_clone::<khora_core::platform::input_map::InputSnapshot>();
    is_default::<khora_core::platform::input_map::InputSnapshot>();
    is_partial_eq::<khora_core::platform::input_map::InputSnapshot>();
    is_eq::<khora_core::platform::input_map::InputSnapshot>();
    // trait `khora_core::platform::window::KhoraWindow`: see `khora_window_trait_items`
    // trait `khora_core::platform::KhoraWindow`: see `khora_window_trait_items`
    let _ = type_name::<khora_core::platform::window::KhoraWindowHandle>();
    let _ = type_name::<khora_core::platform::KhoraWindowHandle>();
    same_type(
        PhantomData::<khora_core::platform::KhoraWindowHandle>,
        PhantomData::<khora_core::platform::window::KhoraWindowHandle>,
    );
    // trait `khora_core::platform::window::WindowHandle`: see `window_handle_trait_items`
    // trait `khora_core::platform::WindowHandle`: see `window_handle_trait_items`
}

#[test]
fn module_runtime_paths_still_resolve() {
    let _ = type_name::<khora_core::runtime::BackendKind>();
    is_registry_kind::<khora_core::runtime::BackendKind>();
    let _ = type_name::<khora_core::runtime::ResourceKind>();
    is_registry_kind::<khora_core::runtime::ResourceKind>();
    let _ = type_name::<khora_core::runtime::Runtime>();
    let _ = type_name::<khora_core::Runtime>();
    same_type(
        PhantomData::<khora_core::Runtime>,
        PhantomData::<khora_core::runtime::Runtime>,
    );
    let _ = runtime_fields as fn(&khora_core::runtime::Runtime);
    let _ = khora_core::runtime::Runtime::new;
    is_default::<khora_core::runtime::Runtime>();
    is_debug::<khora_core::runtime::Runtime>();
    let _ = type_name::<khora_core::runtime::ServiceKind>();
    is_registry_kind::<khora_core::runtime::ServiceKind>();
    // trait `khora_core::runtime::typed_registry::RegistryKind`: see `registry_kind_trait_items`
    // trait `khora_core::runtime::RegistryKind`: see `registry_kind_trait_items`
    let _ = type_name::<
        khora_core::runtime::typed_registry::TypedRegistry<khora_core::runtime::BackendKind>,
    >();
    let _ = type_name::<khora_core::runtime::TypedRegistry<khora_core::runtime::BackendKind>>();
    same_type(
        PhantomData::<khora_core::runtime::TypedRegistry<khora_core::runtime::BackendKind>>,
        PhantomData::<
            khora_core::runtime::typed_registry::TypedRegistry<khora_core::runtime::BackendKind>,
        >,
    );
    let _ =
        <khora_core::runtime::typed_registry::TypedRegistry<khora_core::runtime::BackendKind>>::new;
    let _ = <khora_core::runtime::typed_registry::TypedRegistry<khora_core::runtime::BackendKind>>::with_parent;
    let _ =
        <khora_core::runtime::typed_registry::TypedRegistry<khora_core::runtime::BackendKind>>::insert::<
            u32,
        >;
    let _ =
        <khora_core::runtime::typed_registry::TypedRegistry<khora_core::runtime::BackendKind>>::get::<
            u32,
        >;
    let _ =
        <khora_core::runtime::typed_registry::TypedRegistry<khora_core::runtime::BackendKind>>::require::<
            u32,
        >;
    let _ =
        <khora_core::runtime::typed_registry::TypedRegistry<khora_core::runtime::BackendKind>>::contains::<
            u32,
        >;
    let _ =
        <khora_core::runtime::typed_registry::TypedRegistry<khora_core::runtime::BackendKind>>::len;
    let _ =
        <khora_core::runtime::typed_registry::TypedRegistry<khora_core::runtime::BackendKind>>::is_empty;
    is_default::<
        khora_core::runtime::typed_registry::TypedRegistry<khora_core::runtime::BackendKind>,
    >();
    is_debug::<khora_core::runtime::typed_registry::TypedRegistry<khora_core::runtime::BackendKind>>(
    );
}

#[test]
fn module_scene_paths_still_resolve() {
    let _ = khora_core::scene::HEADER_MAGIC_BYTES;
    let _ = type_name::<khora_core::scene::SceneFile>();
    let _ = scene_file_fields as fn(&khora_core::scene::SceneFile);
    let _ = khora_core::scene::SceneFile::from_bytes;
    let _ = khora_core::scene::SceneFile::to_bytes;
    is_debug::<khora_core::scene::SceneFile>();
    is_clone::<khora_core::scene::SceneFile>();
    is_partial_eq::<khora_core::scene::SceneFile>();
    is_eq::<khora_core::scene::SceneFile>();
    let _ = type_name::<khora_core::scene::SceneFileError>();
    let _ = scene_file_error_variants as fn(&khora_core::scene::SceneFileError);
    is_debug::<khora_core::scene::SceneFileError>();
    is_clone::<khora_core::scene::SceneFileError>();
    is_copy::<khora_core::scene::SceneFileError>();
    is_partial_eq::<khora_core::scene::SceneFileError>();
    is_eq::<khora_core::scene::SceneFileError>();
    let _ = type_name::<khora_core::scene::SceneHeader>();
    let _ = scene_header_fields as fn(&khora_core::scene::SceneHeader);
    let _ = khora_core::scene::SceneHeader::SIZE;
    let _ = khora_core::scene::SceneHeader::from_bytes;
    let _ = khora_core::scene::SceneHeader::to_bytes;
    is_debug::<khora_core::scene::SceneHeader>();
    is_clone::<khora_core::scene::SceneHeader>();
    is_partial_eq::<khora_core::scene::SceneHeader>();
    is_eq::<khora_core::scene::SceneHeader>();
    let _ = type_name::<khora_core::scene::SerializationGoal>();
    let _ = serialization_goal_variants as fn(&khora_core::scene::SerializationGoal);
    is_debug::<khora_core::scene::SerializationGoal>();
    is_clone::<khora_core::scene::SerializationGoal>();
    is_copy::<khora_core::scene::SerializationGoal>();
    is_partial_eq::<khora_core::scene::SerializationGoal>();
    is_eq::<khora_core::scene::SerializationGoal>();
    is_hash::<khora_core::scene::SerializationGoal>();
}

#[test]
fn module_script_paths_still_resolve() {
    let _ = type_name::<khora_core::script::buffer::CommandBuffer>();
    let _ = type_name::<khora_core::script::CommandBuffer>();
    same_type(
        PhantomData::<khora_core::script::CommandBuffer>,
        PhantomData::<khora_core::script::buffer::CommandBuffer>,
    );
    let _ = khora_core::script::buffer::CommandBuffer::new;
    let _ = khora_core::script::buffer::CommandBuffer::push;
    let _ = khora_core::script::buffer::CommandBuffer::len;
    let _ = khora_core::script::buffer::CommandBuffer::is_empty;
    let _ = khora_core::script::buffer::CommandBuffer::as_slice;
    let _ = khora_core::script::buffer::CommandBuffer::iter;
    let _ = khora_core::script::buffer::CommandBuffer::drain;
    let _ = khora_core::script::buffer::CommandBuffer::conflicts;
    let _ = khora_core::script::buffer::CommandBuffer::warn_on_conflicts;
    is_debug::<khora_core::script::buffer::CommandBuffer>();
    is_clone::<khora_core::script::buffer::CommandBuffer>();
    is_default::<khora_core::script::buffer::CommandBuffer>();
    is_partial_eq::<khora_core::script::buffer::CommandBuffer>();
    is_extend_world_command::<khora_core::script::buffer::CommandBuffer>();
    is_into_iterator::<&'static khora_core::script::buffer::CommandBuffer>();
    is_into_iterator::<khora_core::script::buffer::CommandBuffer>();
    let _ = type_name::<khora_core::script::buffer::Conflict>();
    let _ = type_name::<khora_core::script::Conflict>();
    same_type(
        PhantomData::<khora_core::script::Conflict>,
        PhantomData::<khora_core::script::buffer::Conflict>,
    );
    let _ = conflict_fields as fn(&khora_core::script::buffer::Conflict);
    let _ = khora_core::script::buffer::Conflict::message;
    is_debug::<khora_core::script::buffer::Conflict>();
    is_clone::<khora_core::script::buffer::Conflict>();
    is_partial_eq::<khora_core::script::buffer::Conflict>();
    is_eq::<khora_core::script::buffer::Conflict>();
    let _ = type_name::<khora_core::script::command::ComponentName>();
    let _ = type_name::<khora_core::script::ComponentName>();
    same_type(
        PhantomData::<khora_core::script::ComponentName>,
        PhantomData::<khora_core::script::command::ComponentName>,
    );
    let _: fn(String) -> khora_core::script::command::ComponentName =
        khora_core::script::command::ComponentName::new;
    let _ = khora_core::script::command::ComponentName::as_str;
    is_debug::<khora_core::script::command::ComponentName>();
    is_clone::<khora_core::script::command::ComponentName>();
    is_partial_eq::<khora_core::script::command::ComponentName>();
    is_eq::<khora_core::script::command::ComponentName>();
    is_hash::<khora_core::script::command::ComponentName>();
    is_partial_ord::<khora_core::script::command::ComponentName>();
    is_ord::<khora_core::script::command::ComponentName>();
    is_display::<khora_core::script::command::ComponentName>();
    is_from_str::<khora_core::script::command::ComponentName>();
    let _ = type_name::<khora_core::script::command::WorldCommand>();
    let _ = type_name::<khora_core::script::WorldCommand>();
    same_type(
        PhantomData::<khora_core::script::WorldCommand>,
        PhantomData::<khora_core::script::command::WorldCommand>,
    );
    let _ = world_command_variants as fn(&khora_core::script::command::WorldCommand);
    let _ = khora_core::script::command::WorldCommand::entity;
    let _ = khora_core::script::command::WorldCommand::write_target;
    is_debug::<khora_core::script::command::WorldCommand>();
    is_clone::<khora_core::script::command::WorldCommand>();
    is_partial_eq::<khora_core::script::command::WorldCommand>();
    let _ = type_name::<khora_core::script::command::WriteTarget>();
    let _ = type_name::<khora_core::script::WriteTarget>();
    same_type(
        PhantomData::<khora_core::script::WriteTarget>,
        PhantomData::<khora_core::script::command::WriteTarget>,
    );
    let _ = write_target_variants as fn(&khora_core::script::command::WriteTarget);
    is_debug::<khora_core::script::command::WriteTarget>();
    is_clone::<khora_core::script::command::WriteTarget>();
    is_partial_eq::<khora_core::script::command::WriteTarget>();
    is_eq::<khora_core::script::command::WriteTarget>();
    is_hash::<khora_core::script::command::WriteTarget>();
    is_partial_ord::<khora_core::script::command::WriteTarget>();
    is_ord::<khora_core::script::command::WriteTarget>();
    is_display::<khora_core::script::command::WriteTarget>();
    let _ = khora_core::script::event::ENGINE_EVENT_BACKLOG;
    let _ = khora_core::script::ENGINE_EVENT_BACKLOG;
    let _ = type_name::<khora_core::script::event::EventQueue>();
    let _ = type_name::<khora_core::script::EventQueue>();
    same_type(
        PhantomData::<khora_core::script::EventQueue>,
        PhantomData::<khora_core::script::event::EventQueue>,
    );
    let _ = khora_core::script::event::EventQueue::new;
    let _ = khora_core::script::event::EventQueue::push;
    let _ = khora_core::script::event::EventQueue::len;
    let _ = khora_core::script::event::EventQueue::is_empty;
    let _ = khora_core::script::event::EventQueue::as_slice;
    let _ = khora_core::script::event::EventQueue::drain;
    let _ = khora_core::script::event::EventQueue::for_entity;
    is_debug::<khora_core::script::event::EventQueue>();
    is_clone::<khora_core::script::event::EventQueue>();
    is_default::<khora_core::script::event::EventQueue>();
    is_partial_eq::<khora_core::script::event::EventQueue>();
    is_into_iterator::<&'static khora_core::script::event::EventQueue>();
    let _ = type_name::<khora_core::script::event::ScriptEvent>();
    let _ = type_name::<khora_core::script::ScriptEvent>();
    same_type(
        PhantomData::<khora_core::script::ScriptEvent>,
        PhantomData::<khora_core::script::event::ScriptEvent>,
    );
    let _ = script_event_fields as fn(&khora_core::script::event::ScriptEvent);
    let _: fn(khora_core::ecs::entity::EntityId, String) -> khora_core::script::event::ScriptEvent =
        khora_core::script::event::ScriptEvent::new;
    let _ = khora_core::script::event::ScriptEvent::with;
    is_debug::<khora_core::script::event::ScriptEvent>();
    is_clone::<khora_core::script::event::ScriptEvent>();
    is_partial_eq::<khora_core::script::event::ScriptEvent>();
    is_supersedes::<khora_core::script::event::ScriptEvent>();
    let _ = khora_core::script::event::engine_event_channel;
    let _ = khora_core::script::engine_event_channel;
    same_item(
        &khora_core::script::engine_event_channel,
        &khora_core::script::event::engine_event_channel,
    );
    let _ = type_name::<khora_core::script::snapshot::PendingSequence>();
    let _ = type_name::<khora_core::script::PendingSequence>();
    same_type(
        PhantomData::<khora_core::script::PendingSequence>,
        PhantomData::<khora_core::script::snapshot::PendingSequence>,
    );
    let _ = pending_sequence_fields as fn(&khora_core::script::snapshot::PendingSequence);
    is_debug::<khora_core::script::snapshot::PendingSequence>();
    is_clone::<khora_core::script::snapshot::PendingSequence>();
    is_partial_eq::<khora_core::script::snapshot::PendingSequence>();
    is_serialize::<khora_core::script::snapshot::PendingSequence>();
    is_deserialize_owned::<khora_core::script::snapshot::PendingSequence>();
    is_encode::<khora_core::script::snapshot::PendingSequence>();
    is_decode::<khora_core::script::snapshot::PendingSequence>();
    is_borrow_decode::<khora_core::script::snapshot::PendingSequence>();
    let _ = type_name::<khora_core::script::snapshot::ScriptSnapshot>();
    let _ = type_name::<khora_core::script::ScriptSnapshot>();
    same_type(
        PhantomData::<khora_core::script::ScriptSnapshot>,
        PhantomData::<khora_core::script::snapshot::ScriptSnapshot>,
    );
    let _ = script_snapshot_fields as fn(&khora_core::script::snapshot::ScriptSnapshot);
    let _ = khora_core::script::snapshot::ScriptSnapshot::is_empty;
    let _ = khora_core::script::snapshot::ScriptSnapshot::field;
    let _: fn(
        khora_core::script::snapshot::ScriptSnapshot,
        String,
        khora_core::script::value::ScriptValue,
    ) -> khora_core::script::snapshot::ScriptSnapshot =
        khora_core::script::snapshot::ScriptSnapshot::with_field;
    is_debug::<khora_core::script::snapshot::ScriptSnapshot>();
    is_clone::<khora_core::script::snapshot::ScriptSnapshot>();
    is_default::<khora_core::script::snapshot::ScriptSnapshot>();
    is_partial_eq::<khora_core::script::snapshot::ScriptSnapshot>();
    is_serialize::<khora_core::script::snapshot::ScriptSnapshot>();
    is_deserialize_owned::<khora_core::script::snapshot::ScriptSnapshot>();
    is_encode::<khora_core::script::snapshot::ScriptSnapshot>();
    is_decode::<khora_core::script::snapshot::ScriptSnapshot>();
    is_borrow_decode::<khora_core::script::snapshot::ScriptSnapshot>();
    let _ = type_name::<khora_core::script::snapshot::TimerRemaining>();
    let _ = type_name::<khora_core::script::TimerRemaining>();
    same_type(
        PhantomData::<khora_core::script::TimerRemaining>,
        PhantomData::<khora_core::script::snapshot::TimerRemaining>,
    );
    let _ = timer_remaining_fields as fn(&khora_core::script::snapshot::TimerRemaining);
    is_debug::<khora_core::script::snapshot::TimerRemaining>();
    is_clone::<khora_core::script::snapshot::TimerRemaining>();
    is_partial_eq::<khora_core::script::snapshot::TimerRemaining>();
    is_serialize::<khora_core::script::snapshot::TimerRemaining>();
    is_deserialize_owned::<khora_core::script::snapshot::TimerRemaining>();
    is_encode::<khora_core::script::snapshot::TimerRemaining>();
    is_decode::<khora_core::script::snapshot::TimerRemaining>();
    is_borrow_decode::<khora_core::script::snapshot::TimerRemaining>();
    let _ = type_name::<khora_core::script::value::ScriptValue>();
    let _ = type_name::<khora_core::script::ScriptValue>();
    same_type(
        PhantomData::<khora_core::script::ScriptValue>,
        PhantomData::<khora_core::script::value::ScriptValue>,
    );
    let _ = script_value_variants as fn(&khora_core::script::value::ScriptValue);
    let _ = khora_core::script::value::ScriptValue::type_name;
    let _ = khora_core::script::value::ScriptValue::as_bool;
    let _ = khora_core::script::value::ScriptValue::as_int;
    let _ = khora_core::script::value::ScriptValue::as_float;
    let _ = khora_core::script::value::ScriptValue::as_vec2;
    let _ = khora_core::script::value::ScriptValue::as_vec3;
    let _ = khora_core::script::value::ScriptValue::as_vec4;
    let _ = khora_core::script::value::ScriptValue::as_quat;
    let _ = khora_core::script::value::ScriptValue::as_color;
    let _ = khora_core::script::value::ScriptValue::as_entity;
    let _ = khora_core::script::value::ScriptValue::as_str;
    let _ = khora_core::script::value::ScriptValue::as_array;
    let _ = khora_core::script::value::ScriptValue::as_fields;
    is_debug::<khora_core::script::value::ScriptValue>();
    is_clone::<khora_core::script::value::ScriptValue>();
    is_partial_eq::<khora_core::script::value::ScriptValue>();
    is_serialize::<khora_core::script::value::ScriptValue>();
    is_deserialize_owned::<khora_core::script::value::ScriptValue>();
    is_encode::<khora_core::script::value::ScriptValue>();
    is_decode::<khora_core::script::value::ScriptValue>();
    is_borrow_decode::<khora_core::script::value::ScriptValue>();
    let _ = type_name::<khora_core::script::writeback::ScriptStateUpdate>();
    let _ = type_name::<khora_core::script::ScriptStateUpdate>();
    same_type(
        PhantomData::<khora_core::script::ScriptStateUpdate>,
        PhantomData::<khora_core::script::writeback::ScriptStateUpdate>,
    );
    let _ = script_state_update_fields as fn(&khora_core::script::writeback::ScriptStateUpdate);
    is_debug::<khora_core::script::writeback::ScriptStateUpdate>();
    is_clone::<khora_core::script::writeback::ScriptStateUpdate>();
    is_partial_eq::<khora_core::script::writeback::ScriptStateUpdate>();
    is_extend_script_state_update::<khora_core::script::writeback::ScriptStateWriteback>();
    let _ = type_name::<khora_core::script::writeback::ScriptStateWriteback>();
    let _ = type_name::<khora_core::script::ScriptStateWriteback>();
    same_type(
        PhantomData::<khora_core::script::ScriptStateWriteback>,
        PhantomData::<khora_core::script::writeback::ScriptStateWriteback>,
    );
    let _ = khora_core::script::writeback::ScriptStateWriteback::push;
    let _ = khora_core::script::writeback::ScriptStateWriteback::drain;
    is_debug::<khora_core::script::writeback::ScriptStateWriteback>();
    is_clone::<khora_core::script::writeback::ScriptStateWriteback>();
    is_default::<khora_core::script::writeback::ScriptStateWriteback>();
    is_partial_eq::<khora_core::script::writeback::ScriptStateWriteback>();
}

#[test]
fn module_telemetry_paths_still_resolve() {
    let _ = type_name::<khora_core::telemetry::event::TelemetryEvent>();
    let _ = type_name::<khora_core::telemetry::TelemetryEvent>();
    same_type(
        PhantomData::<khora_core::telemetry::TelemetryEvent>,
        PhantomData::<khora_core::telemetry::event::TelemetryEvent>,
    );
    let _ = telemetry_event_variants as fn(&khora_core::telemetry::event::TelemetryEvent);
    is_debug::<khora_core::telemetry::event::TelemetryEvent>();
    is_clone::<khora_core::telemetry::event::TelemetryEvent>();
    let _ = type_name::<khora_core::telemetry::metrics::Metric>();
    let _ = type_name::<khora_core::telemetry::Metric>();
    same_type(
        PhantomData::<khora_core::telemetry::Metric>,
        PhantomData::<khora_core::telemetry::metrics::Metric>,
    );
    let _ = metric_fields as fn(&khora_core::telemetry::metrics::Metric);
    let _: fn(
        khora_core::telemetry::metrics::MetricId,
        String,
        u64,
    ) -> khora_core::telemetry::metrics::Metric =
        khora_core::telemetry::metrics::Metric::new_counter;
    let _: fn(
        khora_core::telemetry::metrics::MetricId,
        String,
        String,
        f64,
    ) -> khora_core::telemetry::metrics::Metric = khora_core::telemetry::metrics::Metric::new_gauge;
    let _: fn(
        khora_core::telemetry::metrics::MetricId,
        String,
        String,
        Vec<f64>,
    ) -> khora_core::telemetry::metrics::Metric =
        khora_core::telemetry::metrics::Metric::new_histogram;
    is_debug::<khora_core::telemetry::metrics::Metric>();
    is_clone::<khora_core::telemetry::metrics::Metric>();
    let _ = type_name::<khora_core::telemetry::metrics::MetricId>();
    let _ = type_name::<khora_core::telemetry::MetricId>();
    same_type(
        PhantomData::<khora_core::telemetry::MetricId>,
        PhantomData::<khora_core::telemetry::metrics::MetricId>,
    );
    let _ = metric_id_fields as fn(&khora_core::telemetry::metrics::MetricId);
    let _: fn(String, String) -> khora_core::telemetry::metrics::MetricId =
        khora_core::telemetry::metrics::MetricId::new;
    let _: fn(
        khora_core::telemetry::metrics::MetricId,
        String,
        String,
    ) -> khora_core::telemetry::metrics::MetricId =
        khora_core::telemetry::metrics::MetricId::with_label;
    let _ = khora_core::telemetry::metrics::MetricId::to_string_formatted;
    is_debug::<khora_core::telemetry::metrics::MetricId>();
    is_clone::<khora_core::telemetry::metrics::MetricId>();
    is_partial_eq::<khora_core::telemetry::metrics::MetricId>();
    is_eq::<khora_core::telemetry::metrics::MetricId>();
    is_hash::<khora_core::telemetry::metrics::MetricId>();
    is_display::<khora_core::telemetry::metrics::MetricId>();
    let _ = type_name::<khora_core::telemetry::metrics::MetricMetadata>();
    let _ = metric_metadata_fields as fn(&khora_core::telemetry::metrics::MetricMetadata);
    let _: fn(
        khora_core::telemetry::metrics::MetricId,
        khora_core::telemetry::metrics::MetricType,
        String,
        String,
    ) -> khora_core::telemetry::metrics::MetricMetadata =
        khora_core::telemetry::metrics::MetricMetadata::new;
    let _ = khora_core::telemetry::metrics::MetricMetadata::update_timestamp;
    is_debug::<khora_core::telemetry::metrics::MetricMetadata>();
    is_clone::<khora_core::telemetry::metrics::MetricMetadata>();
    let _ = type_name::<khora_core::telemetry::metrics::MetricType>();
    let _ = metric_type_variants as fn(&khora_core::telemetry::metrics::MetricType);
    is_debug::<khora_core::telemetry::metrics::MetricType>();
    is_clone::<khora_core::telemetry::metrics::MetricType>();
    is_copy::<khora_core::telemetry::metrics::MetricType>();
    is_partial_eq::<khora_core::telemetry::metrics::MetricType>();
    is_eq::<khora_core::telemetry::metrics::MetricType>();
    let _ = type_name::<khora_core::telemetry::metrics::MetricValue>();
    let _ = type_name::<khora_core::telemetry::MetricValue>();
    same_type(
        PhantomData::<khora_core::telemetry::MetricValue>,
        PhantomData::<khora_core::telemetry::metrics::MetricValue>,
    );
    let _ = metric_value_variants as fn(&khora_core::telemetry::metrics::MetricValue);
    let _ = khora_core::telemetry::metrics::MetricValue::metric_type;
    let _ = khora_core::telemetry::metrics::MetricValue::as_f64;
    let _ = khora_core::telemetry::metrics::MetricValue::as_counter;
    let _ = khora_core::telemetry::metrics::MetricValue::as_gauge;
    is_debug::<khora_core::telemetry::metrics::MetricValue>();
    is_clone::<khora_core::telemetry::metrics::MetricValue>();
    let _ = type_name::<khora_core::telemetry::metrics::MetricsError>();
    let _ = type_name::<khora_core::telemetry::MetricsError>();
    same_type(
        PhantomData::<khora_core::telemetry::MetricsError>,
        PhantomData::<khora_core::telemetry::metrics::MetricsError>,
    );
    let _ = metrics_error_variants as fn(&khora_core::telemetry::metrics::MetricsError);
    is_debug::<khora_core::telemetry::metrics::MetricsError>();
    is_clone::<khora_core::telemetry::metrics::MetricsError>();
    is_display::<khora_core::telemetry::metrics::MetricsError>();
    is_error::<khora_core::telemetry::metrics::MetricsError>();
    let _ = type_name::<khora_core::telemetry::metrics::MetricsResult<u32>>();
    let _ = type_name::<khora_core::telemetry::MetricsResult<u32>>();
    same_type(
        PhantomData::<khora_core::telemetry::MetricsResult<u32>>,
        PhantomData::<khora_core::telemetry::metrics::MetricsResult<u32>>,
    );
    let _ = type_name::<khora_core::telemetry::monitoring::GpuReport>();
    let _ = type_name::<khora_core::telemetry::GpuReport>();
    same_type(
        PhantomData::<khora_core::telemetry::GpuReport>,
        PhantomData::<khora_core::telemetry::monitoring::GpuReport>,
    );
    let _ = gpu_report_fields as fn(&khora_core::telemetry::monitoring::GpuReport);
    let _ = khora_core::telemetry::monitoring::GpuReport::get_hook_timing_us;
    let _ = khora_core::telemetry::monitoring::GpuReport::main_pass_duration_us;
    let _ = khora_core::telemetry::monitoring::GpuReport::frame_total_duration_us;
    let _ = khora_core::telemetry::monitoring::GpuReport::set_hook_timing_us;
    is_debug::<khora_core::telemetry::monitoring::GpuReport>();
    is_clone::<khora_core::telemetry::monitoring::GpuReport>();
    is_copy::<khora_core::telemetry::monitoring::GpuReport>();
    is_default::<khora_core::telemetry::monitoring::GpuReport>();
    let _ = type_name::<khora_core::telemetry::monitoring::HardwareReport>();
    let _ = hardware_report_fields as fn(&khora_core::telemetry::monitoring::HardwareReport);
    is_debug::<khora_core::telemetry::monitoring::HardwareReport>();
    is_clone::<khora_core::telemetry::monitoring::HardwareReport>();
    is_copy::<khora_core::telemetry::monitoring::HardwareReport>();
    is_default::<khora_core::telemetry::monitoring::HardwareReport>();
    let _ = type_name::<khora_core::telemetry::monitoring::MemoryReport>();
    let _ = type_name::<khora_core::telemetry::MemoryReport>();
    same_type(
        PhantomData::<khora_core::telemetry::MemoryReport>,
        PhantomData::<khora_core::telemetry::monitoring::MemoryReport>,
    );
    let _ = memory_report_fields as fn(&khora_core::telemetry::monitoring::MemoryReport);
    let _ = khora_core::telemetry::monitoring::MemoryReport::current_usage_mb;
    let _ = khora_core::telemetry::monitoring::MemoryReport::peak_usage_mb;
    let _ = khora_core::telemetry::monitoring::MemoryReport::allocation_delta_kb;
    let _ = khora_core::telemetry::monitoring::MemoryReport::memory_turnover_rate;
    let _ = khora_core::telemetry::monitoring::MemoryReport::large_allocation_percentage;
    let _ = khora_core::telemetry::monitoring::MemoryReport::memory_utilization_efficiency;
    let _ = khora_core::telemetry::monitoring::MemoryReport::average_allocation_size_mb;
    let _ = khora_core::telemetry::monitoring::MemoryReport::fragmentation_status;
    is_debug::<khora_core::telemetry::monitoring::MemoryReport>();
    is_clone::<khora_core::telemetry::monitoring::MemoryReport>();
    is_copy::<khora_core::telemetry::monitoring::MemoryReport>();
    is_default::<khora_core::telemetry::monitoring::MemoryReport>();
    let _ = type_name::<khora_core::telemetry::monitoring::MonitoredResourceType>();
    let _ = type_name::<khora_core::telemetry::MonitoredResourceType>();
    same_type(
        PhantomData::<khora_core::telemetry::MonitoredResourceType>,
        PhantomData::<khora_core::telemetry::monitoring::MonitoredResourceType>,
    );
    let _ = monitored_resource_type_variants
        as fn(&khora_core::telemetry::monitoring::MonitoredResourceType);
    is_debug::<khora_core::telemetry::monitoring::MonitoredResourceType>();
    is_clone::<khora_core::telemetry::monitoring::MonitoredResourceType>();
    is_copy::<khora_core::telemetry::monitoring::MonitoredResourceType>();
    is_partial_eq::<khora_core::telemetry::monitoring::MonitoredResourceType>();
    is_eq::<khora_core::telemetry::monitoring::MonitoredResourceType>();
    is_hash::<khora_core::telemetry::monitoring::MonitoredResourceType>();
    // trait `khora_core::telemetry::monitoring::ResourceMonitor`: see `resource_monitor_trait_items`
    // trait `khora_core::telemetry::ResourceMonitor`: see `resource_monitor_trait_items`
    let _ = type_name::<khora_core::telemetry::monitoring::ResourceUsageReport>();
    let _ = type_name::<khora_core::telemetry::ResourceUsageReport>();
    same_type(
        PhantomData::<khora_core::telemetry::ResourceUsageReport>,
        PhantomData::<khora_core::telemetry::monitoring::ResourceUsageReport>,
    );
    let _ =
        resource_usage_report_fields as fn(&khora_core::telemetry::monitoring::ResourceUsageReport);
    is_debug::<khora_core::telemetry::monitoring::ResourceUsageReport>();
    is_clone::<khora_core::telemetry::monitoring::ResourceUsageReport>();
    is_copy::<khora_core::telemetry::monitoring::ResourceUsageReport>();
    is_default::<khora_core::telemetry::monitoring::ResourceUsageReport>();
    // trait `khora_core::telemetry::monitoring::VramProvider`: see `vram_provider_trait_items`
    // trait `khora_core::telemetry::VramProvider`: see `vram_provider_trait_items`
    let _ = type_name::<khora_core::telemetry::monitoring::VramReport>();
    let _ = type_name::<khora_core::telemetry::VramReport>();
    same_type(
        PhantomData::<khora_core::telemetry::VramReport>,
        PhantomData::<khora_core::telemetry::monitoring::VramReport>,
    );
    let _ = vram_report_fields as fn(&khora_core::telemetry::monitoring::VramReport);
    is_debug::<khora_core::telemetry::monitoring::VramReport>();
    is_clone::<khora_core::telemetry::monitoring::VramReport>();
    is_copy::<khora_core::telemetry::monitoring::VramReport>();
    is_default::<khora_core::telemetry::monitoring::VramReport>();
}

#[test]
fn module_time_paths_still_resolve() {
    let _ = khora_core::time::DEFAULT_FIXED_DELTA_SECONDS;
    let _ = type_name::<khora_core::time::SharedTime>();
    let _ = type_name::<khora_core::time::Time>();
    let _ = time_fields as fn(&khora_core::time::Time);
    let _ = khora_core::time::Time::with_fixed_delta;
    let _ = khora_core::time::Time::scale;
    let _ = khora_core::time::Time::set_scale;
    let _ = khora_core::time::Time::is_running;
    is_debug::<khora_core::time::Time>();
    is_clone::<khora_core::time::Time>();
    is_copy::<khora_core::time::Time>();
    is_partial_eq::<khora_core::time::Time>();
    is_default::<khora_core::time::Time>();
}

#[test]
fn module_util_paths_still_resolve() {
    let _ = type_name::<khora_core::util::any_map::AnyMap>();
    let _ = khora_core::util::any_map::AnyMap::new;
    let _ = khora_core::util::any_map::AnyMap::insert::<u32>;
    let _ = khora_core::util::any_map::AnyMap::get::<u32>;
    let _ = khora_core::util::any_map::AnyMap::contains::<u32>;
    let _ = khora_core::util::any_map::AnyMap::remove::<u32>;
    is_default::<khora_core::util::any_map::AnyMap>();
    let _ = type_name::<khora_core::util::stopwatch::Stopwatch>();
    let _ = type_name::<khora_core::Stopwatch>();
    same_type(
        PhantomData::<khora_core::Stopwatch>,
        PhantomData::<khora_core::util::stopwatch::Stopwatch>,
    );
    let _ = khora_core::util::stopwatch::Stopwatch::new;
    let _ = khora_core::util::stopwatch::Stopwatch::elapsed;
    let _ = khora_core::util::stopwatch::Stopwatch::elapsed_ms;
    let _ = khora_core::util::stopwatch::Stopwatch::elapsed_us;
    let _ = khora_core::util::stopwatch::Stopwatch::elapsed_secs_f64;
    is_debug::<khora_core::util::stopwatch::Stopwatch>();
    is_clone::<khora_core::util::stopwatch::Stopwatch>();
    is_default::<khora_core::util::stopwatch::Stopwatch>();
}

// ---------------------------------------------------------------------------
// Exported macros, each expanded once. Rustdoc does not list
// `khora_bitflags!` (`#[doc(hidden)]`), but it is `#[macro_export]`ed and the
// renderer's flag types are built with it. `script_value_table!` expands to
// `$crate::ecs::entity::EntityId` and `$crate::math::{Vec2, Vec3, Vec4,
// Quaternion, LinearRgba}`: moving any of those breaks the expansion here.
// ---------------------------------------------------------------------------

khora_core::khora_bitflags! {
    /// A flag set declared through the exported macro.
    pub struct ProbeFlags: u8 {
        /// First bit.
        const A = 1 << 0;
        /// Second bit.
        const B = 1 << 1;
    }
}

macro_rules! probe_script_value_table {
    ($($variant:ident : $rust:ty ;)*) => {
        /// Every row of the table, with the size of its Rust type.
        fn script_value_table_rows() -> Vec<(&'static str, usize)> {
            vec![$((stringify!($variant), std::mem::size_of::<$rust>()),)*]
        }

        /// Whether a value is one of the table's rows.
        fn script_value_is_regular(value: &khora_core::script::ScriptValue) -> bool {
            match value {
                $(khora_core::script::ScriptValue::$variant(_) => true,)*
                _ => false,
            }
        }
    };
}

khora_core::script_value_table!(probe_script_value_table);

#[test]
fn exported_macros_still_expand() {
    let flags = ProbeFlags::A | ProbeFlags::B;
    assert!(flags.contains(ProbeFlags::A));
    assert_eq!(ProbeFlags::ALL_DECLARED.bits(), flags.bits());

    let rows = script_value_table_rows();
    assert!(rows.iter().any(|(name, _)| *name == "Vec3"));
    assert!(script_value_is_regular(
        &khora_core::script::ScriptValue::Bool(true)
    ));
}

// ---------------------------------------------------------------------------
// Paths other crates of the workspace spell today (`crates/`, `examples/`,
// `xtask/`; brace imports expanded; `khora_sdk::khora_core::…`,
// `khora_sdk::editor_ui::…` and `khora_sdk::prelude::math::…` mapped onto
// the `khora_core` path the SDK re-exports). The trailing comment names the
// users. Items, modules and enum variants are imported; associated items
// are named in the test below.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod paths_used_by_other_crates {
    use khora_core::agent::completion::AgentCompletionMap as _; // khora-control
    use khora_core::agent::completion::CompletionOutcome as _; // khora-control
    use khora_core::agent::dependency::AgentDependency as _; // khora-control
    use khora_core::agent::dependency::DependencyKind as _; // khora-control
    use khora_core::agent::gorna::measured_frame_time_ms as _; // khora-agents
    use khora_core::agent::gorna::AdaptationMode as _; // khora-control
    use khora_core::agent::gorna::AgentFrameStatus as _; // khora-control
    use khora_core::agent::gorna::AgentFrameStatusMap as _; // khora-agents, khora-control, khora-sdk
    use khora_core::agent::gorna::AgentHints as _; // khora-control, khora-sdk
    use khora_core::agent::gorna::AgentId as _; // khora-agents, khora-control, khora-sdk
    use khora_core::agent::gorna::AgentId::Audio as _; // khora-sdk
    use khora_core::agent::gorna::AgentId::Overlay as _; // khora-sdk
    use khora_core::agent::gorna::AgentId::Physics as _; // khora-sdk
    use khora_core::agent::gorna::AgentId::Renderer as _; // khora-sdk
    use khora_core::agent::gorna::AgentId::Script as _; // khora-sdk
    use khora_core::agent::gorna::AgentId::ShadowRenderer as _; // khora-sdk
    use khora_core::agent::gorna::AgentId::Skybox as _; // khora-sdk
    use khora_core::agent::gorna::AgentId::Ui as _; // khora-sdk
    use khora_core::agent::gorna::AgentStatus as _; // khora-agents, khora-control, khora-sdk
    use khora_core::agent::gorna::DecisionTrace as _; // khora-control
    use khora_core::agent::gorna::EngineHint as _; // khora-control, khora-sdk
    use khora_core::agent::gorna::NegotiationRequest as _; // khora-agents, khora-control
    use khora_core::agent::gorna::NegotiationResponse as _; // khora-agents, khora-control
    use khora_core::agent::gorna::ResourceBudget as _; // khora-agents, khora-control
    use khora_core::agent::gorna::ResourceConstraints as _; // khora-agents, khora-control
    use khora_core::agent::gorna::StrategyId as _; // khora-agents, khora-control, khora-sdk
    use khora_core::agent::gorna::StrategyId::Balanced as _; // khora-agents
    use khora_core::agent::gorna::StrategyOption as _; // khora-agents, khora-control
    use khora_core::agent::gorna::TickDecisions as _; // khora-control
    use khora_core::agent::mode::EngineMode::Playing as _; // khora-agents
    use khora_core::agent::timing::AgentImportance as _; // khora-control
    use khora_core::agent::Agent as _; // khora-agents, khora-control
    use khora_core::agent::AgentAccess as _; // khora-agents, khora-control
    use khora_core::agent::AgentDependency as _; // khora-agents, khora-control
    use khora_core::agent::AgentImportance as _; // khora-agents, khora-control, khora-sdk
    use khora_core::agent::Contention as _; // khora-agents, khora-control
    use khora_core::agent::DependencyKind as _; // khora-agents
    use khora_core::agent::EngineMode as _; // khora-agents, khora-control
    use khora_core::agent::EngineMode::Playing as _; // khora-agents
    use khora_core::agent::ExecutionPhase as _; // khora-agents, khora-control, khora-sdk
    use khora_core::agent::ExecutionTiming as _; // khora-agents, khora-sdk
    use khora_core::asset::asset_key as _; // khora-editor (as khora_sdk::khora_core::…), khora-io
    use khora_core::asset::font::Font as _; // khora-data, khora-infra, khora-io
    use khora_core::asset::AlphaMode as _; // khora-data, khora-sdk
    use khora_core::asset::AlphaMode::Mask as _; // khora-io
    use khora_core::asset::Asset as _; // khora-agents, khora-data, khora-io
    use khora_core::asset::AssetHandle as _; // khora-agents, khora-data, khora-io, khora-lanes, khora-sdk, sandbox (as khora_sdk::khora_core::…)
    use khora_core::asset::AssetMetadata as _; // khora-agents, khora-io, xtask
    use khora_core::asset::AssetSource as _; // khora-agents, khora-io, khora-sdk, xtask
    use khora_core::asset::AssetUUID as _; // khora-agents, khora-data, khora-editor (as khora_sdk::khora_core::…), khora-infra, khora-io, khora-lanes, khora-sdk, sandbox (as khora_sdk::khora_core::…), xtask
    use khora_core::asset::CompressionKind as _; // khora-agents, khora-editor (as khora_sdk::khora_core::…), khora-io, xtask
    use khora_core::asset::EmissiveMaterial as _; // khora-data, khora-io, khora-sdk
    use khora_core::asset::Handle as _; // khora-infra
    use khora_core::asset::Material as _; // khora-data, khora-io, khora-lanes, khora-sdk
    use khora_core::asset::ScriptModule as _; // khora-io
    use khora_core::asset::StandardMaterial as _; // khora-data, khora-io, khora-sdk
    use khora_core::asset::UnlitMaterial as _; // khora-data, khora-sdk
    use khora_core::asset::WireframeMaterial as _; // khora-data, khora-sdk
    use khora_core::audio::AudioDevice as _; // khora-infra, khora-sdk
    use khora_core::audio::AudioMixBus as _; // khora-agents, khora-infra, khora-lanes, khora-sdk
    use khora_core::audio::AudioStream as _; // khora-infra, khora-sdk
    use khora_core::audio::StreamInfo as _; // khora-infra, khora-lanes, khora-sdk
    use khora_core::ecs::entity::EntityId as _; // khora-agents, khora-data, khora-infra, khora-io, khora-lanes, khora-script, khora-sdk
    use khora_core::engine_context::EngineContext as _; // khora-agents
    use khora_core::event::Channel as _; // khora-agents, khora-data, khora-io, khora-lanes, khora-script, khora-sdk
    use khora_core::event::Supersedes as _; // khora-io, khora-script
    use khora_core::event::WhenFull as _; // khora-io, khora-script
    use khora_core::graph::topological_sort as _; // khora-control, khora-data
    use khora_core::interpolation::SharedTransformInterpolation as _; // khora-agents, khora-data, khora-sdk
    use khora_core::interpolation::TransformInterpolation as _; // khora-data
    use khora_core::lane::lock::mutex_lock as _; // khora-lanes
    use khora_core::lane::lock::mutex_lock_render as _; // khora-lanes
    use khora_core::lane::lock::read_lock as _; // khora-lanes
    use khora_core::lane::lock::read_lock_render as _; // khora-lanes
    use khora_core::lane::lock::write_lock as _; // khora-lanes
    use khora_core::lane::lock::write_lock_render as _; // khora-lanes
    use khora_core::lane::ClearColor as _; // khora-agents, khora-lanes, khora-sdk
    use khora_core::lane::ColorTarget as _; // khora-agents, khora-lanes, khora-sdk
    use khora_core::lane::DepthTarget as _; // khora-agents, khora-lanes, khora-sdk
    use khora_core::lane::Lane as _; // khora-agents, khora-lanes
    use khora_core::lane::LaneBus as _; // khora-agents, khora-control, khora-data
    use khora_core::lane::LaneContext as _; // khora-agents, khora-lanes
    use khora_core::lane::LaneError as _; // khora-lanes
    use khora_core::lane::LaneError::InitializationFailed as _; // khora-lanes
    use khora_core::lane::LaneKind as _; // khora-agents, khora-lanes
    use khora_core::lane::LaneKind::Audio as _; // khora-lanes
    use khora_core::lane::LaneKind::Physics as _; // khora-lanes
    use khora_core::lane::LaneKind::Render as _; // khora-lanes
    use khora_core::lane::LaneRegistry as _; // khora-agents
    use khora_core::lane::OutputDeck as _; // khora-agents, khora-control, khora-data, khora-io, khora-lanes
    use khora_core::lane::PhysicsDeltaTime as _; // khora-agents, khora-lanes
    use khora_core::lane::ShadowAtlasView as _; // khora-agents, khora-lanes
    use khora_core::lane::ShadowComparisonSampler as _; // khora-agents, khora-lanes
    use khora_core::math as _; // khora-sdk (glob re-export)
    use khora_core::math::affine_transform::AffineTransform as _; // khora-agents, khora-data, khora-lanes
    use khora_core::math::dimension as _; // khora-infra
    use khora_core::math::geometry::Aabb as _; // khora-io
    use khora_core::math::simd::compose_trs_to_mat4 as _; // khora-data
    use khora_core::math::simd::compose_trs_to_mat4_scalar as _; // khora-data
    use khora_core::math::simd::normalize_quat_batch as _; // khora-data
    use khora_core::math::simd::TrsBatchSoa as _; // khora-data
    use khora_core::math::vector::Vec3 as _; // khora-lanes
    use khora_core::math::Aabb as _; // khora-data, khora-editor (as khora_sdk::khora_core::…), khora-infra
    use khora_core::math::AffineTransform as _; // khora-data, khora-infra
    use khora_core::math::Extent2D as _; // khora-infra
    use khora_core::math::Extent3D as _; // khora-data, khora-infra, khora-io, khora-lanes, sandbox (as khora_sdk::khora_core::…)
    use khora_core::math::LinearRgba as _; // khora-data, khora-editor (as khora_sdk::prelude::math::…), khora-infra, khora-io, khora-script, khora-sdk, khora-tool-ui
    use khora_core::math::Mat4 as _; // khora-agents, khora-data, khora-editor (as khora_sdk::khora_core::…), khora-lanes, khora-sdk
    use khora_core::math::Origin3D as _; // khora-data, khora-infra
    use khora_core::math::Quat as _; // khora-agents, khora-data, khora-infra
    use khora_core::math::Quaternion as _; // khora-agents, khora-data, khora-script, khora-sdk, sandbox (as khora_sdk::prelude::math::…)
    use khora_core::math::Ray as _; // khora-editor (as khora_sdk::khora_core::…)
    use khora_core::math::Rect2D as _; // khora-sdk
    use khora_core::math::Vec2 as _; // khora-data, khora-infra, khora-io, khora-script, khora-sdk
    use khora_core::math::Vec3 as _; // khora-agents, khora-data, khora-editor (as khora_sdk::khora_core::…, khora_sdk::prelude::math::…), khora-infra, khora-io, khora-lanes, khora-script, khora-sdk, sandbox (as khora_sdk::prelude::math::…)
    use khora_core::math::Vec4 as _; // khora-data, khora-infra, khora-io, khora-lanes, khora-script
    use khora_core::math::EPSILON as _; // khora-data
    use khora_core::memory::get_currently_allocated_bytes as _; // khora-infra
    use khora_core::memory::get_extended_memory_stats as _; // khora-infra
    use khora_core::physics::collision_channel as _; // khora-agents, khora-data, khora-sdk
    use khora_core::physics::BodyType as _; // khora-agents, khora-data, khora-infra, khora-sdk
    use khora_core::physics::CharacterControllerOptions as _; // khora-data, khora-infra
    use khora_core::physics::ColliderDesc as _; // khora-agents, khora-data, khora-infra
    use khora_core::physics::ColliderHandle as _; // khora-data, khora-infra
    use khora_core::physics::ColliderShape as _; // khora-agents, khora-data, khora-infra, khora-sdk
    use khora_core::physics::Collision as _; // khora-data, khora-lanes
    use khora_core::physics::CollisionEvent as _; // khora-infra, khora-lanes
    use khora_core::physics::CollisionEvent::Started as _; // khora-infra
    use khora_core::physics::CollisionEvent::Stopped as _; // khora-infra
    use khora_core::physics::CollisionKind as _; // khora-agents, khora-data, khora-lanes
    use khora_core::physics::ContactBatch as _; // khora-agents, khora-data, khora-lanes
    use khora_core::physics::PhysicsProvider as _; // khora-agents, khora-data, khora-infra, khora-lanes, khora-sdk
    use khora_core::physics::Ray as _; // khora-agents, khora-infra
    use khora_core::physics::RaycastHit as _; // khora-infra
    use khora_core::physics::RigidBodyDesc as _; // khora-agents, khora-data, khora-infra
    use khora_core::physics::RigidBodyHandle as _; // khora-data, khora-infra
    use khora_core::physics::SlotId as _; // khora-infra
    use khora_core::platform::input::KeyCode as _; // khora-infra
    use khora_core::platform::input_channel as _; // khora-agents, khora-data, khora-sdk
    use khora_core::platform::window::KhoraWindow as _; // khora-infra
    use khora_core::platform::window::KhoraWindowHandle as _; // khora-infra
    use khora_core::platform::BatteryLevel as _; // khora-control, khora-infra
    use khora_core::platform::BatteryLevel::Mains as _; // khora-control
    use khora_core::platform::HardwareMonitor as _; // khora-infra
    use khora_core::platform::InputBinding as _; // khora-agents, khora-data, sandbox (as khora_sdk::khora_core::…)
    use khora_core::platform::InputEvent as _; // khora-agents, khora-data, khora-infra, khora-sdk
    use khora_core::platform::InputMap as _; // khora-agents, khora-data, sandbox (as khora_sdk::khora_core::…)
    use khora_core::platform::InputSnapshot as _; // khora-agents, khora-data, khora-script
    use khora_core::platform::KeyCode as _; // khora-agents, khora-data, khora-infra, khora-sdk
    use khora_core::platform::KhoraWindow as _; // khora-editor (as khora_sdk::khora_core::…), khora-sdk
    use khora_core::platform::MouseButton as _; // khora-infra, khora-sdk
    use khora_core::platform::ThermalStatus as _; // khora-control, khora-infra
    use khora_core::platform::ThermalStatus::Critical as _; // khora-control
    use khora_core::platform::ThermalStatus::Throttling as _; // khora-control
    use khora_core::renderer::api::command as _; // khora-infra
    use khora_core::renderer::api::command::bind_group::SamplerBindingType as _; // khora-infra
    use khora_core::renderer::api::command::bind_group::TextureSampleType as _; // khora-infra
    use khora_core::renderer::api::command::pass::LoadOp as _; // khora-infra
    use khora_core::renderer::api::command::pass::StoreOp as _; // khora-infra
    use khora_core::renderer::api::command::BindGroupDescriptor as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::command::BindGroupEntry as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::command::BindGroupId as _; // khora-data, khora-infra, khora-io, khora-lanes
    use khora_core::renderer::api::command::BindGroupLayoutDescriptor as _; // khora-infra
    use khora_core::renderer::api::command::BindGroupLayoutEntry as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::command::BindGroupLayoutId as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::command::BindingResource as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::command::BindingType as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::command::BufferBinding as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::command::BufferBindingType as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::command::BufferBindingType::Uniform as _; // khora-infra
    use khora_core::renderer::api::command::CommandBufferId as _; // khora-data, khora-infra
    use khora_core::renderer::api::command::ComputePassDescriptor as _; // khora-infra, khora-lanes
    use khora_core::renderer::api::command::DrawCommand as _; // khora-lanes
    use khora_core::renderer::api::command::LoadOp as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::command::LoadOp::Clear as _; // khora-lanes
    use khora_core::renderer::api::command::Operations as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::command::RenderPassColorAttachment as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::command::RenderPassDepthStencilAttachment as _; // khora-infra, khora-lanes
    use khora_core::renderer::api::command::RenderPassDescriptor as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::command::SamplerBindingType as _; // khora-data, khora-lanes
    use khora_core::renderer::api::command::SamplerBindingType::Filtering as _; // khora-infra, khora-lanes
    use khora_core::renderer::api::command::StoreOp as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::command::StoreOp::Store as _; // khora-lanes
    use khora_core::renderer::api::command::TextureSampleType as _; // khora-data, khora-lanes
    use khora_core::renderer::api::command::TextureSampleType::Float as _; // khora-infra, khora-lanes
    use khora_core::renderer::api::command::TextureViewDimension as _; // khora-data, khora-lanes
    use khora_core::renderer::api::command::TextureViewDimension::D2 as _; // khora-infra
    use khora_core::renderer::api::device::BackendSelectionConfig as _; // khora-infra
    use khora_core::renderer::api::device::BackendSelectionResult as _; // khora-infra
    use khora_core::renderer::api::device::GpuHook as _; // khora-infra
    use khora_core::renderer::api::device::GraphicsAdapterInfo as _; // khora-infra
    use khora_core::renderer::api::device::GraphicsBackendType as _; // khora-infra
    use khora_core::renderer::api::device::RenderSettings as _; // khora-infra
    use khora_core::renderer::api::device::RenderStats as _; // khora-infra
    use khora_core::renderer::api::device::RendererDeviceType as _; // khora-infra
    use khora_core::renderer::api::frame::FrameContext as _; // khora-agents, khora-sdk
    use khora_core::renderer::api::frame::RenderContext as _; // khora-lanes
    use khora_core::renderer::api::frame::MAX_FRAMES_IN_FLIGHT as _; // khora-lanes
    use khora_core::renderer::api::gpu_scene as _; // khora-sdk
    use khora_core::renderer::api::gpu_scene::mesh::Mesh as _; // khora-editor (as khora_sdk::khora_core::…), khora-sdk
    use khora_core::renderer::api::gpu_scene::GpuMesh as _; // khora-agents, khora-data, khora-io, khora-lanes
    use khora_core::renderer::api::gpu_scene::Mesh as _; // khora-data, khora-editor (as khora_sdk::khora_core::…), khora-io, khora-sdk
    use khora_core::renderer::api::gpu_scene::ModelUniforms as _; // khora-lanes
    use khora_core::renderer::api::gpu_scene::RenderObject as _; // khora-infra
    use khora_core::renderer::api::ibl::fill_ibl_bind_group_entries as _; // khora-lanes
    use khora_core::renderer::api::ibl::ibl_bind_group_layout_entries as _; // khora-lanes
    use khora_core::renderer::api::ibl::IblGpuBindings as _; // khora-data, khora-lanes
    use khora_core::renderer::api::material::bindings::flag as _; // khora-data, khora-infra
    use khora_core::renderer::api::material::fill_material_bind_group_entries as _; // khora-data
    use khora_core::renderer::api::material::GpuMaterial as _; // khora-data, khora-io
    use khora_core::renderer::api::material::MaterialGpuBindings as _; // khora-data
    use khora_core::renderer::api::material::MaterialUniforms as _; // khora-data, khora-lanes
    use khora_core::renderer::api::pipeline::enums::BlendFactor as _; // khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::enums::BlendOperation as _; // khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::enums::CompareFunction as _; // khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::enums::CullMode as _; // khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::enums::FrontFace as _; // khora-infra
    use khora_core::renderer::api::pipeline::enums::PolygonMode as _; // khora-infra
    use khora_core::renderer::api::pipeline::enums::PrimitiveTopology as _; // khora-infra, khora-io, khora-lanes
    use khora_core::renderer::api::pipeline::enums::StencilOperation as _; // khora-infra
    use khora_core::renderer::api::pipeline::enums::VertexFormat as _; // khora-infra, khora-io, khora-lanes
    use khora_core::renderer::api::pipeline::enums::VertexStepMode as _; // khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::state::BlendComponentDescriptor as _; // khora-lanes
    use khora_core::renderer::api::pipeline::state::BlendStateDescriptor as _; // khora-lanes
    use khora_core::renderer::api::pipeline::state::ColorWrites as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::state::DepthBiasState as _; // khora-lanes
    use khora_core::renderer::api::pipeline::state::StencilFaceState as _; // khora-lanes
    use khora_core::renderer::api::pipeline::BlendComponentDescriptor as _; // khora-infra
    use khora_core::renderer::api::pipeline::BlendFactor::One as _; // khora-infra
    use khora_core::renderer::api::pipeline::BlendFactor::OneMinusSrcAlpha as _; // khora-infra
    use khora_core::renderer::api::pipeline::BlendFactor::SrcAlpha as _; // khora-infra
    use khora_core::renderer::api::pipeline::BlendFactor::Zero as _; // khora-infra
    use khora_core::renderer::api::pipeline::BlendOperation::Add as _; // khora-infra
    use khora_core::renderer::api::pipeline::BlendStateDescriptor as _; // khora-infra
    use khora_core::renderer::api::pipeline::ColorTargetStateDescriptor as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::ColorWrites as _; // khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::ComputePipelineDescriptor as _; // khora-infra
    use khora_core::renderer::api::pipeline::ComputePipelineId as _; // khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::ComputePipelineKey as _; // khora-infra
    use khora_core::renderer::api::pipeline::ComputePipelineSpec as _; // khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::DepthStencilStateDescriptor as _; // khora-lanes
    use khora_core::renderer::api::pipeline::LayoutCacheKey as _; // khora-infra
    use khora_core::renderer::api::pipeline::LayoutKey as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::LayoutSpec as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::MultisampleStateDescriptor as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::PipelineKey as _; // khora-infra
    use khora_core::renderer::api::pipeline::PipelineLayoutDescriptor as _; // khora-infra
    use khora_core::renderer::api::pipeline::PipelineLayoutId as _; // khora-infra
    use khora_core::renderer::api::pipeline::PipelineSpec as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::PrimitiveStateDescriptor as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::PrimitiveTopology as _; // khora-agents, khora-data, khora-infra, khora-io, khora-lanes
    use khora_core::renderer::api::pipeline::RenderPipelineDescriptor as _; // khora-infra
    use khora_core::renderer::api::pipeline::RenderPipelineId as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::VertexAttributeDescriptor as _; // khora-data, khora-infra, khora-io, khora-lanes
    use khora_core::renderer::api::pipeline::VertexBufferLayoutDescriptor as _; // khora-infra, khora-lanes
    use khora_core::renderer::api::pipeline::VertexFormat as _; // khora-data
    use khora_core::renderer::api::pipeline::VertexFormat::Float32x2 as _; // khora-infra
    use khora_core::renderer::api::pipeline::VertexFormat::Float32x4 as _; // khora-infra
    use khora_core::renderer::api::pipeline::VertexStepMode::Vertex as _; // khora-infra
    use khora_core::renderer::api::resource as _; // khora-sdk
    use khora_core::renderer::api::resource::buffer as _; // khora-infra
    use khora_core::renderer::api::resource::buffer::BufferUsage as _; // khora-infra
    use khora_core::renderer::api::resource::texture as _; // khora-infra
    use khora_core::renderer::api::resource::texture::AddressMode as _; // khora-infra
    use khora_core::renderer::api::resource::texture::FilterMode as _; // khora-infra
    use khora_core::renderer::api::resource::texture::ImageAspect as _; // khora-infra
    use khora_core::renderer::api::resource::texture::MipmapFilterMode as _; // khora-infra
    use khora_core::renderer::api::resource::texture::SamplerBorderColor as _; // khora-infra
    use khora_core::renderer::api::resource::texture::TextureDimension as _; // khora-infra
    use khora_core::renderer::api::resource::texture::TextureUsage as _; // khora-infra
    use khora_core::renderer::api::resource::texture::TextureViewDimension as _; // khora-infra
    use khora_core::renderer::api::resource::AddressMode as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::resource::AddressMode::ClampToEdge as _; // khora-lanes
    use khora_core::renderer::api::resource::BufferDescriptor as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::resource::BufferId as _; // khora-agents, khora-data, khora-infra, khora-io, khora-lanes
    use khora_core::renderer::api::resource::BufferUsage as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::resource::CameraUniformData as _; // khora-infra, khora-lanes
    use khora_core::renderer::api::resource::CpuTexture as _; // khora-agents, khora-data, khora-io, sandbox (as khora_sdk::khora_core::…)
    use khora_core::renderer::api::resource::FilterMode as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::resource::FilterMode::Linear as _; // khora-lanes
    use khora_core::renderer::api::resource::ImageAspect as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::resource::IndexFormat as _; // khora-infra
    use khora_core::renderer::api::resource::IndexFormat as _; // khora-agents, khora-data, khora-infra, khora-io, khora-lanes
    use khora_core::renderer::api::resource::MipmapFilterMode as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::resource::MipmapFilterMode::Linear as _; // khora-lanes
    use khora_core::renderer::api::resource::SampleCount as _; // khora-infra
    use khora_core::renderer::api::resource::SampleCount as _; // khora-data, khora-infra, khora-io, khora-lanes, sandbox (as khora_sdk::khora_core::…)
    use khora_core::renderer::api::resource::SamplerDescriptor as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::resource::SamplerId as _; // khora-data, khora-infra, khora-io, khora-lanes
    use khora_core::renderer::api::resource::TextureColorSpace as _; // khora-data
    use khora_core::renderer::api::resource::TextureDescriptor as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::resource::TextureDimension as _; // khora-data, khora-infra, khora-io, khora-lanes, sandbox (as khora_sdk::khora_core::…)
    use khora_core::renderer::api::resource::TextureFormat as _; // khora-infra
    use khora_core::renderer::api::resource::TextureFormat as _; // khora-data, khora-infra, khora-io, khora-lanes, sandbox (as khora_sdk::khora_core::…)
    use khora_core::renderer::api::resource::TextureFormat::Rgba8Unorm as _; // khora-agents
    use khora_core::renderer::api::resource::TextureId as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::resource::TextureUsage as _; // khora-data, khora-infra, khora-io, khora-lanes, sandbox (as khora_sdk::khora_core::…)
    use khora_core::renderer::api::resource::TextureViewDescriptor as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::resource::TextureViewDimension as _; // khora-lanes
    use khora_core::renderer::api::resource::TextureViewId as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::resource::ViewInfo as _; // khora-data, khora-editor (as khora_sdk::khora_core::…), khora-infra
    use khora_core::renderer::api::shader::defs::ShaderDefs as _; // khora-infra
    use khora_core::renderer::api::shader::CpuShaderSource as _; // khora-io
    use khora_core::renderer::api::shader::ShaderDefScalar as _; // khora-infra
    use khora_core::renderer::api::shader::ShaderModuleDescriptor as _; // khora-infra
    use khora_core::renderer::api::shader::ShaderModuleId as _; // khora-infra
    use khora_core::renderer::api::shader::ShaderSourceData as _; // khora-infra
    use khora_core::renderer::api::shader::ShaderStage as _; // khora-infra
    use khora_core::renderer::api::shader::ShaderVariantKey as _; // khora-data, khora-infra, khora-io, khora-lanes
    use khora_core::renderer::api::shadow::bindings as _; // khora-lanes
    use khora_core::renderer::api::shadow::bindings::binding::ATLAS_2D as _; // khora-lanes
    use khora_core::renderer::api::shadow::bindings::binding::ATLAS_CUBE as _; // khora-lanes
    use khora_core::renderer::api::shadow::bindings::binding::LIGHTING_UNIFORMS as _; // khora-lanes
    use khora_core::renderer::api::shadow::bindings::fill_shadow_bind_group_entries as _; // khora-lanes
    use khora_core::renderer::api::shadow::bindings::shadow_bind_group_layout_entries as _; // khora-lanes
    use khora_core::renderer::api::shadow::ShadowEntries as _; // khora-lanes
    use khora_core::renderer::api::shadow::ShadowEntry as _; // khora-lanes
    use khora_core::renderer::api::shadow::ShadowEntry::Atlas2D as _; // khora-lanes
    use khora_core::renderer::api::shadow::ShadowEntry::Cube as _; // khora-lanes
    use khora_core::renderer::api::shadow::ShadowFrame as _; // khora-data, khora-lanes
    use khora_core::renderer::api::shadow::ShadowGpuBindings as _; // khora-lanes
    use khora_core::renderer::api::text::TextLayout as _; // khora-data, khora-infra
    use khora_core::renderer::api::text::TextRenderer as _; // khora-agents, khora-data, khora-infra, khora-lanes, khora-sdk
    use khora_core::renderer::api::util::f32_to_f16_bits as _; // khora-io
    use khora_core::renderer::api::util::flags::ShaderStageFlags as _; // khora-infra
    use khora_core::renderer::api::util::AtlasRect as _; // khora-data
    use khora_core::renderer::api::util::ShaderStageFlags as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::api::util::TextureAtlas as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::error::RenderError as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::error::RenderError::ResourceError as _; // khora-lanes
    use khora_core::renderer::error::ResourceError as _; // khora-infra
    use khora_core::renderer::error::ResourceError::BackendError as _; // khora-lanes
    use khora_core::renderer::light as _; // khora-sdk
    use khora_core::renderer::light::CullingUniformsData as _; // khora-lanes
    use khora_core::renderer::light::DirectionalLight as _; // khora-agents, khora-data, khora-editor (as khora_sdk::khora_core::…), khora-lanes, khora-sdk
    use khora_core::renderer::light::DirectionalLightUniform as _; // khora-lanes
    use khora_core::renderer::light::LightType as _; // khora-agents, khora-data, khora-editor (as khora_sdk::khora_core::…), khora-lanes, khora-sdk
    use khora_core::renderer::light::LightType::Directional as _; // khora-lanes
    use khora_core::renderer::light::LightType::Point as _; // khora-lanes
    use khora_core::renderer::light::LightType::Spot as _; // khora-lanes
    use khora_core::renderer::light::LightingUniforms as _; // khora-lanes
    use khora_core::renderer::light::PointLight as _; // khora-agents, khora-data, khora-editor (as khora_sdk::khora_core::…), khora-lanes, khora-sdk
    use khora_core::renderer::light::PointLightUniform as _; // khora-lanes
    use khora_core::renderer::light::SpotLight as _; // khora-data, khora-editor (as khora_sdk::khora_core::…), khora-sdk
    use khora_core::renderer::light::SpotLightUniform as _; // khora-lanes
    use khora_core::renderer::light::MAX_DIRECTIONAL_LIGHTS as _; // khora-lanes
    use khora_core::renderer::light::MAX_POINT_LIGHTS as _; // khora-lanes
    use khora_core::renderer::light::MAX_SPOT_LIGHTS as _; // khora-lanes
    use khora_core::renderer::traits::CommandEncoder as _; // khora-data, khora-infra, khora-lanes
    use khora_core::renderer::traits::ComputePass as _; // khora-infra
    use khora_core::renderer::traits::FrameTargets as _; // khora-infra
    use khora_core::renderer::traits::GpuProfiler as _; // khora-infra
    use khora_core::renderer::traits::GraphicsBackendSelector as _; // khora-infra
    use khora_core::renderer::traits::GraphicsDevice as _; // khora-infra, khora-io
    use khora_core::renderer::traits::PipelineSystem as _; // khora-agents, khora-data, khora-infra, khora-io, khora-lanes, khora-sdk
    use khora_core::renderer::traits::RenderPass as _; // khora-infra, khora-lanes
    use khora_core::renderer::traits::RenderSystem as _; // khora-infra, khora-sdk
    use khora_core::renderer::ForwardPlusTileConfig as _; // khora-lanes
    use khora_core::renderer::GraphicsDevice as _; // khora-agents, khora-data, khora-infra, khora-lanes, khora-sdk
    use khora_core::renderer::PipelineError as _; // khora-infra
    use khora_core::renderer::RenderError as _; // khora-infra
    use khora_core::renderer::RenderSystem as _; // khora-agents
    use khora_core::renderer::ResourceError as _; // khora-infra
    use khora_core::renderer::ShaderError as _; // khora-infra
    use khora_core::renderer::TileSize as _; // khora-lanes
    use khora_core::scene::SceneFile as _; // khora-io, khora-sdk
    use khora_core::scene::SceneHeader as _; // khora-io
    use khora_core::scene::SerializationGoal as _; // khora-io, khora-sdk
    use khora_core::scene::HEADER_MAGIC_BYTES as _; // khora-io
    use khora_core::script::engine_event_channel as _; // khora-agents, khora-data, khora-sdk
    use khora_core::script::CommandBuffer as _; // khora-agents, khora-data, khora-lanes, khora-script
    use khora_core::script::ComponentName as _; // khora-data
    use khora_core::script::EventQueue as _; // khora-lanes, khora-script
    use khora_core::script::PendingSequence as _; // khora-lanes
    use khora_core::script::ScriptEvent as _; // khora-agents, khora-data, khora-lanes, khora-script, khora-sdk
    use khora_core::script::ScriptSnapshot as _; // khora-data, khora-lanes
    use khora_core::script::ScriptStateUpdate as _; // khora-data, khora-lanes
    use khora_core::script::ScriptStateWriteback as _; // khora-agents, khora-data, khora-lanes
    use khora_core::script::ScriptValue as _; // khora-agents, khora-data, khora-lanes, khora-script, khora-sdk
    use khora_core::script::ScriptValue::Int as _; // khora-lanes
    use khora_core::script::ScriptValue::Unit as _; // khora-data
    use khora_core::script::TimerRemaining as _; // khora-lanes
    use khora_core::script::WorldCommand as _; // khora-agents, khora-data, khora-lanes, khora-script
    use khora_core::script_value_table as _; // khora-data, khora-script
    use khora_core::telemetry::event::TelemetryEvent as _; // khora-telemetry
    use khora_core::telemetry::metrics::Metric as _; // khora-telemetry
    use khora_core::telemetry::metrics::MetricId as _; // khora-infra, khora-telemetry
    use khora_core::telemetry::metrics::MetricType as _; // khora-telemetry
    use khora_core::telemetry::metrics::MetricValue as _; // khora-infra, khora-telemetry
    use khora_core::telemetry::metrics::MetricValue::Histogram as _; // khora-telemetry
    use khora_core::telemetry::metrics::MetricsError as _; // khora-telemetry
    use khora_core::telemetry::metrics::MetricsResult as _; // khora-telemetry
    use khora_core::telemetry::monitoring::GpuReport as _; // khora-infra
    use khora_core::telemetry::monitoring::HardwareReport as _; // khora-control
    use khora_core::telemetry::monitoring::MemoryReport as _; // khora-infra
    use khora_core::telemetry::monitoring::MonitoredResourceType as _; // khora-infra
    use khora_core::telemetry::monitoring::ResourceMonitor as _; // khora-infra
    use khora_core::telemetry::monitoring::ResourceUsageReport as _; // khora-infra
    use khora_core::telemetry::monitoring::VramProvider as _; // khora-infra
    use khora_core::telemetry::Metric as _; // khora-telemetry
    use khora_core::telemetry::MetricId as _; // khora-control, khora-telemetry
    use khora_core::telemetry::MetricValue as _; // khora-control, khora-telemetry
    use khora_core::telemetry::MetricValue::Gauge as _; // khora-control
    use khora_core::telemetry::MetricsError as _; // khora-telemetry
    use khora_core::telemetry::MetricsResult as _; // khora-telemetry
    use khora_core::telemetry::MonitoredResourceType as _; // khora-infra, khora-sdk
    use khora_core::telemetry::ResourceMonitor as _; // khora-infra, khora-telemetry
    use khora_core::telemetry::ResourceUsageReport as _; // khora-infra
    use khora_core::telemetry::TelemetryEvent as _; // khora-control, khora-sdk
    use khora_core::telemetry::TelemetryEvent::PhaseChange as _; // khora-sdk
    use khora_core::telemetry::VramProvider as _; // khora-infra
    use khora_core::time::SharedTime as _; // khora-agents, khora-control, khora-data, khora-sdk
    use khora_core::time::Time as _; // khora-control, khora-sdk
    use khora_core::time::DEFAULT_FIXED_DELTA_SECONDS as _; // khora-control
    use khora_core::ui::editor as _; // khora-sdk (glob re-export)
    use khora_core::ui::editor::gizmo::GizmoLineInstance as _; // khora-sdk
    use khora_core::ui::editor::overlay::EditorOverlay as _; // khora-infra
    use khora_core::ui::editor::overlay::OverlayError as _; // khora-infra
    use khora_core::ui::editor::overlay::OverlayScreenDescriptor as _; // khora-infra
    use khora_core::ui::editor::panel::EditorPanel as _; // khora-infra
    use khora_core::ui::editor::panel::PanelLocation as _; // khora-infra
    use khora_core::ui::editor::shell::EditorShell as _; // khora-infra
    use khora_core::ui::editor::state::EditorState as _; // khora-infra
    use khora_core::ui::editor::state::StatusBarData as _; // khora-infra
    use khora_core::ui::editor::ui_builder::FontFamilyHint as _; // khora-infra, khora-tool-ui
    use khora_core::ui::editor::ui_builder::FontFamilyHint::Monospace as _; // khora-tool-ui
    use khora_core::ui::editor::ui_builder::InlineEditEvent as _; // khora-infra
    use khora_core::ui::editor::ui_builder::Interaction as _; // khora-infra, khora-tool-ui
    use khora_core::ui::editor::ui_builder::TextAlign as _; // khora-infra, khora-tool-ui
    use khora_core::ui::editor::ui_builder::TextAlign::Center as _; // khora-tool-ui
    use khora_core::ui::editor::viewport_texture::ViewportTextureHandle as _; // khora-editor (as khora_sdk::editor_ui::…), khora-infra, khora-sdk, khora-tool-ui
    use khora_core::ui::editor::AssetEntry as _; // khora-editor (as khora_sdk::editor_ui::…), khora-sdk
    use khora_core::ui::editor::ComponentJson as _; // khora-sdk
    use khora_core::ui::editor::EditorMode as _; // khora-editor (as khora_sdk::editor_ui::…), khora-sdk
    use khora_core::ui::editor::EditorPanel as _; // khora-editor (as khora_sdk::editor_ui::…), khora-sdk
    use khora_core::ui::editor::EditorShell as _; // khora-sdk
    use khora_core::ui::editor::EditorState as _; // khora-editor (as khora_sdk::editor_ui::…), khora-sdk
    use khora_core::ui::editor::EntityIcon as _; // khora-sdk
    use khora_core::ui::editor::FontFamilyHint as _; // khora-editor (as khora_sdk::editor_ui::…), khora-sdk
    use khora_core::ui::editor::FontFamilyHint::Monospace as _; // khora-editor (as khora_sdk::editor_ui::…)
    use khora_core::ui::editor::GizmoLineInstance as _; // khora-data, khora-editor (as khora_sdk::editor_ui::…), khora-lanes
    use khora_core::ui::editor::GizmoMode as _; // khora-sdk
    use khora_core::ui::editor::Icon as _; // khora-editor (as khora_sdk::editor_ui::…), khora-sdk, khora-tool-ui
    use khora_core::ui::editor::InlineEditEvent as _; // khora-sdk
    use khora_core::ui::editor::InspectedEntity as _; // khora-editor (as khora_sdk::editor_ui::…), khora-sdk
    use khora_core::ui::editor::Interaction as _; // khora-editor (as khora_sdk::editor_ui::…), khora-sdk
    use khora_core::ui::editor::LogEntry as _; // khora-sdk
    use khora_core::ui::editor::LogLevel as _; // khora-sdk
    use khora_core::ui::editor::PanelLocation as _; // khora-sdk
    use khora_core::ui::editor::PlayMode as _; // khora-sdk
    use khora_core::ui::editor::PropertyEdit as _; // khora-editor (as khora_sdk::editor_ui::…), khora-sdk
    use khora_core::ui::editor::SceneNode as _; // khora-editor (as khora_sdk::editor_ui::…), khora-sdk
    use khora_core::ui::editor::StatusBarData as _; // khora-sdk
    use khora_core::ui::editor::TextAlign as _; // khora-editor (as khora_sdk::editor_ui::…), khora-sdk
    use khora_core::ui::editor::UiBuilder as _; // khora-editor (as khora_sdk::editor_ui::…), khora-infra, khora-sdk, khora-tool-ui
    use khora_core::ui::editor::ViewportTextureHandle as _; // khora-infra
    use khora_core::ui::fonts::FontHandle as _; // khora-editor (as khora_sdk::editor_ui::…), khora-infra, khora-sdk
    use khora_core::ui::fonts::FontPack as _; // khora-editor (as khora_sdk::editor_ui::…), khora-infra, khora-sdk
    use khora_core::ui::fonts::NamedFont as _; // khora-editor (as khora_sdk::editor_ui::…), khora-infra, khora-sdk
    use khora_core::ui::layout::UiLayoutView as _; // khora-data, khora-infra
    use khora_core::ui::theme::UiTheme as _; // khora-editor (as khora_sdk::editor_ui::…), khora-sdk, khora-tool-ui
    use khora_core::ui::types::UiBorder as _; // khora-data
    use khora_core::ui::types::UiColor as _; // khora-data
    use khora_core::ui::types::UiFlexDirection as _; // khora-data, khora-infra
    use khora_core::ui::types::UiImage as _; // khora-data
    use khora_core::ui::types::UiNode as _; // khora-data, khora-infra
    use khora_core::ui::types::UiRect as _; // khora-data
    use khora_core::ui::types::UiText as _; // khora-data
    use khora_core::ui::types::UiTransform as _; // khora-data, khora-infra
    use khora_core::ui::types::UiVal as _; // khora-data, khora-infra
    use khora_core::ui::Align as _; // khora-sdk
    use khora_core::ui::Align2 as _; // khora-sdk
    use khora_core::ui::App as _; // khora-infra, khora-sdk
    use khora_core::ui::AppContext as _; // khora-infra, khora-sdk
    use khora_core::ui::AppLifecycle as _; // khora-sdk
    use khora_core::ui::CornerRadius as _; // khora-infra, khora-sdk
    use khora_core::ui::EditorOverlay as _; // khora-editor (as khora_sdk::khora_core::…), khora-infra
    use khora_core::ui::FontHandle as _; // khora-sdk
    use khora_core::ui::FontHandle::Owned as _; // khora-infra
    use khora_core::ui::FontHandle::Static as _; // khora-infra
    use khora_core::ui::FontPack as _; // khora-infra, khora-sdk
    use khora_core::ui::LayoutSystem as _; // khora-infra, khora-sdk
    use khora_core::ui::Margin as _; // khora-infra, khora-sdk
    use khora_core::ui::NamedFont as _; // khora-infra, khora-sdk
    use khora_core::ui::OverlayScreenDescriptor as _; // khora-editor (as khora_sdk::khora_core::…), khora-infra
    use khora_core::ui::Stroke as _; // khora-infra, khora-sdk
    use khora_core::ui::UiBuilder as _; // khora-infra, khora-sdk, khora-tool-ui
    use khora_core::ui::UiLayoutView as _; // khora-data
    use khora_core::ui::UiTheme as _; // khora-infra, khora-sdk, khora-tool-ui
    use khora_core::util::stopwatch::Stopwatch as _;
    use khora_core::Backends as _; // khora-sdk
    use khora_core::EngineContext as _; // khora-agents, khora-control
    use khora_core::Resources as _; // khora-sdk
    use khora_core::Runtime as _; // khora-agents, khora-control, khora-data, khora-io, khora-sdk
    use khora_core::Services as _; // khora-sdk
    use khora_core::Stopwatch as _; // khora-infra, khora-lanes
    use khora_core::WorldAccess as _; // khora-agents, khora-control
    use khora_core::WorldAccess::Exclusive as _; // khora-agents // khora-telemetry
}

#[test]
fn associated_items_used_by_other_crates_still_resolve() {
    let _ = khora_core::EngineContext::for_initialisation; // khora-sdk
    let _ = khora_core::Runtime::new; // khora-sdk
    let _ = khora_core::agent::Contention::none; // khora-agents
    let _ = khora_core::agent::ExecutionPhase::OUTPUT; // khora-sdk
    let _ = khora_core::asset::AssetHandle::<khora_core::asset::EmissiveMaterial>::new; // khora-data
    let _ = khora_core::asset::AssetUUID::new_v5; // sandbox (as khora_sdk::khora_core::…)
    let _ = khora_core::interpolation::TransformInterpolation::new; // khora-agents, khora-sdk
    let _ = khora_core::lane::LaneBus::new; // khora-agents, khora-sdk
    let _ = khora_core::lane::LaneError::missing; // khora-lanes
    let _ = khora_core::lane::OutputDeck::new; // khora-agents, khora-sdk
    let _ = khora_core::math::LinearRgba::BLACK; // khora-lanes
    let _ = khora_core::math::LinearRgba::BLUE; // sandbox (as khora_sdk::prelude::math::…)
    let _ = khora_core::math::LinearRgba::CYAN; // sandbox (as khora_sdk::prelude::math::…)
    let _ = khora_core::math::LinearRgba::GREEN; // sandbox (as khora_sdk::prelude::math::…)
    let _ = khora_core::math::LinearRgba::RED; // sandbox (as khora_sdk::prelude::math::…)
    let _ = khora_core::math::LinearRgba::WHITE; // khora-lanes
    let _ = khora_core::math::LinearRgba::YELLOW; // sandbox (as khora_sdk::prelude::math::…)
    let _ = khora_core::math::LinearRgba::new; // khora-agents, khora-editor (as khora_sdk::prelude::math::…), khora-script, khora-sdk, sandbox (as khora_sdk::prelude::math::…)
    let _ = khora_core::math::LinearRgba::rgb; // sandbox (as khora_sdk::prelude::math::…)
    let _ = khora_core::math::Mat4::IDENTITY; // khora-data, khora-lanes
    let _ = khora_core::math::Quat::IDENTITY; // khora-data
    let _ = khora_core::math::Quaternion::IDENTITY; // khora-lanes
    let _ = khora_core::math::Quaternion::new; // khora-script
    let _ = khora_core::math::Vec2::new; // khora-script
    let _ = khora_core::math::Vec3::ONE; // khora-lanes, khora-script
    let _ = khora_core::math::Vec3::ZERO; // khora-data, khora-editor (as khora_sdk::prelude::math::…), khora-lanes
    let _ = khora_core::math::Vec3::new; // khora-editor (as khora_sdk::prelude::math::…), khora-lanes, khora-script
    let _ = khora_core::math::Vec4::new; // khora-script
    let _ = khora_core::platform::InputMap::new; // khora-sdk
    let _ = khora_core::renderer::GpuLight::from_parts; // khora-lanes
    let _ = khora_core::renderer::api::frame::RenderContext::new; // khora-lanes
    let _ = khora_core::renderer::api::resource::BufferUsage::COPY_DST; // khora-lanes
    let _ = khora_core::renderer::api::resource::BufferUsage::STORAGE; // khora-lanes
    let _ = khora_core::renderer::api::resource::BufferUsage::UNIFORM; // khora-lanes
    let _ = khora_core::renderer::light::DirectionalLight::default; // khora-data
    let _ = khora_core::renderer::light::PointLight::default; // khora-data
    let _ = khora_core::renderer::light::SpotLight::default; // khora-data
    let _ = khora_core::script::EventQueue::new; // khora-lanes
    let _: fn(khora_core::ecs::entity::EntityId, String) -> khora_core::script::ScriptEvent =
        khora_core::script::ScriptEvent::new; // khora-lanes
    let _ = khora_core::script::ScriptSnapshot::default; // khora-lanes
    let _: fn(String, String) -> khora_core::telemetry::MetricId =
        khora_core::telemetry::MetricId::new; // khora-control
    let _ = khora_core::time::Time::default; // khora-agents, khora-sdk
    let _ = khora_core::ui::UiTheme::default; // khora-editor (as khora_sdk::khora_core::…)
}
