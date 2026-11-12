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

//! Compile-level guard over `khora_lanes`'s public surface, plus the exported
//! macros and the paths other crates of the workspace use.
//!
//! Every `pub` item reachable through a `pub` path is named here at its full
//! path: each `pub mod`, the `pub use` re-exports, their types, free functions,
//! constants, type aliases, public fields, enum variants (matched
//! exhaustively), inherent `pub` methods (turbofished where generic), and the
//! trait impls. A reorganisation that moves code between files must keep every
//! one of these paths valid, so this module stops compiling the moment one
//! disappears.
//!
//! Re-exports are also checked for *identity*: a `pub use` that starts pointing
//! at a different item with the same name is a compile error (`same_type` for
//! types, `same_item` for functions).
//!
//! The list was generated from `khora_lanes`'s rustdoc JSON, so it is complete for the
//! tree it was written against. Nothing is constructed; the tests only have to
//! type-check.

use std::any::type_name;
use std::marker::PhantomData;

/// Compiles only when both arguments name the same type.
fn same_type<T: ?Sized>(_: PhantomData<T>, _: PhantomData<T>) {}

fn is_clone<T: Clone>() {}
fn is_copy<T: Copy>() {}
fn is_debug<T: std::fmt::Debug>() {}
fn is_default<T: Default>() {}
fn is_deserialize_owned<T: serde::de::DeserializeOwned>() {}
fn is_eq<T: Eq>() {}
fn is_lane_family<T: khora_core::lane::family::LaneFamily>() {}
fn is_typed_lane<T: khora_core::lane::Lane>() {}
fn is_ord<T: Ord>() {}
fn is_partial_eq<T: PartialEq>() {}
fn is_partial_ord<T: PartialOrd>() {}
fn is_serialize<T: serde::Serialize>() {}

// ---------------------------------------------------------------------------
// Every `pub mod`, down to the leaves.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod every_pub_mod {
    use khora_lanes::audio as _;
    use khora_lanes::overlay as _;
    use khora_lanes::physics as _;
    use khora_lanes::render as _;
    use khora_lanes::render::util as _;
    use khora_lanes::render::util::dynamic_uniform_ring_buffer as _;
    use khora_lanes::render::util::uniform_ring_buffer as _;
    use khora_lanes::script_lane as _;
    use khora_lanes::script_lane::persistence as _;
    use khora_lanes::script_lane::report as _;
    use khora_lanes::script_lane::runtime as _;
    use khora_lanes::shadows as _;
    use khora_lanes::skybox as _;
    use khora_lanes::ui as _;
}

// ---------------------------------------------------------------------------
// Public fields (read through a reference) and enum variants (matched
// exhaustively). Nothing is constructed.
// ---------------------------------------------------------------------------

fn forward_plus_gpu_resources_fields(x: &khora_lanes::render::ForwardPlusGpuResources) {
    let _ = (
        &x.light_buffer,
        &x.light_index_buffer,
        &x.light_grid_buffer,
        &x.tile_info_buffer,
        &x.culling_uniforms_buffer,
        &x.shadow_view_projs_buffer,
        &x.camera_layout,
        &x.model_layout,
        &x.material_layout,
        &x.lighting_layout,
        &x.culling_layout,
        &x.camera_ring,
        &x.model_ring,
        &x.culling_bind_group,
        &x.tile_capacity,
        &x.culling_pipeline,
        &x.render_pipeline,
    );
}

/// The Forward+ rings are the ring buffers of `render_lane::util`.
fn forward_plus_gpu_resources_rings(
    x: &khora_lanes::render::ForwardPlusGpuResources,
) -> (
    &Option<khora_lanes::render::util::UniformRingBuffer>,
    &Option<khora_lanes::render::util::DynamicUniformRingBuffer>,
) {
    (&x.camera_ring, &x.model_ring)
}

fn shader_complexity_variants(x: &khora_lanes::render::ShaderComplexity) {
    match x {
        khora_lanes::render::ShaderComplexity::Unlit => {}
        khora_lanes::render::ShaderComplexity::SimpleLit => {}
        khora_lanes::render::ShaderComplexity::FullPBR => {}
    }
}

fn script_run_report_fields(x: &khora_lanes::script_lane::report::ScriptRunReport) {
    let _ = (
        &x.completed,
        &x.deferred,
        &x.faulted,
        &x.unloaded,
        &x.spent,
        &x.raised,
        &x.undelivered,
        &x.state,
    );
}

fn instance_fields(x: &khora_lanes::script_lane::runtime::Instance) {
    let _ = (
        &x.module,
        &x.fields,
        &x.spawned,
        &x.farewelled,
        &x.disabled,
        &x.pending,
        &x.fault,
        &x.loading,
        &x.after_load,
    );
}

fn pending_fields(x: &khora_lanes::script_lane::runtime::Pending) {
    let _ = (&x.machine, &x.remaining, &x.body, &x.fingerprint);
}

fn body_variants(x: &khora_lanes::script_lane::runtime::Body) {
    match x {
        khora_lanes::script_lane::runtime::Body::Sequence => {}
        khora_lanes::script_lane::runtime::Body::Spawn => {}
        khora_lanes::script_lane::runtime::Body::Update => {}
        khora_lanes::script_lane::runtime::Body::Load => {}
        khora_lanes::script_lane::runtime::Body::Timer { index, rearm } => {
            let _ = (index, rearm);
        }
    }
}

fn reload_report_fields(x: &khora_lanes::script_lane::runtime::ReloadReport) {
    let _ = (&x.behavior, &x.kept, &x.added, &x.dropped);
}

#[test]
fn module_audio_paths_still_resolve() {
    let _ = type_name::<khora_lanes::audio::SpatialMixingLane>();
    let _ = type_name::<khora_lanes::audio::AudioFamily>();
    is_default::<khora_lanes::audio::SpatialMixingLane>();
    is_typed_lane::<khora_lanes::audio::SpatialMixingLane>();
    is_lane_family::<khora_lanes::audio::AudioFamily>();
}

#[test]
fn module_physics_lane_paths_still_resolve() {
    let _ = type_name::<khora_lanes::physics::StandardPhysicsLane>();
    let _ = type_name::<khora_lanes::physics::PhysicsFamily>();
    is_default::<khora_lanes::physics::StandardPhysicsLane>();
    is_typed_lane::<khora_lanes::physics::StandardPhysicsLane>();
    is_lane_family::<khora_lanes::physics::PhysicsFamily>();
}

#[test]
fn module_overlay_lane_paths_still_resolve() {
    let _ = type_name::<khora_lanes::overlay::GizmoLane>();
    is_debug::<khora_lanes::overlay::GizmoLane>();
    is_default::<khora_lanes::overlay::GizmoLane>();
    let _ = type_name::<khora_lanes::overlay::GizmoFamily>();
    let _ = type_name::<khora_lanes::overlay::GizmoInputs<'static>>();
    let _ = type_name::<khora_lanes::overlay::GizmoOutputs<'static>>();
    is_typed_lane::<khora_lanes::overlay::GizmoLane>();
    is_lane_family::<khora_lanes::overlay::GizmoFamily>();
    let _ = type_name::<khora_lanes::overlay::GridLane>();
    is_debug::<khora_lanes::overlay::GridLane>();
    is_default::<khora_lanes::overlay::GridLane>();
    let _ = type_name::<khora_lanes::overlay::GridFamily>();
    let _ = type_name::<khora_lanes::overlay::GridInputs<'static>>();
    let _ = type_name::<khora_lanes::overlay::GridOutputs<'static>>();
    is_typed_lane::<khora_lanes::overlay::GridLane>();
    is_lane_family::<khora_lanes::overlay::GridFamily>();
    let _ = type_name::<khora_lanes::overlay::WireframeLane>();
    is_debug::<khora_lanes::overlay::WireframeLane>();
    is_default::<khora_lanes::overlay::WireframeLane>();
    let _ = type_name::<khora_lanes::overlay::WireframeFamily>();
    let _ = type_name::<khora_lanes::overlay::WireframeInputs<'static>>();
    let _ = type_name::<khora_lanes::overlay::WireframeOutputs<'static>>();
    is_typed_lane::<khora_lanes::overlay::WireframeLane>();
    is_lane_family::<khora_lanes::overlay::WireframeFamily>();
}

#[test]
fn module_render_lane_paths_still_resolve() {
    let _ = type_name::<khora_lanes::render::ForwardPlusGpuResources>();
    let _ = forward_plus_gpu_resources_fields as fn(&khora_lanes::render::ForwardPlusGpuResources);
    let _ = khora_lanes::render::ForwardPlusGpuResources::is_initialized;
    is_debug::<khora_lanes::render::ForwardPlusGpuResources>();
    is_default::<khora_lanes::render::ForwardPlusGpuResources>();
    let _ = type_name::<khora_lanes::render::ForwardPlusLane>();
    is_default::<khora_lanes::render::ForwardPlusLane>();
    is_typed_lane::<khora_lanes::render::ForwardPlusLane>();
    let _ = type_name::<khora_lanes::render::LitForwardLane>();
    is_default::<khora_lanes::render::LitForwardLane>();
    is_typed_lane::<khora_lanes::render::LitForwardLane>();
    let _ = type_name::<khora_lanes::render::ShaderComplexity>();
    let _ = shader_complexity_variants as fn(&khora_lanes::render::ShaderComplexity);
    let _ = khora_lanes::render::ShaderComplexity::cost_multiplier;
    let _ = khora_lanes::render::ShaderComplexity::name;
    is_debug::<khora_lanes::render::ShaderComplexity>();
    is_clone::<khora_lanes::render::ShaderComplexity>();
    is_copy::<khora_lanes::render::ShaderComplexity>();
    is_partial_eq::<khora_lanes::render::ShaderComplexity>();
    is_eq::<khora_lanes::render::ShaderComplexity>();
    is_partial_ord::<khora_lanes::render::ShaderComplexity>();
    is_ord::<khora_lanes::render::ShaderComplexity>();
    is_default::<khora_lanes::render::ShaderComplexity>();
    let _ = type_name::<khora_lanes::render::SimpleUnlitLane>();
    is_default::<khora_lanes::render::SimpleUnlitLane>();
    let _ = type_name::<khora_lanes::render::RenderFamily>();
    is_lane_family::<khora_lanes::render::RenderFamily>();
    let _ = type_name::<khora_lanes::render::RenderOutputs<'static>>();
    is_typed_lane::<khora_lanes::render::SimpleUnlitLane>();
}

#[test]
fn module_render_lane_util_paths_still_resolve() {
    const _: u32 = khora_lanes::render::util::dynamic_uniform_ring_buffer::DEFAULT_MAX_ELEMENTS;
    const _: u32 = khora_lanes::render::util::dynamic_uniform_ring_buffer::MIN_UNIFORM_ALIGNMENT;
    let _ = type_name::<
        khora_lanes::render::util::dynamic_uniform_ring_buffer::DynamicUniformRingBuffer,
    >();
    let _ = type_name::<khora_lanes::render::util::DynamicUniformRingBuffer>();
    same_type(
        PhantomData::<khora_lanes::render::util::DynamicUniformRingBuffer>,
        PhantomData::<
            khora_lanes::render::util::dynamic_uniform_ring_buffer::DynamicUniformRingBuffer,
        >,
    );
    let _ = khora_lanes::render::util::dynamic_uniform_ring_buffer::DynamicUniformRingBuffer::new;
    let _ =
        khora_lanes::render::util::dynamic_uniform_ring_buffer::DynamicUniformRingBuffer::advance;
    let _ = khora_lanes::render::util::dynamic_uniform_ring_buffer::DynamicUniformRingBuffer::push;
    let _ = khora_lanes::render::util::dynamic_uniform_ring_buffer::DynamicUniformRingBuffer::current_bind_group;
    let _ = khora_lanes::render::util::dynamic_uniform_ring_buffer::DynamicUniformRingBuffer::current_slot_index;
    let _ =
        khora_lanes::render::util::dynamic_uniform_ring_buffer::DynamicUniformRingBuffer::destroy;
    is_debug::<khora_lanes::render::util::dynamic_uniform_ring_buffer::DynamicUniformRingBuffer>();
    let _ = type_name::<khora_lanes::render::util::uniform_ring_buffer::UniformRingBuffer>();
    let _ = type_name::<khora_lanes::render::util::UniformRingBuffer>();
    same_type(
        PhantomData::<khora_lanes::render::util::UniformRingBuffer>,
        PhantomData::<khora_lanes::render::util::uniform_ring_buffer::UniformRingBuffer>,
    );
    let _ = khora_lanes::render::util::uniform_ring_buffer::UniformRingBuffer::new;
    let _ = khora_lanes::render::util::uniform_ring_buffer::UniformRingBuffer::advance;
    let _ = khora_lanes::render::util::uniform_ring_buffer::UniformRingBuffer::write;
    let _ = khora_lanes::render::util::uniform_ring_buffer::UniformRingBuffer::current_bind_group;
    let _ = khora_lanes::render::util::uniform_ring_buffer::UniformRingBuffer::current_buffer;
    let _ = khora_lanes::render::util::uniform_ring_buffer::UniformRingBuffer::current_slot_index;
    let _ = khora_lanes::render::util::uniform_ring_buffer::UniformRingBuffer::slot_count;
    let _ = khora_lanes::render::util::uniform_ring_buffer::UniformRingBuffer::data_size;
    let _ = khora_lanes::render::util::uniform_ring_buffer::UniformRingBuffer::destroy;
    is_debug::<khora_lanes::render::util::uniform_ring_buffer::UniformRingBuffer>();
    let _ = forward_plus_gpu_resources_rings;
}

#[test]
fn module_shadow_lane_paths_still_resolve() {
    let _ = type_name::<khora_lanes::shadows::ShadowsLane>();
    let _ = type_name::<khora_lanes::shadows::ShadowsFamily>();
    let _ = type_name::<khora_lanes::shadows::ShadowsInputs<'static>>();
    let _ = type_name::<khora_lanes::shadows::ShadowsOutputs<'static>>();
    is_default::<khora_lanes::shadows::ShadowsLane>();
    is_typed_lane::<khora_lanes::shadows::ShadowsLane>();
    is_lane_family::<khora_lanes::shadows::ShadowsFamily>();
}

#[test]
fn module_skybox_lane_paths_still_resolve() {
    let _ = type_name::<khora_lanes::skybox::SkyboxLane>();
    is_debug::<khora_lanes::skybox::SkyboxLane>();
    is_default::<khora_lanes::skybox::SkyboxLane>();
    let _ = type_name::<khora_lanes::skybox::SkyboxFamily>();
    let _ = type_name::<khora_lanes::skybox::SkyboxInputs<'static>>();
    let _ = type_name::<khora_lanes::skybox::SkyboxOutputs<'static>>();
    is_typed_lane::<khora_lanes::skybox::SkyboxLane>();
    is_lane_family::<khora_lanes::skybox::SkyboxFamily>();
}

#[test]
fn module_ui_lane_paths_still_resolve() {
    let _ = type_name::<khora_lanes::ui::UiLane>();
    is_debug::<khora_lanes::ui::UiLane>();
    is_default::<khora_lanes::ui::UiLane>();
    let _ = type_name::<khora_lanes::ui::UiFamily>();
    let _ = type_name::<khora_lanes::ui::UiInputs<'static>>();
    let _ = type_name::<khora_lanes::ui::UiOutputs<'static>>();
    is_typed_lane::<khora_lanes::ui::UiLane>();
    is_lane_family::<khora_lanes::ui::UiFamily>();
}

#[test]
fn module_script_lane_paths_still_resolve() {
    let _ = type_name::<khora_lanes::script_lane::BudgetedScriptLane>();
    let _ = khora_lanes::script_lane::BudgetedScriptLane::new;
    is_debug::<khora_lanes::script_lane::BudgetedScriptLane>();
    is_default::<khora_lanes::script_lane::BudgetedScriptLane>();
    is_typed_lane::<khora_lanes::script_lane::BudgetedScriptLane>();
    let _ = type_name::<khora_lanes::script_lane::ScriptFamily>();
    let _ = type_name::<khora_lanes::script_lane::ScriptInputs<'static>>();
    let _ = type_name::<khora_lanes::script_lane::ScriptOutputs>();
    is_lane_family::<khora_lanes::script_lane::ScriptFamily>();
    let _ = khora_lanes::script_lane::resumption::resume;
    let _ = khora_lanes::script_lane::resumption::carry;
    let _ = khora_lanes::script_lane::persistence::snapshot_from_store;
    let _ = khora_lanes::script_lane::persistence::store_from_snapshot;
    let _ = khora_lanes::script_lane::persistence::suspend;
    let _ = type_name::<khora_lanes::script_lane::report::ScriptRunReport>();
    let _ = type_name::<khora_lanes::script_lane::ScriptRunReport>();
    same_type(
        PhantomData::<khora_lanes::script_lane::ScriptRunReport>,
        PhantomData::<khora_lanes::script_lane::report::ScriptRunReport>,
    );
    let _ = script_run_report_fields as fn(&khora_lanes::script_lane::report::ScriptRunReport);
    is_debug::<khora_lanes::script_lane::report::ScriptRunReport>();
    is_clone::<khora_lanes::script_lane::report::ScriptRunReport>();
    is_default::<khora_lanes::script_lane::report::ScriptRunReport>();
    is_partial_eq::<khora_lanes::script_lane::report::ScriptRunReport>();
    let _ = khora_lanes::script_lane::run_behaviors;
    let _ = khora_lanes::script_lane::runtime::INITIAL_RATE;
    let _ = type_name::<khora_lanes::script_lane::runtime::Instance>();
    let _ = type_name::<khora_lanes::script_lane::Instance>();
    same_type(
        PhantomData::<khora_lanes::script_lane::Instance>,
        PhantomData::<khora_lanes::script_lane::runtime::Instance>,
    );
    let _ = instance_fields as fn(&khora_lanes::script_lane::runtime::Instance);
    is_debug::<khora_lanes::script_lane::runtime::Instance>();
    is_default::<khora_lanes::script_lane::runtime::Instance>();
    let _ = type_name::<khora_lanes::script_lane::runtime::Pending>();
    let _ = type_name::<khora_lanes::script_lane::Pending>();
    same_type(
        PhantomData::<khora_lanes::script_lane::Pending>,
        PhantomData::<khora_lanes::script_lane::runtime::Pending>,
    );
    let _ = pending_fields as fn(&khora_lanes::script_lane::runtime::Pending);
    is_debug::<khora_lanes::script_lane::runtime::Pending>();
    is_clone::<khora_lanes::script_lane::runtime::Pending>();
    is_partial_eq::<khora_lanes::script_lane::runtime::Pending>();
    is_serialize::<khora_lanes::script_lane::runtime::Pending>();
    is_deserialize_owned::<khora_lanes::script_lane::runtime::Pending>();
    let _ = type_name::<khora_lanes::script_lane::runtime::Body>();
    let _ = type_name::<khora_lanes::script_lane::Body>();
    same_type(
        PhantomData::<khora_lanes::script_lane::Body>,
        PhantomData::<khora_lanes::script_lane::runtime::Body>,
    );
    let _ = body_variants as fn(&khora_lanes::script_lane::runtime::Body);
    is_debug::<khora_lanes::script_lane::runtime::Body>();
    is_clone::<khora_lanes::script_lane::runtime::Body>();
    is_partial_eq::<khora_lanes::script_lane::runtime::Body>();
    is_serialize::<khora_lanes::script_lane::runtime::Body>();
    is_deserialize_owned::<khora_lanes::script_lane::runtime::Body>();
    let _ = type_name::<khora_lanes::script_lane::runtime::ReloadReport>();
    let _ = type_name::<khora_lanes::script_lane::ReloadReport>();
    same_type(
        PhantomData::<khora_lanes::script_lane::ReloadReport>,
        PhantomData::<khora_lanes::script_lane::runtime::ReloadReport>,
    );
    let _ = reload_report_fields as fn(&khora_lanes::script_lane::runtime::ReloadReport);
    let _ = khora_lanes::script_lane::runtime::ReloadReport::lost_anything;
    is_debug::<khora_lanes::script_lane::runtime::ReloadReport>();
    is_clone::<khora_lanes::script_lane::runtime::ReloadReport>();
    is_partial_eq::<khora_lanes::script_lane::runtime::ReloadReport>();
    is_eq::<khora_lanes::script_lane::runtime::ReloadReport>();
    let _ = type_name::<khora_lanes::script_lane::runtime::ScriptRuntime>();
    let _ = type_name::<khora_lanes::script_lane::ScriptRuntime>();
    same_type(
        PhantomData::<khora_lanes::script_lane::ScriptRuntime>,
        PhantomData::<khora_lanes::script_lane::runtime::ScriptRuntime>,
    );
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::new;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::take_pending;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::set_pending;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::pending_len;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::set_last_report;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::last_report;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::report_unloaded;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::rate;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::observe;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::overhead;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::fuel_for;
    let _: fn(
        &mut khora_lanes::script_lane::runtime::ScriptRuntime,
        String,
        khora_script::Program,
    ) = khora_lanes::script_lane::runtime::ScriptRuntime::add_program;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::reload;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::program;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::module_count;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::instance;
    #[allow(clippy::type_complexity)]
    let _: fn(
        &khora_lanes::script_lane::runtime::ScriptRuntime,
        fn(khora_core::ecs::entity::EntityId) -> bool,
    ) -> Vec<(khora_core::ecs::entity::EntityId, String, String)> =
        khora_lanes::script_lane::runtime::ScriptRuntime::departed;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::peek;
    let _: fn(
        &mut khora_lanes::script_lane::runtime::ScriptRuntime,
        fn(khora_core::ecs::entity::EntityId) -> bool,
    ) = khora_lanes::script_lane::runtime::ScriptRuntime::retain_live;
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::instance_count;
    is_debug::<khora_lanes::script_lane::runtime::ScriptRuntime>();
    is_default::<khora_lanes::script_lane::runtime::ScriptRuntime>();
}

// ---------------------------------------------------------------------------
// The exported macro, expanded at its crate-root path. It names `log::error!`
// in its body, so it also breaks if a move changes what it expands to.
// ---------------------------------------------------------------------------

fn lock_or_log_with_default(m: &std::sync::Mutex<u32>) -> u32 {
    let guard = khora_lanes::lock_or_log!(m.lock(), "public_paths::with_default", 0);
    *guard
}

fn lock_or_log_without_default(m: &std::sync::Mutex<u32>, out: &mut u32) {
    let guard = khora_lanes::lock_or_log!(m.lock(), "public_paths::without_default");
    *out = *guard;
}

#[test]
fn exported_macros_still_expand() {
    let m = std::sync::Mutex::new(7);
    assert_eq!(lock_or_log_with_default(&m), 7);
    let mut out = 0;
    lock_or_log_without_default(&m, &mut out);
    assert_eq!(out, 7);
}

// ---------------------------------------------------------------------------
// Paths other crates of the workspace spell today (`crates/`, `examples/`,
// `xtask/`; brace imports expanded; `khora_sdk::khora_lanes::…`
// mapped onto the `khora_lanes` path the SDK re-exports). The trailing comment
// names the users. Items, modules and enum variants are imported;
// associated items are named in the test below.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod paths_used_by_other_crates {
    use khora_lanes::audio::SpatialMixingLane as _; // khora-agents
    use khora_lanes::overlay::GizmoFamily as _; // khora-agents
    use khora_lanes::overlay::GizmoLane as _; // khora-agents
    use khora_lanes::overlay::GridFamily as _; // khora-agents
    use khora_lanes::overlay::GridLane as _; // khora-agents
    use khora_lanes::overlay::WireframeFamily as _; // khora-agents
    use khora_lanes::overlay::WireframeLane as _; // khora-agents
    use khora_lanes::physics::StandardPhysicsLane as _; // khora-agents
    use khora_lanes::render::ForwardPlusLane as _; // khora-agents
    use khora_lanes::render::LitForwardLane as _; // khora-agents
    use khora_lanes::render::RenderFamily as _; // khora-agents
    use khora_lanes::render::SimpleUnlitLane as _; // khora-agents
    use khora_lanes::script_lane::runtime::INITIAL_RATE as _; // khora-agents
    use khora_lanes::script_lane::BudgetedScriptLane as _; // khora-agents
    use khora_lanes::script_lane::ScriptFamily as _; // khora-agents
    use khora_lanes::script_lane::ScriptRunReport as _; // khora-agents
    use khora_lanes::script_lane::ScriptRuntime as _; // khora-agents, khora-sdk
    use khora_lanes::shadows::ShadowsFamily as _; // khora-agents
    use khora_lanes::shadows::ShadowsLane as _; // khora-agents
    use khora_lanes::skybox::SkyboxLane as _; // khora-agents
    use khora_lanes::ui::UiFamily as _; // khora-agents
    use khora_lanes::ui::UiLane as _; // khora-agents
}

#[test]
fn associated_items_used_by_other_crates_still_resolve() {
    let _ = khora_lanes::overlay::GizmoLane::default; // khora-agents
    let _ = khora_lanes::overlay::GridLane::default; // khora-agents
    let _ = khora_lanes::overlay::WireframeLane::default; // khora-agents
    let _ = khora_lanes::script_lane::ScriptRuntime::new; // khora-agents, khora-sdk
    let _ = khora_lanes::skybox::SkyboxLane::default; // khora-agents
    let _ = khora_lanes::ui::UiLane::default; // khora-agents
}
