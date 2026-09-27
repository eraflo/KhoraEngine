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
fn is_lane<T: khora_core::lane::Lane>() {}
fn is_ord<T: Ord>() {}
fn is_partial_eq<T: PartialEq>() {}
fn is_partial_ord<T: PartialOrd>() {}
fn is_serialize<T: serde::Serialize>() {}

// ---------------------------------------------------------------------------
// Every `pub mod`, down to the leaves.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod every_pub_mod {
    use khora_lanes::audio_lane as _;
    use khora_lanes::physics_lane as _;
    use khora_lanes::render_lane as _;
    use khora_lanes::render_lane::shaders as _;
    use khora_lanes::render_lane::shadows_lane as _;
    use khora_lanes::render_lane::shadows_lane::algo as _;
    use khora_lanes::render_lane::shadows_lane::algo::atlas_2d as _;
    use khora_lanes::render_lane::shadows_lane::algo::atlas_cube as _;
    use khora_lanes::render_lane::shadows_lane::algo::bindings as _;
    use khora_lanes::render_lane::shadows_lane::algo::pass as _;
    use khora_lanes::render_lane::shadows_lane::algo::state as _;
    use khora_lanes::render_lane::util as _;
    use khora_lanes::script_lane as _;
    use khora_lanes::script_lane::persistence as _;
    use khora_lanes::script_lane::report as _;
    use khora_lanes::script_lane::runtime as _;
}

// ---------------------------------------------------------------------------
// Public fields (read through a reference) and enum variants (matched
// exhaustively). Nothing is constructed.
// ---------------------------------------------------------------------------

fn forward_plus_gpu_resources_fields(x: &khora_lanes::render_lane::ForwardPlusGpuResources) {
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
        &x.culling_pipeline,
        &x.render_pipeline,
    );
}

fn forward_plus_lane_fields(x: &khora_lanes::render_lane::ForwardPlusLane) {
    let _ = (&x.tile_config, &x.shader_complexity, &x.gpu_resources);
}

fn lit_forward_lane_fields(x: &khora_lanes::render_lane::LitForwardLane) {
    let _ = (
        &x.shader_complexity,
        &x.max_directional_lights,
        &x.max_point_lights,
        &x.max_spot_lights,
    );
}

fn shader_complexity_variants(x: &khora_lanes::render_lane::ShaderComplexity) {
    match x {
        khora_lanes::render_lane::ShaderComplexity::Unlit => {}
        khora_lanes::render_lane::ShaderComplexity::SimpleLit => {}
        khora_lanes::render_lane::ShaderComplexity::FullPBR => {}
    }
}

fn atlas2_d_fields(x: &khora_lanes::render_lane::shadows_lane::algo::atlas_2d::Atlas2D) {
    let _ = (&x.texture, &x.view);
}

fn atlas_cube_fields(x: &khora_lanes::render_lane::shadows_lane::algo::atlas_cube::AtlasCube) {
    let _ = (&x.texture, &x.view, &x.face_views);
}

fn attachment_pass_fields(x: &khora_lanes::render_lane::shadows_lane::algo::pass::AttachmentPass) {
    let _ = (
        &x.target_view,
        &x.base_array_layer,
        &x.camera_bg,
        &x.camera_offset,
        &x.draw_cmds,
    );
}

fn shadow_draw_cmd_fields(x: &khora_lanes::render_lane::shadows_lane::algo::pass::ShadowDrawCmd) {
    let _ = (
        &x.model_bg,
        &x.model_offset,
        &x.vertex_buffer,
        &x.index_buffer,
        &x.index_count,
        &x.index_format,
    );
}

fn shadows_lane_state_fields(
    x: &khora_lanes::render_lane::shadows_lane::algo::state::ShadowsLaneState,
) {
    let _ = (
        &x.pipeline,
        &x.camera_layout,
        &x.model_layout,
        &x.atlas_2d,
        &x.atlas_cube,
        &x.shadow_sampler,
        &x.shadow_results,
        &x.camera_ring,
        &x.model_ring,
    );
}

fn fuel_fields(x: &khora_lanes::script_lane::Fuel) {
    let _ = (&x.0,);
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
        &x.initialised,
        &x.spawned,
        &x.farewelled,
        &x.disabled,
        &x.pending,
        &x.carried,
    );
}

fn pending_fields(x: &khora_lanes::script_lane::runtime::Pending) {
    let _ = (&x.machine, &x.remaining);
}

fn reload_report_fields(x: &khora_lanes::script_lane::runtime::ReloadReport) {
    let _ = (&x.behavior, &x.kept, &x.added, &x.dropped);
}

#[test]
fn module_audio_lane_paths_still_resolve() {
    let _ = type_name::<khora_lanes::audio_lane::SpatialMixingLane>();
    let _ = khora_lanes::audio_lane::SpatialMixingLane::new;
    let _ = khora_lanes::audio_lane::SpatialMixingLane::mix;
    is_default::<khora_lanes::audio_lane::SpatialMixingLane>();
    is_lane::<khora_lanes::audio_lane::SpatialMixingLane>();
}

#[test]
fn module_physics_lane_paths_still_resolve() {
    let _ = type_name::<khora_lanes::physics_lane::StandardPhysicsLane>();
    let _ = khora_lanes::physics_lane::StandardPhysicsLane::new;
    is_debug::<khora_lanes::physics_lane::StandardPhysicsLane>();
    is_default::<khora_lanes::physics_lane::StandardPhysicsLane>();
    is_lane::<khora_lanes::physics_lane::StandardPhysicsLane>();
}

#[test]
fn module_render_lane_paths_still_resolve() {
    let _ = type_name::<khora_lanes::render_lane::ForwardPlusGpuResources>();
    let _ =
        forward_plus_gpu_resources_fields as fn(&khora_lanes::render_lane::ForwardPlusGpuResources);
    let _ = khora_lanes::render_lane::ForwardPlusGpuResources::is_initialized;
    is_debug::<khora_lanes::render_lane::ForwardPlusGpuResources>();
    is_default::<khora_lanes::render_lane::ForwardPlusGpuResources>();
    let _ = type_name::<khora_lanes::render_lane::ForwardPlusLane>();
    let _ = forward_plus_lane_fields as fn(&khora_lanes::render_lane::ForwardPlusLane);
    let _ = khora_lanes::render_lane::ForwardPlusLane::new;
    let _ = khora_lanes::render_lane::ForwardPlusLane::with_config;
    let _ = khora_lanes::render_lane::ForwardPlusLane::with_complexity;
    let _ = khora_lanes::render_lane::ForwardPlusLane::set_screen_size;
    let _ = khora_lanes::render_lane::ForwardPlusLane::tile_count;
    let _ = khora_lanes::render_lane::ForwardPlusLane::total_tiles;
    let _ = khora_lanes::render_lane::ForwardPlusLane::effective_light_count;
    let _ = khora_lanes::render_lane::ForwardPlusLane::get_pipeline_for_material;
    is_default::<khora_lanes::render_lane::ForwardPlusLane>();
    is_lane::<khora_lanes::render_lane::ForwardPlusLane>();
    let _ = type_name::<khora_lanes::render_lane::GizmoLane>();
    is_debug::<khora_lanes::render_lane::GizmoLane>();
    is_default::<khora_lanes::render_lane::GizmoLane>();
    is_lane::<khora_lanes::render_lane::GizmoLane>();
    let _ = type_name::<khora_lanes::render_lane::GridLane>();
    is_debug::<khora_lanes::render_lane::GridLane>();
    is_default::<khora_lanes::render_lane::GridLane>();
    is_lane::<khora_lanes::render_lane::GridLane>();
    let _ = type_name::<khora_lanes::render_lane::LitForwardLane>();
    let _ = lit_forward_lane_fields as fn(&khora_lanes::render_lane::LitForwardLane);
    let _ = khora_lanes::render_lane::LitForwardLane::new;
    let _ = khora_lanes::render_lane::LitForwardLane::with_complexity;
    let _ = khora_lanes::render_lane::LitForwardLane::effective_light_counts;
    let _ = khora_lanes::render_lane::LitForwardLane::get_pipeline_for_material;
    is_default::<khora_lanes::render_lane::LitForwardLane>();
    is_lane::<khora_lanes::render_lane::LitForwardLane>();
    let _ = type_name::<khora_lanes::render_lane::shadows_lane::LowResShadowsLane>();
    let _ = type_name::<khora_lanes::render_lane::LowResShadowsLane>();
    same_type(
        PhantomData::<khora_lanes::render_lane::shadows_lane::LowResShadowsLane>,
        PhantomData::<khora_lanes::render_lane::LowResShadowsLane>,
    );
    let _ = khora_lanes::render_lane::LowResShadowsLane::ATLAS_2D_RESOLUTION;
    let _ = khora_lanes::render_lane::LowResShadowsLane::ATLAS_2D_MAX_LIGHTS;
    let _ = khora_lanes::render_lane::LowResShadowsLane::CUBE_FACE_RESOLUTION;
    let _ = khora_lanes::render_lane::LowResShadowsLane::CUBE_MAX_LIGHTS;
    let _ = khora_lanes::render_lane::LowResShadowsLane::new;
    is_default::<khora_lanes::render_lane::LowResShadowsLane>();
    is_lane::<khora_lanes::render_lane::LowResShadowsLane>();
    let _ = type_name::<khora_lanes::render_lane::shadows_lane::MediumShadowsLane>();
    let _ = type_name::<khora_lanes::render_lane::MediumShadowsLane>();
    same_type(
        PhantomData::<khora_lanes::render_lane::shadows_lane::MediumShadowsLane>,
        PhantomData::<khora_lanes::render_lane::MediumShadowsLane>,
    );
    let _ = khora_lanes::render_lane::MediumShadowsLane::ATLAS_2D_RESOLUTION;
    let _ = khora_lanes::render_lane::MediumShadowsLane::ATLAS_2D_MAX_LIGHTS;
    let _ = khora_lanes::render_lane::MediumShadowsLane::CUBE_FACE_RESOLUTION;
    let _ = khora_lanes::render_lane::MediumShadowsLane::CUBE_MAX_LIGHTS;
    let _ = khora_lanes::render_lane::MediumShadowsLane::new;
    is_default::<khora_lanes::render_lane::MediumShadowsLane>();
    is_lane::<khora_lanes::render_lane::MediumShadowsLane>();
    let _ = type_name::<khora_lanes::render_lane::ShaderComplexity>();
    let _ = shader_complexity_variants as fn(&khora_lanes::render_lane::ShaderComplexity);
    let _ = khora_lanes::render_lane::ShaderComplexity::cost_multiplier;
    let _ = khora_lanes::render_lane::ShaderComplexity::name;
    is_debug::<khora_lanes::render_lane::ShaderComplexity>();
    is_clone::<khora_lanes::render_lane::ShaderComplexity>();
    is_copy::<khora_lanes::render_lane::ShaderComplexity>();
    is_partial_eq::<khora_lanes::render_lane::ShaderComplexity>();
    is_eq::<khora_lanes::render_lane::ShaderComplexity>();
    is_partial_ord::<khora_lanes::render_lane::ShaderComplexity>();
    is_ord::<khora_lanes::render_lane::ShaderComplexity>();
    is_default::<khora_lanes::render_lane::ShaderComplexity>();
    let _ = type_name::<khora_lanes::render_lane::SharedGizmoFrame>();
    let _ = type_name::<khora_lanes::render_lane::SharedGridConfig>();
    let _ = type_name::<khora_lanes::render_lane::SharedWireframeConfig>();
    let _ = type_name::<khora_lanes::render_lane::SimpleUnlitLane>();
    let _ = khora_lanes::render_lane::SimpleUnlitLane::new;
    let _ = khora_lanes::render_lane::SimpleUnlitLane::get_pipeline_for_material;
    is_default::<khora_lanes::render_lane::SimpleUnlitLane>();
    is_lane::<khora_lanes::render_lane::SimpleUnlitLane>();
    let _ = type_name::<khora_lanes::render_lane::SkyboxLane>();
    is_debug::<khora_lanes::render_lane::SkyboxLane>();
    is_default::<khora_lanes::render_lane::SkyboxLane>();
    is_lane::<khora_lanes::render_lane::SkyboxLane>();
    let _ = type_name::<khora_lanes::render_lane::StandardPbrLane>();
    is_default::<khora_lanes::render_lane::StandardPbrLane>();
    is_lane::<khora_lanes::render_lane::StandardPbrLane>();
    let _ = type_name::<khora_lanes::render_lane::shadows_lane::StandardShadowsLane>();
    let _ = type_name::<khora_lanes::render_lane::StandardShadowsLane>();
    same_type(
        PhantomData::<khora_lanes::render_lane::shadows_lane::StandardShadowsLane>,
        PhantomData::<khora_lanes::render_lane::StandardShadowsLane>,
    );
    let _ = khora_lanes::render_lane::StandardShadowsLane::ATLAS_2D_RESOLUTION;
    let _ = khora_lanes::render_lane::StandardShadowsLane::ATLAS_2D_MAX_LIGHTS;
    let _ = khora_lanes::render_lane::StandardShadowsLane::CUBE_FACE_RESOLUTION;
    let _ = khora_lanes::render_lane::StandardShadowsLane::CUBE_MAX_LIGHTS;
    let _ = khora_lanes::render_lane::StandardShadowsLane::new;
    is_default::<khora_lanes::render_lane::StandardShadowsLane>();
    is_lane::<khora_lanes::render_lane::StandardShadowsLane>();
    let _ = type_name::<khora_lanes::render_lane::UiRenderLane>();
    let _ = khora_lanes::render_lane::UiRenderLane::new;
    is_default::<khora_lanes::render_lane::UiRenderLane>();
    is_lane::<khora_lanes::render_lane::UiRenderLane>();
    let _ = type_name::<khora_lanes::render_lane::WireframeLane>();
    is_debug::<khora_lanes::render_lane::WireframeLane>();
    is_default::<khora_lanes::render_lane::WireframeLane>();
    is_lane::<khora_lanes::render_lane::WireframeLane>();
    let _ = khora_lanes::render_lane::shaders::EGUI_WGSL;
    let _ = khora_lanes::render_lane::shaders::TEXT_WGSL;
    let _ = khora_lanes::render_lane::shadows_lane::LOW_RES_STRATEGY_NAME;
    let _ = khora_lanes::render_lane::shadows_lane::MEDIUM_STRATEGY_NAME;
    let _ = khora_lanes::render_lane::shadows_lane::STANDARD_STRATEGY_NAME;
    let _ = type_name::<khora_lanes::render_lane::shadows_lane::algo::atlas_2d::Atlas2D>();
    let _ = atlas2_d_fields as fn(&khora_lanes::render_lane::shadows_lane::algo::atlas_2d::Atlas2D);
    let _ = khora_lanes::render_lane::shadows_lane::algo::atlas_2d::Atlas2D::create;
    let _ = khora_lanes::render_lane::shadows_lane::algo::atlas_2d::Atlas2D::view_id;
    let _ = khora_lanes::render_lane::shadows_lane::algo::atlas_2d::Atlas2D::destroy;
    is_default::<khora_lanes::render_lane::shadows_lane::algo::atlas_2d::Atlas2D>();
    let _ = khora_lanes::render_lane::shadows_lane::algo::atlas_2d::collect_pass;
    let _ = type_name::<khora_lanes::render_lane::shadows_lane::algo::atlas_cube::AtlasCube>();
    let _ = atlas_cube_fields
        as fn(&khora_lanes::render_lane::shadows_lane::algo::atlas_cube::AtlasCube);
    let _ = khora_lanes::render_lane::shadows_lane::algo::atlas_cube::AtlasCube::create;
    let _ = khora_lanes::render_lane::shadows_lane::algo::atlas_cube::AtlasCube::view_id;
    let _ =
        khora_lanes::render_lane::shadows_lane::algo::atlas_cube::AtlasCube::face_views_snapshot;
    let _ = khora_lanes::render_lane::shadows_lane::algo::atlas_cube::AtlasCube::destroy;
    is_default::<khora_lanes::render_lane::shadows_lane::algo::atlas_cube::AtlasCube>();
    let _ = khora_lanes::render_lane::shadows_lane::algo::atlas_cube::collect_passes;
    let _ = khora_lanes::render_lane::shadows_lane::algo::bindings::build_bindings;
    let _ = type_name::<khora_lanes::render_lane::shadows_lane::algo::pass::AttachmentPass>();
    let _ = attachment_pass_fields
        as fn(&khora_lanes::render_lane::shadows_lane::algo::pass::AttachmentPass);
    let _ = type_name::<khora_lanes::render_lane::shadows_lane::algo::pass::ShadowDrawCmd>();
    let _ = shadow_draw_cmd_fields
        as fn(&khora_lanes::render_lane::shadows_lane::algo::pass::ShadowDrawCmd);
    is_debug::<khora_lanes::render_lane::shadows_lane::algo::pass::ShadowDrawCmd>();
    is_clone::<khora_lanes::render_lane::shadows_lane::algo::pass::ShadowDrawCmd>();
    is_copy::<khora_lanes::render_lane::shadows_lane::algo::pass::ShadowDrawCmd>();
    let _ = khora_lanes::render_lane::shadows_lane::algo::pass::build_draw_cmds;
    let _ = khora_lanes::render_lane::shadows_lane::algo::pass::record_depth_pass;
    let _ = type_name::<khora_lanes::render_lane::shadows_lane::algo::state::ShadowsLaneState>();
    let _ = type_name::<khora_lanes::render_lane::shadows_lane::algo::ShadowsLaneState>();
    same_type(
        PhantomData::<khora_lanes::render_lane::shadows_lane::algo::ShadowsLaneState>,
        PhantomData::<khora_lanes::render_lane::shadows_lane::algo::state::ShadowsLaneState>,
    );
    let _ = shadows_lane_state_fields
        as fn(&khora_lanes::render_lane::shadows_lane::algo::state::ShadowsLaneState);
    let _ = khora_lanes::render_lane::shadows_lane::algo::state::ShadowsLaneState::init_gpu;
    let _ = khora_lanes::render_lane::shadows_lane::algo::state::ShadowsLaneState::render;
    let _ = khora_lanes::render_lane::shadows_lane::algo::state::ShadowsLaneState::shadow_bindings;
    let _ = khora_lanes::render_lane::shadows_lane::algo::state::ShadowsLaneState::shutdown;
    is_default::<khora_lanes::render_lane::shadows_lane::algo::state::ShadowsLaneState>();
}

#[test]
fn module_script_lane_paths_still_resolve() {
    let _ = type_name::<khora_lanes::script_lane::BudgetedScriptLane>();
    let _ = khora_lanes::script_lane::BudgetedScriptLane::new;
    is_debug::<khora_lanes::script_lane::BudgetedScriptLane>();
    is_default::<khora_lanes::script_lane::BudgetedScriptLane>();
    is_lane::<khora_lanes::script_lane::BudgetedScriptLane>();
    let _ = type_name::<khora_lanes::script_lane::Fuel>();
    let _ = fuel_fields as fn(&khora_lanes::script_lane::Fuel);
    is_debug::<khora_lanes::script_lane::Fuel>();
    is_clone::<khora_lanes::script_lane::Fuel>();
    is_copy::<khora_lanes::script_lane::Fuel>();
    is_partial_eq::<khora_lanes::script_lane::Fuel>();
    is_eq::<khora_lanes::script_lane::Fuel>();
    let _ = khora_lanes::script_lane::persistence::resume;
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
    let _ = khora_lanes::script_lane::runtime::ScriptRuntime::observe_rate;
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
    let _ = khora_lanes::script_lane::runtime::restore_carried;
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
// Paths other crates of the workspace spell today (`crates/`, `hub/`,
// `examples/`, `xtask/`; brace imports expanded; `khora_sdk::khora_lanes::…`
// mapped onto the `khora_lanes` path the SDK re-exports). The trailing comment
// names the users. Items, modules and enum variants are imported;
// associated items are named in the test below.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod paths_used_by_other_crates {
    use khora_lanes::audio_lane::SpatialMixingLane as _; // khora-agents
    use khora_lanes::physics_lane::StandardPhysicsLane as _; // khora-agents
    use khora_lanes::render_lane::shaders::EGUI_WGSL as _; // khora-editor (as khora_sdk::khora_lanes::…)
    use khora_lanes::render_lane::shaders::TEXT_WGSL as _; // khora-sdk
    use khora_lanes::render_lane::shadows_lane::LOW_RES_STRATEGY_NAME as _; // khora-agents
    use khora_lanes::render_lane::shadows_lane::MEDIUM_STRATEGY_NAME as _; // khora-agents
    use khora_lanes::render_lane::shadows_lane::STANDARD_STRATEGY_NAME as _; // khora-agents
    use khora_lanes::render_lane::ForwardPlusLane as _; // khora-agents
    use khora_lanes::render_lane::LitForwardLane as _; // khora-agents
    use khora_lanes::render_lane::LowResShadowsLane as _; // khora-agents
    use khora_lanes::render_lane::MediumShadowsLane as _; // khora-agents
    use khora_lanes::render_lane::SharedGizmoFrame as _; // khora-agents, khora-editor (as khora_sdk::khora_lanes::…), khora-sdk
    use khora_lanes::render_lane::SharedGridConfig as _; // khora-agents, khora-editor (as khora_sdk::khora_lanes::…), khora-sdk
    use khora_lanes::render_lane::SharedWireframeConfig as _; // khora-agents, khora-editor (as khora_sdk::khora_lanes::…), khora-sdk
    use khora_lanes::render_lane::SimpleUnlitLane as _; // khora-agents
    use khora_lanes::render_lane::StandardPbrLane as _; // khora-agents
    use khora_lanes::render_lane::StandardShadowsLane as _; // khora-agents
    use khora_lanes::render_lane::UiRenderLane as _; // khora-agents
    use khora_lanes::script_lane::runtime::INITIAL_RATE as _; // khora-agents
    use khora_lanes::script_lane::BudgetedScriptLane as _; // khora-agents
    use khora_lanes::script_lane::Fuel as _; // khora-agents
    use khora_lanes::script_lane::ScriptRunReport as _; // khora-agents
    use khora_lanes::script_lane::ScriptRuntime as _; // khora-agents, khora-sdk
}

#[test]
fn associated_items_used_by_other_crates_still_resolve() {
    let _ = khora_lanes::render_lane::GizmoLane::default; // khora-agents
    let _ = khora_lanes::render_lane::GridLane::default; // khora-agents
    let _ = khora_lanes::render_lane::SkyboxLane::default; // khora-agents
    let _ = khora_lanes::render_lane::WireframeLane::default; // khora-agents
    let _ = khora_lanes::script_lane::ScriptRuntime::new; // khora-agents, khora-sdk
}
