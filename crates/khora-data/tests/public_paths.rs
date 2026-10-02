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

//! Compile-level guard over `khora_data`'s public surface.
//!
//! Every `pub` item reachable through a `pub` path of the crate is named here at
//! its full path: the crate-root re-exports, each `pub mod`, the glob re-exports
//! of the private modules (`ecs::{world, query, page, registry, entity, ...}`),
//! their types, free functions, constants, type aliases, public fields, enum
//! variants, inherent `pub` methods, trait items, and derived trait impls. A
//! reorganisation that moves code between files must keep every one of these
//! paths valid, so this file stops compiling the moment one disappears.
//!
//! Re-exports are also checked for *identity*: a `pub use` that starts pointing
//! at a different item with the same name is a compile error (`same_type` for
//! types, `same_item` for functions, bound forwarding for traits).
//!
//! The last test spells, exactly, the paths other crates of the workspace import
//! today. Nothing is constructed; the tests only have to type-check.

use std::any::type_name;
use std::marker::PhantomData;

/// Compiles only when both arguments name the same type.
fn same_type<T: ?Sized>(_: PhantomData<T>, _: PhantomData<T>) {}

/// Compiles only when both references point at the same item (two distinct
/// `fn` items never share a type, and `&fn-item` does not coerce).
fn same_item<T>(_: &T, _: &T) {}

fn is_clone<T: Clone>() {}
fn is_copy<T: Copy>() {}
fn is_debug<T: std::fmt::Debug + ?Sized>() {}
fn is_default<T: Default>() {}
fn is_eq<T: Eq + ?Sized>() {}
fn is_partial_eq<T: PartialEq + ?Sized>() {}
fn is_hash<T: std::hash::Hash + ?Sized>() {}
fn is_display<T: std::fmt::Display + ?Sized>() {}
fn is_error<T: std::error::Error + ?Sized>() {}
fn is_component<T: khora_data::ecs::component::Component>() {}
fn is_ui_layout_view<T: khora_core::ui::UiLayoutView>() {}

// ---------------------------------------------------------------------------
// Every `pub mod`, including the ones that only hold `inventory`
// registrations and export nothing else.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod every_pub_mod {
    use khora_data::assets as _;
    use khora_data::ecs as _;
    use khora_data::ecs::component as _;
    use khora_data::ecs::layout as _;
    use khora_data::ecs::maintenance as _;
    use khora_data::ecs::soa as _;
    use khora_data::ecs::system as _;
    use khora_data::ecs::systems as _;
    use khora_data::ecs::systems::asset_eviction as _;
    use khora_data::ecs::systems::audio_playback_writeback as _;
    use khora_data::ecs::systems::capture_previous_transform as _;
    use khora_data::ecs::systems::collision_dispatch as _;
    use khora_data::ecs::systems::collision_to_script as _;
    use khora_data::ecs::systems::ecs_maintenance as _;
    use khora_data::ecs::systems::ensure_global_transform as _;
    use khora_data::ecs::systems::gpu_material_sync as _;
    use khora_data::ecs::systems::gpu_mesh_sync as _;
    use khora_data::ecs::systems::ibl_bake as _;
    use khora_data::ecs::systems::input_map_update as _;
    use khora_data::ecs::systems::physics_debug_extraction as _;
    use khora_data::ecs::systems::physics_world_writeback as _;
    use khora_data::ecs::systems::script_commands as _;
    use khora_data::ecs::systems::script_state as _;
    use khora_data::ecs::systems::transform_propagation as _;
    use khora_data::flow as _;
    use khora_data::flow::audio as _;
    use khora_data::flow::physics as _;
    use khora_data::flow::render as _;
    use khora_data::flow::script as _;
    use khora_data::flow::shadow as _;
    use khora_data::flow::ui as _;
    use khora_data::gpu as _;
    use khora_data::gpu::eviction as _;
    use khora_data::gpu::ibl as _;
    use khora_data::gpu::projection as _;
    use khora_data::physics as _;
    use khora_data::physics::collision as _;
    use khora_data::render as _;
    use khora_data::scene as _;
    use khora_data::scene::component_registration as _;
    use khora_data::scene::material_registration as _;
    use khora_data::scene::migrations as _;
    use khora_data::scene::record as _;
    use khora_data::scene::shape as _;
    use khora_data::ui as _;
    use khora_data::ui::components as _;
    use khora_data::ui::image_atlas as _;
    use khora_data::ui::layout_view as _;
}

// ---------------------------------------------------------------------------
// Public fields (read through a reference) and enum variants (matched
// exhaustively). Nothing is constructed.
// ---------------------------------------------------------------------------

fn sound_data_fields(x: &khora_data::assets::SoundData) {
    let _ = (&x.samples, &x.channels, &x.sample_rate);
}

fn layout_recommendation_variants(x: &khora_data::ecs::layout::LayoutRecommendation) {
    match x {
        khora_data::ecs::layout::LayoutRecommendation::KeepSoa => {}
        khora_data::ecs::layout::LayoutRecommendation::SimdFieldSoa => {}
        khora_data::ecs::layout::LayoutRecommendation::HotColdSplit => {}
    }
}

fn layout_advisor_fields(x: &khora_data::ecs::layout::LayoutAdvisor) {
    let _ = (&x.fat_bytes, &x.large_batch_rows);
}

fn tick_phase_variants(x: &khora_data::ecs::system::TickPhase) {
    match x {
        khora_data::ecs::system::TickPhase::PreSimulation => {}
        khora_data::ecs::system::TickPhase::PostSimulation => {}
        khora_data::ecs::system::TickPhase::PreExtract => {}
        khora_data::ecs::system::TickPhase::Maintenance => {}
    }
}

fn data_system_registration_fields(x: &khora_data::ecs::system::DataSystemRegistration) {
    let _ = (&x.name, &x.phase, &x.run, &x.order_hint, &x.runs_after);
}

fn apply_error_variants(x: &khora_data::ecs::systems::script_commands::ApplyError) {
    match x {
        khora_data::ecs::systems::script_commands::ApplyError::NoSuchEntity(..) => {}
        khora_data::ecs::systems::script_commands::ApplyError::NoTransform(..) => {}
        khora_data::ecs::systems::script_commands::ApplyError::NotAttached { .. } => {}
        khora_data::ecs::systems::script_commands::ApplyError::AlreadyAttached { .. } => {}
        khora_data::ecs::systems::script_commands::ApplyError::UnknownComponent(..) => {}
        khora_data::ecs::systems::script_commands::ApplyError::WouldCycle { .. } => {}
        khora_data::ecs::systems::script_commands::ApplyError::Rejected { .. } => {}
    }
}

fn query_mode_variants(x: &khora_data::ecs::QueryMode) {
    match x {
        khora_data::ecs::QueryMode::Native => {}
        khora_data::ecs::QueryMode::Transversal => {}
        khora_data::ecs::QueryMode::EntityScan => {}
    }
}

fn query_plan_fields(x: &khora_data::ecs::QueryPlan) {
    let _ = (
        &x.mode,
        &x.driver_domain,
        &x.peer_domains,
        &x.driver_signature,
    );
}

fn playback_state_fields(x: &khora_data::ecs::PlaybackState) {
    let _ = (&x.cursor,);
}

fn audio_source_fields(x: &khora_data::ecs::AudioSource) {
    let _ = (&x.handle, &x.volume, &x.looping, &x.autoplay, &x.state);
}

fn projection_type_variants(x: &khora_data::ecs::ProjectionType) {
    match x {
        khora_data::ecs::ProjectionType::Perspective { .. } => {}
        khora_data::ecs::ProjectionType::Orthographic { .. } => {}
    }
}

fn camera_fields(x: &khora_data::ecs::Camera) {
    let _ = (
        &x.projection,
        &x.aspect_ratio,
        &x.z_near,
        &x.z_far,
        &x.is_active,
    );
}

fn children_fields(x: &khora_data::ecs::Children) {
    let _ = (&x.0,);
}

fn global_transform_fields(x: &khora_data::ecs::GlobalTransform) {
    let _ = (&x.0,);
}

fn handle_component_fields(x: &khora_data::ecs::HandleComponent<khora_data::assets::SoundData>) {
    let _ = (&x.handle, &x.uuid);
}

fn light_fields(x: &khora_data::ecs::Light) {
    let _ = (&x.light_type, &x.enabled);
}

fn material_ref_variants(x: &khora_data::ecs::MaterialRef) {
    match x {
        khora_data::ecs::MaterialRef::Inline { .. } => {}
        khora_data::ecs::MaterialRef::Asset(..) => {}
    }
}

fn serializable_material_data_fields(x: &khora_data::scene::SerializableMaterialData) {
    let _ = (&x.type_name, &x.data);
}

fn material_registration_fields(x: &khora_data::scene::MaterialRegistration) {
    let _ = (
        &x.type_name,
        &x.serialize,
        &x.deserialize,
        &x.create_default,
        &x.serialize_json,
        &x.deserialize_json,
    );
}

fn procedural_mesh_kind_variants(x: &khora_data::ecs::ProceduralMeshKind) {
    match x {
        khora_data::ecs::ProceduralMeshKind::Cube => {}
        khora_data::ecs::ProceduralMeshKind::Sphere => {}
        khora_data::ecs::ProceduralMeshKind::Plane => {}
    }
}

fn mesh_ref_variants(x: &khora_data::ecs::MeshRef) {
    match x {
        khora_data::ecs::MeshRef::Procedural { .. } => {}
        khora_data::ecs::MeshRef::Asset(..) => {}
    }
}

fn name_fields(x: &khora_data::ecs::Name) {
    let _ = (&x.0,);
}

fn parent_fields(x: &khora_data::ecs::Parent) {
    let _ = (&x.0,);
}

fn body_motion_fields(x: &khora_data::ecs::BodyMotion) {
    let _ = (&x.linear, &x.angular);
}

fn collider_fields(x: &khora_data::ecs::Collider) {
    let _ = (
        &x.handle,
        &x.shape,
        &x.friction,
        &x.restitution,
        &x.is_sensor,
    );
}

fn kinematic_character_controller_fields(x: &khora_data::ecs::KinematicCharacterController) {
    let _ = (
        &x.desired_translation,
        &x.offset,
        &x.max_slope_climb_angle,
        &x.min_slope_slide_angle,
        &x.autostep_height,
        &x.autostep_min_width,
        &x.autostep_enabled,
        &x.is_grounded,
    );
}

fn physics_debug_data_fields(x: &khora_data::ecs::PhysicsDebugData) {
    let _ = (&x.vertices, &x.indices, &x.enabled);
}

fn physics_material_fields(x: &khora_data::ecs::PhysicsMaterial) {
    let _ = (&x.friction, &x.restitution);
}

fn rigid_body_fields(x: &khora_data::ecs::RigidBody) {
    let _ = (
        &x.handle,
        &x.body_type,
        &x.mass,
        &x.ccd_enabled,
        &x.initial_velocity,
        &x.initial_angular_velocity,
    );
}

fn script_fields(x: &khora_data::ecs::Script) {
    let _ = (&x.module, &x.behavior, &x.fields, &x.runtime);
}

fn simulated_transform_fields(x: &khora_data::ecs::SimulatedTransform) {
    let _ = (&x.0,);
}

fn tag_fields(x: &khora_data::ecs::Tag) {
    let _ = (&x.0,);
}

fn transform_fields(x: &khora_data::ecs::Transform) {
    let _ = (&x.translation, &x.rotation, &x.scale);
}

fn set_from_bytes_error_variants(x: &khora_data::ecs::SetFromBytesError) {
    match x {
        khora_data::ecs::SetFromBytesError::MisalignedLength { .. } => {}
        khora_data::ecs::SetFromBytesError::PayloadTooLarge { .. } => {}
    }
}

fn page_index_fields(x: &khora_data::ecs::PageIndex) {
    let _ = (&x.page_id, &x.row_index);
}

fn semantic_domain_variants(x: &khora_data::ecs::SemanticDomain) {
    match x {
        khora_data::ecs::SemanticDomain::Spatial => {}
        khora_data::ecs::SemanticDomain::Render => {}
        khora_data::ecs::SemanticDomain::Audio => {}
        khora_data::ecs::SemanticDomain::Physics => {}
        khora_data::ecs::SemanticDomain::Ui => {}
        khora_data::ecs::SemanticDomain::Script => {}
    }
}

fn component_provenance_variants(x: &khora_data::ecs::ComponentProvenance) {
    match x {
        khora_data::ecs::ComponentProvenance::Authored => {}
        khora_data::ecs::ComponentProvenance::ToolAuthored => {}
        khora_data::ecs::ComponentProvenance::Derived => {}
        khora_data::ecs::ComponentProvenance::Runtime => {}
    }
}

fn layout_policy_variants(x: &khora_data::ecs::LayoutPolicy) {
    match x {
        khora_data::ecs::LayoutPolicy::Soa => {}
        khora_data::ecs::LayoutPolicy::AoSoA { .. } => {}
    }
}

fn access_counters_fields(x: &khora_data::ecs::AccessCounters) {
    let _ = (&x.query_count, &x.rows_scanned);
}

fn component_domain_registration_fields(x: &khora_data::ecs::ComponentDomainRegistration) {
    let _ = (&x.register,);
}

fn add_component_error_variants(x: &khora_data::ecs::AddComponentError) {
    match x {
        khora_data::ecs::AddComponentError::EntityNotFound => {}
        khora_data::ecs::AddComponentError::ComponentNotRegistered => {}
        khora_data::ecs::AddComponentError::ComponentAlreadyExists => {}
    }
}

fn remove_component_error_variants(x: &khora_data::ecs::RemoveComponentError) {
    match x {
        khora_data::ecs::RemoveComponentError::EntityNotFound => {}
        khora_data::ecs::RemoveComponentError::ComponentNotRegistered => {}
        khora_data::ecs::RemoveComponentError::ComponentNotPresent => {}
    }
}

fn deserialize_archetype_error_variants(x: &khora_data::ecs::DeserializeArchetypeError) {
    match x {
        khora_data::ecs::DeserializeArchetypeError::Decode(..) => {}
        khora_data::ecs::DeserializeArchetypeError::UnknownComponent(..) => {}
        khora_data::ecs::DeserializeArchetypeError::InvalidColumn(..) => {}
    }
}

fn domain_stats_fields(x: &khora_data::ecs::DomainStats) {
    let _ = (&x.entity_count, &x.page_count);
}

fn audio_source_snapshot_fields(x: &khora_data::flow::audio::AudioSourceSnapshot) {
    let _ = (
        &x.entity,
        &x.handle,
        &x.position,
        &x.volume,
        &x.looping,
        &x.autoplay,
        &x.state,
    );
}

fn audio_playback_update_fields(x: &khora_data::flow::audio::AudioPlaybackUpdate) {
    let _ = (&x.entity, &x.new_state);
}

fn audio_playback_writeback_fields(x: &khora_data::flow::audio::AudioPlaybackWriteback) {
    let _ = (&x.updates,);
}

fn audio_view_fields(x: &khora_data::flow::audio::AudioView) {
    let _ = (
        &x.source_count,
        &x.listener_position,
        &x.listener_transform,
        &x.sources,
    );
}

fn physics_view_fields(x: &khora_data::flow::physics::PhysicsView) {
    let _ = (&x.active_bodies, &x.stashed_bodies, &x.camera_anchor);
}

fn physics_step_result_fields(x: &khora_data::flow::physics::PhysicsStepResult) {
    let _ = (&x.dt,);
}

fn script_program_fields(x: &khora_data::flow::script::ScriptProgram) {
    let _ = (&x.module, &x.behavior);
}

fn script_instance_fields(x: &khora_data::flow::script::ScriptInstance) {
    let _ = (
        &x.entity,
        &x.program,
        &x.authored,
        &x.translation,
        &x.rotation,
        &x.scale,
    );
}

fn script_view_fields(x: &khora_data::flow::script::ScriptView) {
    let _ = (&x.delta_seconds, &x.programs, &x.instances, &x.input);
}

fn shadow_matrices_variants(x: &khora_data::flow::shadow::ShadowMatrices) {
    match x {
        khora_data::flow::shadow::ShadowMatrices::Single(..) => {}
        khora_data::flow::shadow::ShadowMatrices::Cube(..) => {}
    }
}

fn shadow_view_fields(x: &khora_data::flow::shadow::ShadowView) {
    let _ = (&x.light_count, &x.matrices);
}

fn selection_fields(x: &khora_data::flow::Selection) {
    let _ = (&x.entities,);
}

fn flow_registration_fields(x: &khora_data::flow::FlowRegistration) {
    let _ = (&x.name, &x.domain, &x.run);
}

fn environment_map_fields(x: &khora_data::gpu::ibl::EnvironmentMap) {
    let _ = (&x.texture,);
}

fn collision_pair_fields(x: &khora_data::physics::collision::CollisionPair) {
    let _ = (&x.entity_a, &x.entity_b);
}

fn collision_pairs_fields(x: &khora_data::physics::collision::CollisionPairs) {
    let _ = (&x.pairs,);
}

fn overlay_pass_slot_fields(x: &khora_data::render::OverlayPassSlot) {
    let _ = (&x.0,);
}

fn pass_contribution_fields(x: &khora_data::render::PassContribution) {
    let _ = (&x.descriptor, &x.command_buffer);
}

fn pass_descriptor_fields(x: &khora_data::render::PassDescriptor) {
    let _ = (&x.name, &x.reads, &x.writes);
}

fn resource_id_variants(x: &khora_data::render::ResourceId) {
    match x {
        khora_data::render::ResourceId::Color => {}
        khora_data::render::ResourceId::Depth => {}
        khora_data::render::ResourceId::ShadowAtlas => {}
        khora_data::render::ResourceId::Custom(..) => {}
    }
}

fn scene_pass_slot_fields(x: &khora_data::render::ScenePassSlot) {
    let _ = (&x.0,);
}

fn skybox_pass_slot_fields(x: &khora_data::render::SkyboxPassSlot) {
    let _ = (&x.0,);
}

fn transparent_pass_slot_fields(x: &khora_data::render::TransparentPassSlot) {
    let _ = (&x.0,);
}

fn ui_pass_slot_fields(x: &khora_data::render::UiPassSlot) {
    let _ = (&x.0,);
}

fn gizmo_frame_fields(x: &khora_data::render::GizmoFrame) {
    let _ = (&x.lines,);
}

fn grid_config_fields(x: &khora_data::render::GridConfig) {
    let _ = (&x.enabled,);
}

fn wireframe_config_fields(x: &khora_data::render::WireframeConfig) {
    let _ = (&x.enabled, &x.line_color, &x.line_width);
}

fn extracted_light_fields(x: &khora_data::render::ExtractedLight) {
    let _ = (
        &x.light_type,
        &x.position,
        &x.direction,
        &x.shadow_view_proj,
        &x.shadow_atlas_index,
    );
}

fn extracted_mesh_fields(x: &khora_data::render::ExtractedMesh) {
    let _ = (
        &x.transform,
        &x.cpu_mesh_uuid,
        &x.gpu_mesh,
        &x.material,
        &x.gpu_material,
    );
}

fn extracted_view_fields(x: &khora_data::render::ExtractedView) {
    let _ = (&x.view_proj, &x.position);
}

fn render_world_fields(x: &khora_data::render::RenderWorld) {
    let _ = (&x.meshes, &x.lights, &x.views);
}

fn component_registration_fields(
    x: &khora_data::scene::component_registration::ComponentRegistration,
) {
    let _ = (
        &x.type_id,
        &x.shape,
        &x.type_name,
        &x.provenance,
        &x.serialize_recipe,
        &x.deserialize_recipe,
        &x.create_default,
        &x.to_json,
        &x.from_json,
        &x.remove,
    );
}

fn component_shape_variants(x: &khora_data::scene::shape::ComponentShape) {
    match x {
        khora_data::scene::shape::ComponentShape::Fields(..) => {}
        khora_data::scene::shape::ComponentShape::Opaque => {}
    }
}

fn field_schema_fields(x: &khora_data::scene::shape::FieldSchema) {
    let _ = (&x.name, &x.ty);
}

fn migration_error_variants(x: &khora_data::scene::migrations::MigrationError) {
    match x {
        khora_data::scene::migrations::MigrationError::DecodeFailed(..) => {}
        khora_data::scene::migrations::MigrationError::EncodeFailed(..) => {}
        khora_data::scene::migrations::MigrationError::StepMissing { .. } => {}
    }
}

fn scene_migration_registration_fields(
    x: &khora_data::scene::migrations::SceneMigrationRegistration,
) {
    let _ = (&x.migration,);
}

fn scene_recipe_fields(x: &khora_data::scene::SceneRecipe) {
    let _ = (&x.commands,);
}

fn scene_command_variants(x: &khora_data::scene::SceneCommand) {
    match x {
        khora_data::scene::SceneCommand::Spawn { .. } => {}
        khora_data::scene::SceneCommand::AddComponent { .. } => {}
        khora_data::scene::SceneCommand::SetParent { .. } => {}
    }
}

fn component_definition_fields(x: &khora_data::scene::ComponentDefinition) {
    let _ = (&x.type_name, &x.data_base64);
}

fn entity_definition_fields(x: &khora_data::scene::EntityDefinition) {
    let _ = (&x.id, &x.components);
}

fn scene_definition_fields(x: &khora_data::scene::SceneDefinition) {
    let _ = (&x.entities,);
}

fn serialization_error_variants(x: &khora_data::scene::SerializationError) {
    match x {
        khora_data::scene::SerializationError::ProcessingFailed(..) => {}
    }
}

fn deserialization_error_variants(x: &khora_data::scene::DeserializationError) {
    match x {
        khora_data::scene::DeserializationError::InvalidFormat(..) => {}
        khora_data::scene::DeserializationError::WorldPopulationFailed(..) => {}
    }
}

fn ui_node_fields(x: &khora_data::ui::components::UiNode) {
    let _ = (
        &x.width,
        &x.height,
        &x.min_width,
        &x.min_height,
        &x.max_width,
        &x.max_height,
        &x.padding,
        &x.margin,
        &x.flex_direction,
        &x.flex_grow,
        &x.flex_shrink,
    );
}

fn ui_transform_fields(x: &khora_data::ui::components::UiTransform) {
    let _ = (&x.pos, &x.size, &x.z_index);
}

fn ui_style_fields(x: &khora_data::ui::components::UiStyle) {
    let _ = (
        &x.background_color,
        &x.border_radius,
        &x.border_color,
        &x.border_width,
        &x.texture_id,
    );
}

fn ui_color_fields(x: &khora_data::ui::components::UiColor) {
    let _ = (&x.0,);
}

fn ui_image_fields(x: &khora_data::ui::components::UiImage) {
    let _ = (&x.texture,);
}

fn ui_border_fields(x: &khora_data::ui::components::UiBorder) {
    let _ = (&x.width, &x.color, &x.radius);
}

fn ui_interaction_state_variants(x: &khora_data::ui::components::UiInteractionState) {
    match x {
        khora_data::ui::components::UiInteractionState::Normal => {}
        khora_data::ui::components::UiInteractionState::Hovered => {}
        khora_data::ui::components::UiInteractionState::Pressed => {}
        khora_data::ui::components::UiInteractionState::Focused => {}
    }
}

fn ui_interaction_fields(x: &khora_data::ui::components::UiInteraction) {
    let _ = (&x.state, &x.focusable, &x.blocks_input);
}

fn ui_text_fields(x: &khora_data::ui::components::UiText) {
    let _ = (&x.content, &x.font, &x.size, &x.color);
}

fn extracted_ui_node_fields(x: &khora_data::ui::ExtractedUiNode) {
    let _ = (&x.pos, &x.size, &x.color, &x.border, &x.image, &x.z_index);
}

fn extracted_ui_text_fields(x: &khora_data::ui::ExtractedUiText) {
    let _ = (&x.pos, &x.layout, &x.color, &x.z_index);
}

fn ui_atlas_map_fields(x: &khora_data::ui::UiAtlasMap) {
    let _ = (&x.0,);
}

fn ui_scene_fields(x: &khora_data::ui::UiScene) {
    let _ = (&x.nodes, &x.texts, &x.surface_size);
}

// ---------------------------------------------------------------------------
// Trait items, named through a generic parameter so no implementor is needed.
// A re-exported trait is checked for identity by forwarding a bound both ways.
// ---------------------------------------------------------------------------

fn component_trait_items<T: khora_data::ecs::component::Component>() {
    let _ = T::make_column;
    let _ = T::push_into_column;
    let _ = T::copy_row_between;
    let _ = T::clone_from_column;
    let _ = T::set_in_column;
}

fn component_trait_identity<T: khora_data::ecs::Component>() {
    component_trait_items::<T>();
}

fn component_trait_identity_rev<T: khora_data::ecs::component::Component>() {
    component_trait_identity::<T>();
}

fn soa_layout_trait_items<S: khora_data::ecs::soa::SoaLayout>() {
    let _ = S::FIELD_COUNT;
    let _ = S::FIELD_NAMES;
    let _ = S::scatter_push;
    let _ = S::scatter_set;
    let _ = S::gather;
}

fn soa_layout_trait_identity<S: khora_data::ecs::SoaLayout>() {
    soa_layout_trait_items::<S>();
}

fn soa_layout_trait_identity_rev<S: khora_data::ecs::soa::SoaLayout>() {
    soa_layout_trait_identity::<S>();
}

fn component_bundle_trait_items<B: khora_data::ecs::ComponentBundle>() {
    let _ = B::type_ids;
    let _ = B::create_columns;
    let _ = B::update_metadata;
    let _ = B::add_to_page;
}

fn any_vec_trait_items<V: khora_data::ecs::AnyVec + ?Sized>() {
    let _ = V::as_any;
    let _ = V::as_any_mut;
    let _ = V::swap_remove_any;
    let _ = V::to_bytes;
    let _ = V::set_from_bytes;
}

fn world_query_trait_items<Q: khora_data::ecs::WorldQuery>() {
    let _ = type_name::<Q::Item<'static>>();
    let _ = Q::type_ids;
    let _ = Q::without_type_ids;
    let _ = Q::optional_type_ids;
    let _ = Q::mutable_type_ids;
    let _ = Q::fetch;
    let _ = Q::fetch_from_world;
}

fn flow_trait_items<F: khora_data::flow::Flow>() {
    let _ = type_name::<F::View>();
    let _ = F::DOMAIN;
    let _ = F::NAME;
    let _ = F::select;
    let _ = F::project;
    let _ = F::cache_key;
}

fn scene_migration_trait_items<M: khora_data::scene::migrations::SceneMigration + ?Sized>() {
    let _ = M::from_version;
    let _ = M::to_version;
    let _ = M::migrate;
}

fn serialization_strategy_trait_items<S: khora_data::scene::SerializationStrategy>() {
    let _ = S::get_strategy_id;
    let _ = S::serialize;
    let _ = S::deserialize;
}

// ---------------------------------------------------------------------------
// Methods taking `impl Trait` cannot be named as a bare path; calling them from
// a never-run function checks them instead.
// ---------------------------------------------------------------------------

fn methods_taking_impl_into(tag: &mut khora_data::ecs::Tag) {
    let _ = khora_data::ecs::Name::new("");
    let _ =
        khora_data::ecs::Script::new("", "").with_field("", khora_core::script::ScriptValue::Unit);
    let _ = tag.insert("");
}

fn world_for_each_soa_column_mut<S: khora_data::ecs::soa::SoaLayout>(
    world: &mut khora_data::ecs::World,
) {
    world.for_each_soa_column_mut::<S>(|_: &mut khora_data::ecs::soa::FieldSoaColumn<S>| {});
}

// ---------------------------------------------------------------------------
// A field-SoA probe, so the items generic over `SoaLayout` can be instantiated.
// Its impls are never exercised at run time here (`derive_paths.rs` does that).
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct ProbeSoa {
    x: f32,
}

impl khora_data::ecs::component::Component for ProbeSoa {}

impl khora_data::ecs::soa::SoaLayout for ProbeSoa {
    const FIELD_COUNT: usize = 1;
    const FIELD_NAMES: &'static [&'static str] = &["x"];
    fn scatter_push(&self, fields: &mut [Vec<f32>]) {
        fields[0].push(self.x);
    }
    fn scatter_set(&self, fields: &mut [Vec<f32>], row: usize) {
        fields[0][row] = self.x;
    }
    fn gather(fields: &[Vec<f32>], row: usize) -> Self {
        Self { x: fields[0][row] }
    }
}

// ---------------------------------------------------------------------------
// `register_flow!` probe: a flow of this test crate, registered through the
// exported macro (whose body spells `$crate::ecs::World`, `$crate::flow::Flow`,
// `$crate::flow::run_flow_cached` and `$crate::flow::FlowRegistration`).
// ---------------------------------------------------------------------------

#[derive(Default)]
struct ProbeFlow;

impl khora_data::flow::Flow for ProbeFlow {
    type View = ();
    const DOMAIN: khora_data::ecs::SemanticDomain = khora_data::ecs::SemanticDomain::Ui;
    const NAME: &'static str = "public_paths_probe_flow";

    fn project(
        &self,
        _world: &khora_data::ecs::World,
        _sel: &khora_data::flow::Selection,
        _runtime: &khora_core::Runtime,
    ) {
    }
}

khora_data::register_flow!(ProbeFlow);

/// Items generic over a field-SoA component: no `khora_data` type opts into
/// `#[component(layout = "soa")]`, so they are named through a parameter.
fn soa_generic<S: khora_data::ecs::soa::SoaLayout + std::fmt::Debug>() {
    let _ = type_name::<khora_data::ecs::soa::FieldSoaColumn<S>>();
    let _ = type_name::<khora_data::ecs::FieldSoaColumn<S>>();
    same_type(
        PhantomData::<khora_data::ecs::FieldSoaColumn<S>>,
        PhantomData::<khora_data::ecs::soa::FieldSoaColumn<S>>,
    );
    let _ = khora_data::ecs::soa::FieldSoaColumn::<S>::new;
    let _ = khora_data::ecs::soa::FieldSoaColumn::<S>::len;
    let _ = khora_data::ecs::soa::FieldSoaColumn::<S>::is_empty;
    let _ = khora_data::ecs::soa::FieldSoaColumn::<S>::push;
    let _ = khora_data::ecs::soa::FieldSoaColumn::<S>::get;
    let _ = khora_data::ecs::soa::FieldSoaColumn::<S>::set;
    let _ = khora_data::ecs::soa::FieldSoaColumn::<S>::field;
    let _ = khora_data::ecs::soa::FieldSoaColumn::<S>::field_mut;
    let _ = khora_data::ecs::soa::FieldSoaColumn::<S>::fields;
    let _ = khora_data::ecs::soa::FieldSoaColumn::<S>::fields_mut;
    is_debug::<khora_data::ecs::soa::FieldSoaColumn<S>>();
    is_default::<khora_data::ecs::soa::FieldSoaColumn<S>>();
    let _ = type_name::<khora_data::ecs::Soa<S>>();
}

#[test]
fn module_assets_paths_still_resolve() {
    let _ = type_name::<khora_data::assets::SoundData>();
    let _ = sound_data_fields as fn(&khora_data::assets::SoundData);
    is_clone::<khora_data::assets::SoundData>();
    is_debug::<khora_data::assets::SoundData>();
    is_default::<khora_data::assets::SoundData>();
    let _ = type_name::<khora_data::assets::Assets<khora_data::assets::SoundData>>();
    let _ = khora_data::assets::Assets::<khora_data::assets::SoundData>::new;
    let _ = khora_data::assets::Assets::<khora_data::assets::SoundData>::insert;
    let _ = khora_data::assets::Assets::<khora_data::assets::SoundData>::get;
    let _ = khora_data::assets::Assets::<khora_data::assets::SoundData>::contains;
    let _ = khora_data::assets::Assets::<khora_data::assets::SoundData>::remove;
    let _ = khora_data::assets::Assets::<khora_data::assets::SoundData>::keys;
    let _ = khora_data::assets::Assets::<khora_data::assets::SoundData>::len;
    let _ = khora_data::assets::Assets::<khora_data::assets::SoundData>::is_empty;
    is_clone::<khora_data::assets::Assets<khora_data::assets::SoundData>>();
    is_default::<khora_data::assets::Assets<khora_data::assets::SoundData>>();
    let _ = type_name::<khora_data::assets::AssetStore>();
    let _ = type_name::<khora_data::AssetStore>();
    same_type(
        PhantomData::<khora_data::AssetStore>,
        PhantomData::<khora_data::assets::AssetStore>,
    );
    let _ = khora_data::assets::AssetStore::new;
    let _ = khora_data::assets::AssetStore::store::<khora_data::assets::SoundData>;
    is_clone::<khora_data::assets::AssetStore>();
    is_default::<khora_data::assets::AssetStore>();
}

#[test]
fn module_ecs_paths_still_resolve() {
    component_trait_items::<khora_data::ecs::Transform>();
    component_trait_identity::<khora_data::ecs::Transform>();
    component_trait_identity_rev::<khora_data::ecs::Transform>();
    let _ = type_name::<khora_data::ecs::layout::Ucb1>();
    let _ = khora_data::ecs::layout::Ucb1::DEFAULT_EXPLORATION;
    let _ = khora_data::ecs::layout::Ucb1::new;
    let _ = khora_data::ecs::layout::Ucb1::with_exploration;
    let _ = khora_data::ecs::layout::Ucb1::arm_count;
    let _ = khora_data::ecs::layout::Ucb1::pulls;
    let _ = khora_data::ecs::layout::Ucb1::select;
    let _ = khora_data::ecs::layout::Ucb1::record;
    let _ = khora_data::ecs::layout::Ucb1::best_arm;
    let _ = khora_data::ecs::layout::Ucb1::mean_reward;
    is_clone::<khora_data::ecs::layout::Ucb1>();
    is_debug::<khora_data::ecs::layout::Ucb1>();
    let _ = type_name::<khora_data::ecs::layout::SlidingWindowUcb1>();
    let _ = khora_data::ecs::layout::SlidingWindowUcb1::new;
    let _ = khora_data::ecs::layout::SlidingWindowUcb1::with_exploration;
    let _ = khora_data::ecs::layout::SlidingWindowUcb1::arm_count;
    let _ = khora_data::ecs::layout::SlidingWindowUcb1::samples;
    let _ = khora_data::ecs::layout::SlidingWindowUcb1::select;
    let _ = khora_data::ecs::layout::SlidingWindowUcb1::record;
    let _ = khora_data::ecs::layout::SlidingWindowUcb1::best_arm;
    let _ = khora_data::ecs::layout::SlidingWindowUcb1::mean_reward;
    is_clone::<khora_data::ecs::layout::SlidingWindowUcb1>();
    is_debug::<khora_data::ecs::layout::SlidingWindowUcb1>();
    let _ = khora_data::ecs::layout::net_reward;
    let _ = khora_data::ecs::layout::should_switch;
    let _ = type_name::<khora_data::ecs::layout::DecayCounter>();
    let _ = khora_data::ecs::layout::DecayCounter::new;
    let _ = khora_data::ecs::layout::DecayCounter::add;
    let _ = khora_data::ecs::layout::DecayCounter::tick;
    let _ = khora_data::ecs::layout::DecayCounter::value;
    is_clone::<khora_data::ecs::layout::DecayCounter>();
    is_copy::<khora_data::ecs::layout::DecayCounter>();
    is_debug::<khora_data::ecs::layout::DecayCounter>();
    let _ = type_name::<khora_data::ecs::layout::LayoutRecommendation>();
    let _ = layout_recommendation_variants as fn(&khora_data::ecs::layout::LayoutRecommendation);
    let _ = khora_data::ecs::layout::LayoutRecommendation::reason;
    is_clone::<khora_data::ecs::layout::LayoutRecommendation>();
    is_copy::<khora_data::ecs::layout::LayoutRecommendation>();
    is_debug::<khora_data::ecs::layout::LayoutRecommendation>();
    is_eq::<khora_data::ecs::layout::LayoutRecommendation>();
    is_partial_eq::<khora_data::ecs::layout::LayoutRecommendation>();
    let _ = type_name::<khora_data::ecs::layout::LayoutAdvisor>();
    let _ = layout_advisor_fields as fn(&khora_data::ecs::layout::LayoutAdvisor);
    let _ = khora_data::ecs::layout::LayoutAdvisor::recommend;
    is_clone::<khora_data::ecs::layout::LayoutAdvisor>();
    is_copy::<khora_data::ecs::layout::LayoutAdvisor>();
    is_debug::<khora_data::ecs::layout::LayoutAdvisor>();
    is_default::<khora_data::ecs::layout::LayoutAdvisor>();
    let _ = type_name::<khora_data::ecs::maintenance::EcsMaintenance>();
    let _ = type_name::<khora_data::ecs::EcsMaintenance>();
    same_type(
        PhantomData::<khora_data::ecs::EcsMaintenance>,
        PhantomData::<khora_data::ecs::maintenance::EcsMaintenance>,
    );
    let _ = khora_data::ecs::maintenance::EcsMaintenance::new;
    let _ = khora_data::ecs::maintenance::EcsMaintenance::with_budget;
    let _ = khora_data::ecs::maintenance::EcsMaintenance::tick;
    let _ = khora_data::ecs::maintenance::EcsMaintenance::last_compacted_count;
    let _ = khora_data::ecs::maintenance::EcsMaintenance::max_per_frame;
    is_default::<khora_data::ecs::maintenance::EcsMaintenance>();
    soa_generic::<ProbeSoa>();
    soa_layout_trait_identity_rev::<ProbeSoa>();
    let _ = world_for_each_soa_column_mut::<ProbeSoa> as fn(&mut khora_data::ecs::World);
    let _ = type_name::<khora_data::ecs::system::TickPhase>();
    let _ = type_name::<khora_data::ecs::TickPhase>();
    same_type(
        PhantomData::<khora_data::ecs::TickPhase>,
        PhantomData::<khora_data::ecs::system::TickPhase>,
    );
    let _ = tick_phase_variants as fn(&khora_data::ecs::system::TickPhase);
    is_clone::<khora_data::ecs::system::TickPhase>();
    is_copy::<khora_data::ecs::system::TickPhase>();
    is_debug::<khora_data::ecs::system::TickPhase>();
    is_eq::<khora_data::ecs::system::TickPhase>();
    is_hash::<khora_data::ecs::system::TickPhase>();
    is_partial_eq::<khora_data::ecs::system::TickPhase>();
    let _ = type_name::<khora_data::ecs::system::DataSystemRegistration>();
    let _ = type_name::<khora_data::ecs::DataSystemRegistration>();
    same_type(
        PhantomData::<khora_data::ecs::DataSystemRegistration>,
        PhantomData::<khora_data::ecs::system::DataSystemRegistration>,
    );
    let _ = data_system_registration_fields as fn(&khora_data::ecs::system::DataSystemRegistration);
    let _ = khora_data::ecs::systems::collision_to_script::TOUCHED;
    let _ = khora_data::ecs::systems::collision_to_script::SEPARATED;
    let _ = type_name::<khora_data::ecs::systems::script_commands::ApplyError>();
    let _ = apply_error_variants as fn(&khora_data::ecs::systems::script_commands::ApplyError);
    is_clone::<khora_data::ecs::systems::script_commands::ApplyError>();
    is_debug::<khora_data::ecs::systems::script_commands::ApplyError>();
    is_display::<khora_data::ecs::systems::script_commands::ApplyError>();
    is_eq::<khora_data::ecs::systems::script_commands::ApplyError>();
    is_partial_eq::<khora_data::ecs::systems::script_commands::ApplyError>();
    let _ = khora_data::ecs::systems::script_commands::apply_all;
    let _ = khora_data::ecs::systems::transform_propagation::transform_propagation_system;
    let _ = khora_data::ecs::systems::transform_propagation_system;
    same_item(
        &khora_data::ecs::systems::transform_propagation_system,
        &khora_data::ecs::systems::transform_propagation::transform_propagation_system,
    );
    let _ = type_name::<khora_data::ecs::DomainBitset>();
    let _ = khora_data::ecs::DomainBitset::new;
    let _ = khora_data::ecs::DomainBitset::set;
    let _ = khora_data::ecs::DomainBitset::clear;
    let _ = khora_data::ecs::DomainBitset::get_block;
    let _ = khora_data::ecs::DomainBitset::is_set;
    let _ = khora_data::ecs::DomainBitset::intersect;
    is_clone::<khora_data::ecs::DomainBitset>();
    is_debug::<khora_data::ecs::DomainBitset>();
    is_default::<khora_data::ecs::DomainBitset>();
    component_bundle_trait_items::<khora_data::ecs::Transform>();
    component_bundle_trait_items::<(khora_data::ecs::Transform, khora_data::ecs::Name)>();
    let _ = type_name::<khora_data::ecs::QueryMode>();
    let _ = query_mode_variants as fn(&khora_data::ecs::QueryMode);
    is_clone::<khora_data::ecs::QueryMode>();
    is_copy::<khora_data::ecs::QueryMode>();
    is_debug::<khora_data::ecs::QueryMode>();
    is_eq::<khora_data::ecs::QueryMode>();
    is_partial_eq::<khora_data::ecs::QueryMode>();
    let _ = type_name::<khora_data::ecs::QueryPlan>();
    let _ = query_plan_fields as fn(&khora_data::ecs::QueryPlan);
    let _ = khora_data::ecs::QueryPlan::new;
    let _ = khora_data::ecs::QueryPlan::entity_scan;
    is_clone::<khora_data::ecs::QueryPlan>();
    is_debug::<khora_data::ecs::QueryPlan>();
    let _ = khora_data::ecs::entities_with_all_tags;
    let _ = khora_data::ecs::entities_with_any_tag;
    let _ = khora_data::ecs::entities_with_tag;
    let _ = type_name::<khora_data::ecs::AudioListener>();
    is_clone::<khora_data::ecs::AudioListener>();
    is_component::<khora_data::ecs::AudioListener>();
    is_copy::<khora_data::ecs::AudioListener>();
    is_debug::<khora_data::ecs::AudioListener>();
    is_default::<khora_data::ecs::AudioListener>();
    let _ = type_name::<khora_data::ecs::PlaybackState>();
    let _ = playback_state_fields as fn(&khora_data::ecs::PlaybackState);
    is_clone::<khora_data::ecs::PlaybackState>();
    is_debug::<khora_data::ecs::PlaybackState>();
    is_partial_eq::<khora_data::ecs::PlaybackState>();
    let _ = type_name::<khora_data::ecs::AudioSource>();
    let _ = audio_source_fields as fn(&khora_data::ecs::AudioSource);
    let _ = khora_data::ecs::AudioSource::new;
    is_clone::<khora_data::ecs::AudioSource>();
    is_component::<khora_data::ecs::AudioSource>();
    is_debug::<khora_data::ecs::AudioSource>();
    is_default::<khora_data::ecs::AudioSource>();
    let _ = type_name::<khora_data::ecs::ProjectionType>();
    let _ = projection_type_variants as fn(&khora_data::ecs::ProjectionType);
    is_clone::<khora_data::ecs::ProjectionType>();
    is_copy::<khora_data::ecs::ProjectionType>();
    is_debug::<khora_data::ecs::ProjectionType>();
    is_partial_eq::<khora_data::ecs::ProjectionType>();
    let _ = type_name::<khora_data::ecs::Camera>();
    let _ = camera_fields as fn(&khora_data::ecs::Camera);
    let _ = khora_data::ecs::Camera::new_perspective;
    let _ = khora_data::ecs::Camera::new_orthographic;
    let _ = khora_data::ecs::Camera::default_perspective;
    let _ = khora_data::ecs::Camera::default_orthographic;
    let _ = khora_data::ecs::Camera::projection_matrix;
    let _ = khora_data::ecs::Camera::set_aspect_ratio;
    is_clone::<khora_data::ecs::Camera>();
    is_component::<khora_data::ecs::Camera>();
    is_copy::<khora_data::ecs::Camera>();
    is_debug::<khora_data::ecs::Camera>();
    is_default::<khora_data::ecs::Camera>();
    is_partial_eq::<khora_data::ecs::Camera>();
    let _ = type_name::<khora_data::ecs::Children>();
    let _ = children_fields as fn(&khora_data::ecs::Children);
    is_clone::<khora_data::ecs::Children>();
    is_component::<khora_data::ecs::Children>();
    is_debug::<khora_data::ecs::Children>();
    is_default::<khora_data::ecs::Children>();
    is_eq::<khora_data::ecs::Children>();
    is_partial_eq::<khora_data::ecs::Children>();
    let _ = type_name::<khora_data::ecs::GlobalTransform>();
    let _ = global_transform_fields as fn(&khora_data::ecs::GlobalTransform);
    let _ = khora_data::ecs::GlobalTransform::new;
    let _ = khora_data::ecs::GlobalTransform::identity;
    let _ = khora_data::ecs::GlobalTransform::to_matrix;
    let _ = khora_data::ecs::GlobalTransform::at_position;
    is_clone::<khora_data::ecs::GlobalTransform>();
    is_component::<khora_data::ecs::GlobalTransform>();
    is_copy::<khora_data::ecs::GlobalTransform>();
    is_debug::<khora_data::ecs::GlobalTransform>();
    is_default::<khora_data::ecs::GlobalTransform>();
    is_partial_eq::<khora_data::ecs::GlobalTransform>();
    let _ = type_name::<khora_data::ecs::HandleComponent<khora_data::assets::SoundData>>();
    let _ = handle_component_fields
        as fn(&khora_data::ecs::HandleComponent<khora_data::assets::SoundData>);
    is_clone::<khora_data::ecs::HandleComponent<khora_data::assets::SoundData>>();
    is_component::<khora_data::ecs::HandleComponent<khora_data::assets::SoundData>>();
    let _ = type_name::<khora_data::ecs::Light>();
    let _ = light_fields as fn(&khora_data::ecs::Light);
    let _ = khora_data::ecs::Light::new;
    let _ = khora_data::ecs::Light::directional;
    let _ = khora_data::ecs::Light::point;
    let _ = khora_data::ecs::Light::spot;
    is_clone::<khora_data::ecs::Light>();
    is_component::<khora_data::ecs::Light>();
    is_debug::<khora_data::ecs::Light>();
    is_default::<khora_data::ecs::Light>();
    let _ = type_name::<khora_data::ecs::MaterialHandle>();
    let _ = type_name::<khora_data::ecs::MaterialRef>();
    let _ = material_ref_variants as fn(&khora_data::ecs::MaterialRef);
    let _ = khora_data::ecs::MaterialRef::inline;
    let _ = khora_data::ecs::MaterialRef::uuid;
    is_clone::<khora_data::ecs::MaterialRef>();
    is_component::<khora_data::ecs::MaterialRef>();
    let _ = type_name::<khora_data::ecs::ProceduralMeshKind>();
    let _ = procedural_mesh_kind_variants as fn(&khora_data::ecs::ProceduralMeshKind);
    is_clone::<khora_data::ecs::ProceduralMeshKind>();
    is_copy::<khora_data::ecs::ProceduralMeshKind>();
    is_debug::<khora_data::ecs::ProceduralMeshKind>();
    is_eq::<khora_data::ecs::ProceduralMeshKind>();
    is_partial_eq::<khora_data::ecs::ProceduralMeshKind>();
    let _ = type_name::<khora_data::ecs::MeshRef>();
    let _ = mesh_ref_variants as fn(&khora_data::ecs::MeshRef);
    let _ = khora_data::ecs::MeshRef::procedural;
    let _ = khora_data::ecs::MeshRef::uuid;
    let _ = khora_data::ecs::MeshRef::unit_cube;
    is_clone::<khora_data::ecs::MeshRef>();
    is_component::<khora_data::ecs::MeshRef>();
    is_debug::<khora_data::ecs::MeshRef>();
    is_partial_eq::<khora_data::ecs::MeshRef>();
    let _ = khora_data::ecs::reconstruct_procedural_mesh;
    let _ = khora_data::ecs::create_plane;
    let _ = khora_data::ecs::create_cube;
    let _ = khora_data::ecs::create_sphere;
    let _ = type_name::<khora_data::ecs::Name>();
    let _ = name_fields as fn(&khora_data::ecs::Name);
    let _ = methods_taking_impl_into as fn(&mut khora_data::ecs::Tag);
    let _ = khora_data::ecs::Name::as_str;
    is_clone::<khora_data::ecs::Name>();
    is_component::<khora_data::ecs::Name>();
    is_debug::<khora_data::ecs::Name>();
    is_default::<khora_data::ecs::Name>();
    is_display::<khora_data::ecs::Name>();
    is_eq::<khora_data::ecs::Name>();
    is_partial_eq::<khora_data::ecs::Name>();
    let _ = type_name::<khora_data::ecs::Parent>();
    let _ = parent_fields as fn(&khora_data::ecs::Parent);
    is_clone::<khora_data::ecs::Parent>();
    is_component::<khora_data::ecs::Parent>();
    is_copy::<khora_data::ecs::Parent>();
    is_debug::<khora_data::ecs::Parent>();
    is_default::<khora_data::ecs::Parent>();
    is_eq::<khora_data::ecs::Parent>();
    is_partial_eq::<khora_data::ecs::Parent>();
    let _ = type_name::<khora_data::ecs::ActiveEvents>();
    is_clone::<khora_data::ecs::ActiveEvents>();
    is_component::<khora_data::ecs::ActiveEvents>();
    is_copy::<khora_data::ecs::ActiveEvents>();
    is_debug::<khora_data::ecs::ActiveEvents>();
    is_default::<khora_data::ecs::ActiveEvents>();
    let _ = type_name::<khora_data::ecs::BodyMotion>();
    let _ = body_motion_fields as fn(&khora_data::ecs::BodyMotion);
    is_clone::<khora_data::ecs::BodyMotion>();
    is_component::<khora_data::ecs::BodyMotion>();
    is_copy::<khora_data::ecs::BodyMotion>();
    is_debug::<khora_data::ecs::BodyMotion>();
    is_default::<khora_data::ecs::BodyMotion>();
    is_partial_eq::<khora_data::ecs::BodyMotion>();
    let _ = type_name::<khora_data::ecs::Collider>();
    let _ = collider_fields as fn(&khora_data::ecs::Collider);
    let _ = khora_data::ecs::Collider::new_box;
    let _ = khora_data::ecs::Collider::new_sphere;
    is_clone::<khora_data::ecs::Collider>();
    is_component::<khora_data::ecs::Collider>();
    is_debug::<khora_data::ecs::Collider>();
    is_default::<khora_data::ecs::Collider>();
    let _ = type_name::<khora_data::ecs::KinematicCharacterController>();
    let _ =
        kinematic_character_controller_fields as fn(&khora_data::ecs::KinematicCharacterController);
    is_clone::<khora_data::ecs::KinematicCharacterController>();
    is_component::<khora_data::ecs::KinematicCharacterController>();
    is_debug::<khora_data::ecs::KinematicCharacterController>();
    is_default::<khora_data::ecs::KinematicCharacterController>();
    let _ = type_name::<khora_data::ecs::PhysicsDebugData>();
    let _ = physics_debug_data_fields as fn(&khora_data::ecs::PhysicsDebugData);
    is_clone::<khora_data::ecs::PhysicsDebugData>();
    is_component::<khora_data::ecs::PhysicsDebugData>();
    is_debug::<khora_data::ecs::PhysicsDebugData>();
    is_default::<khora_data::ecs::PhysicsDebugData>();
    let _ = type_name::<khora_data::ecs::PhysicsMaterial>();
    let _ = physics_material_fields as fn(&khora_data::ecs::PhysicsMaterial);
    is_clone::<khora_data::ecs::PhysicsMaterial>();
    is_component::<khora_data::ecs::PhysicsMaterial>();
    is_copy::<khora_data::ecs::PhysicsMaterial>();
    is_debug::<khora_data::ecs::PhysicsMaterial>();
    is_default::<khora_data::ecs::PhysicsMaterial>();
    let _ = type_name::<khora_data::ecs::RigidBody>();
    let _ = rigid_body_fields as fn(&khora_data::ecs::RigidBody);
    let _ = khora_data::ecs::RigidBody::new_dynamic;
    let _ = khora_data::ecs::RigidBody::new_static;
    is_clone::<khora_data::ecs::RigidBody>();
    is_component::<khora_data::ecs::RigidBody>();
    is_debug::<khora_data::ecs::RigidBody>();
    is_default::<khora_data::ecs::RigidBody>();
    let _ = type_name::<khora_data::ecs::Script>();
    let _ = script_fields as fn(&khora_data::ecs::Script);
    let _ = khora_data::ecs::Script::field;
    is_clone::<khora_data::ecs::Script>();
    is_component::<khora_data::ecs::Script>();
    is_debug::<khora_data::ecs::Script>();
    is_default::<khora_data::ecs::Script>();
    is_partial_eq::<khora_data::ecs::Script>();
    let _ = type_name::<khora_data::ecs::SimulatedTransform>();
    let _ = simulated_transform_fields as fn(&khora_data::ecs::SimulatedTransform);
    let _ = khora_data::ecs::SimulatedTransform::from_parts;
    let _ = khora_data::ecs::SimulatedTransform::translation;
    let _ = khora_data::ecs::SimulatedTransform::rotation;
    is_clone::<khora_data::ecs::SimulatedTransform>();
    is_component::<khora_data::ecs::SimulatedTransform>();
    is_copy::<khora_data::ecs::SimulatedTransform>();
    is_debug::<khora_data::ecs::SimulatedTransform>();
    is_default::<khora_data::ecs::SimulatedTransform>();
    is_partial_eq::<khora_data::ecs::SimulatedTransform>();
    let _ = type_name::<khora_data::ecs::Tag>();
    let _ = tag_fields as fn(&khora_data::ecs::Tag);
    let _ = khora_data::ecs::Tag::new;
    let _ = khora_data::ecs::Tag::remove;
    let _ = khora_data::ecs::Tag::contains;
    let _ = khora_data::ecs::Tag::len;
    let _ = khora_data::ecs::Tag::is_empty;
    let _ = khora_data::ecs::Tag::iter;
    is_clone::<khora_data::ecs::Tag>();
    is_component::<khora_data::ecs::Tag>();
    is_debug::<khora_data::ecs::Tag>();
    is_default::<khora_data::ecs::Tag>();
    is_eq::<khora_data::ecs::Tag>();
    is_partial_eq::<khora_data::ecs::Tag>();
    let _ = type_name::<khora_data::ecs::Teleported>();
    is_clone::<khora_data::ecs::Teleported>();
    is_component::<khora_data::ecs::Teleported>();
    is_copy::<khora_data::ecs::Teleported>();
    is_debug::<khora_data::ecs::Teleported>();
    is_default::<khora_data::ecs::Teleported>();
    let _ = type_name::<khora_data::ecs::Transform>();
    let _ = transform_fields as fn(&khora_data::ecs::Transform);
    let _ = khora_data::ecs::Transform::new;
    let _ = khora_data::ecs::Transform::from_translation;
    let _ = khora_data::ecs::Transform::identity;
    let _ = khora_data::ecs::Transform::to_mat4;
    is_clone::<khora_data::ecs::Transform>();
    is_component::<khora_data::ecs::Transform>();
    is_copy::<khora_data::ecs::Transform>();
    is_debug::<khora_data::ecs::Transform>();
    is_default::<khora_data::ecs::Transform>();
    is_partial_eq::<khora_data::ecs::Transform>();
    let _ = type_name::<khora_data::ecs::EntityMetadata>();
    is_clone::<khora_data::ecs::EntityMetadata>();
    is_debug::<khora_data::ecs::EntityMetadata>();
    is_default::<khora_data::ecs::EntityMetadata>();
    let _ = khora_data::ecs::MAX_COLUMN_PAYLOAD_BYTES;
    let _ = type_name::<khora_data::ecs::SetFromBytesError>();
    let _ = set_from_bytes_error_variants as fn(&khora_data::ecs::SetFromBytesError);
    is_clone::<khora_data::ecs::SetFromBytesError>();
    is_debug::<khora_data::ecs::SetFromBytesError>();
    is_display::<khora_data::ecs::SetFromBytesError>();
    is_eq::<khora_data::ecs::SetFromBytesError>();
    is_error::<khora_data::ecs::SetFromBytesError>();
    is_partial_eq::<khora_data::ecs::SetFromBytesError>();
    let _ = type_name::<dyn khora_data::ecs::AnyVec>();
    any_vec_trait_items::<dyn khora_data::ecs::AnyVec>();
    let _ = type_name::<khora_data::ecs::PageIndex>();
    let _ = page_index_fields as fn(&khora_data::ecs::PageIndex);
    is_clone::<khora_data::ecs::PageIndex>();
    is_copy::<khora_data::ecs::PageIndex>();
    is_debug::<khora_data::ecs::PageIndex>();
    is_eq::<khora_data::ecs::PageIndex>();
    is_hash::<khora_data::ecs::PageIndex>();
    is_partial_eq::<khora_data::ecs::PageIndex>();
    let _ = type_name::<khora_data::ecs::ComponentPage>();
    world_query_trait_items::<&'static khora_data::ecs::Transform>();
    world_query_trait_items::<&'static mut khora_data::ecs::Transform>();
    world_query_trait_items::<khora_data::ecs::Without<khora_data::ecs::Transform>>();
    let _ = type_name::<khora_data::ecs::Query<'static, &'static khora_data::ecs::Transform>>();
    let _ = type_name::<khora_data::ecs::Without<khora_data::ecs::Transform>>();
    let _ = type_name::<khora_data::ecs::QueryMut<'static, &'static khora_data::ecs::Transform>>();
    let _ = type_name::<khora_data::ecs::SemanticDomain>();
    let _ = semantic_domain_variants as fn(&khora_data::ecs::SemanticDomain);
    let _ = khora_data::ecs::SemanticDomain::COUNT;
    let _ = khora_data::ecs::SemanticDomain::index;
    is_clone::<khora_data::ecs::SemanticDomain>();
    is_copy::<khora_data::ecs::SemanticDomain>();
    is_debug::<khora_data::ecs::SemanticDomain>();
    is_eq::<khora_data::ecs::SemanticDomain>();
    is_hash::<khora_data::ecs::SemanticDomain>();
    is_partial_eq::<khora_data::ecs::SemanticDomain>();
    let _ = type_name::<khora_data::ecs::ComponentProvenance>();
    let _ = component_provenance_variants as fn(&khora_data::ecs::ComponentProvenance);
    let _ = khora_data::ecs::ComponentProvenance::is_hand_authorable;
    let _ = khora_data::ecs::ComponentProvenance::is_copied_on_duplicate;
    is_clone::<khora_data::ecs::ComponentProvenance>();
    is_copy::<khora_data::ecs::ComponentProvenance>();
    is_debug::<khora_data::ecs::ComponentProvenance>();
    is_default::<khora_data::ecs::ComponentProvenance>();
    is_eq::<khora_data::ecs::ComponentProvenance>();
    is_hash::<khora_data::ecs::ComponentProvenance>();
    is_partial_eq::<khora_data::ecs::ComponentProvenance>();
    let _ = type_name::<khora_data::ecs::LayoutPolicy>();
    let _ = layout_policy_variants as fn(&khora_data::ecs::LayoutPolicy);
    is_clone::<khora_data::ecs::LayoutPolicy>();
    is_copy::<khora_data::ecs::LayoutPolicy>();
    is_debug::<khora_data::ecs::LayoutPolicy>();
    is_default::<khora_data::ecs::LayoutPolicy>();
    is_eq::<khora_data::ecs::LayoutPolicy>();
    is_partial_eq::<khora_data::ecs::LayoutPolicy>();
    let _ = type_name::<khora_data::ecs::AccessCounters>();
    let _ = access_counters_fields as fn(&khora_data::ecs::AccessCounters);
    is_debug::<khora_data::ecs::AccessCounters>();
    is_default::<khora_data::ecs::AccessCounters>();
    let _ = type_name::<khora_data::ecs::ComponentRegistry>();
    let _ = khora_data::ecs::ComponentRegistry::get_domain;
    let _ = khora_data::ecs::ComponentRegistry::layout_of;
    let _ = khora_data::ecs::ComponentRegistry::set_layout;
    let _ = khora_data::ecs::ComponentRegistry::record_access;
    let _ = khora_data::ecs::ComponentRegistry::access_stats;
    let _ = khora_data::ecs::ComponentRegistry::size_of;
    let _ = khora_data::ecs::ComponentRegistry::access_snapshot;
    is_debug::<khora_data::ecs::ComponentRegistry>();
    is_default::<khora_data::ecs::ComponentRegistry>();
    let _ = type_name::<khora_data::ecs::ComponentDomainRegistration>();
    let _ =
        component_domain_registration_fields as fn(&khora_data::ecs::ComponentDomainRegistration);
    let _ = type_name::<khora_data::ecs::TypeRegistry>();
    is_debug::<khora_data::ecs::TypeRegistry>();
    is_default::<khora_data::ecs::TypeRegistry>();
    let _ = type_name::<khora_data::ecs::AddComponentError>();
    let _ = add_component_error_variants as fn(&khora_data::ecs::AddComponentError);
    is_debug::<khora_data::ecs::AddComponentError>();
    is_eq::<khora_data::ecs::AddComponentError>();
    is_partial_eq::<khora_data::ecs::AddComponentError>();
    let _ = type_name::<khora_data::ecs::RemoveComponentError>();
    let _ = remove_component_error_variants as fn(&khora_data::ecs::RemoveComponentError);
    is_debug::<khora_data::ecs::RemoveComponentError>();
    is_eq::<khora_data::ecs::RemoveComponentError>();
    is_partial_eq::<khora_data::ecs::RemoveComponentError>();
    let _ = type_name::<khora_data::ecs::DeserializeArchetypeError>();
    let _ = deserialize_archetype_error_variants as fn(&khora_data::ecs::DeserializeArchetypeError);
    is_debug::<khora_data::ecs::DeserializeArchetypeError>();
    is_display::<khora_data::ecs::DeserializeArchetypeError>();
    is_error::<khora_data::ecs::DeserializeArchetypeError>();
    let _ = type_name::<khora_data::ecs::DomainStats>();
    let _ = domain_stats_fields as fn(&khora_data::ecs::DomainStats);
    is_clone::<khora_data::ecs::DomainStats>();
    is_copy::<khora_data::ecs::DomainStats>();
    is_debug::<khora_data::ecs::DomainStats>();
    is_default::<khora_data::ecs::DomainStats>();
    let _ = type_name::<khora_data::ecs::World>();
    let _ = khora_data::ecs::World::new;
    let _ = khora_data::ecs::World::component_domain;
    let _ = khora_data::ecs::World::component_layout;
    let _ = khora_data::ecs::World::component_access_stats;
    let _ = khora_data::ecs::World::entity_count;
    let _ = khora_data::ecs::World::component_access_snapshot;
    let _ = khora_data::ecs::World::domain_epoch;
    let _ = khora_data::ecs::World::instance_id;
    let _ = khora_data::ecs::World::spawn::<khora_data::ecs::Transform>;
    let _ = khora_data::ecs::World::contains;
    let _ = khora_data::ecs::World::set_parent;
    let _ = khora_data::ecs::World::despawn_subtree;
    let _ = khora_data::ecs::World::is_descendant_of;
    let _ = khora_data::ecs::World::despawn;
    let _ = khora_data::ecs::World::query::<&'static khora_data::ecs::Transform>;
    let _ = khora_data::ecs::World::query_mut::<&'static khora_data::ecs::Transform>;
    let _ = khora_data::ecs::World::register_component::<khora_data::ecs::Transform>;
    let _ = khora_data::ecs::World::add_component::<khora_data::ecs::Transform>;
    let _ = khora_data::ecs::World::remove_component::<khora_data::ecs::Transform>;
    let _ = khora_data::ecs::World::remove_component_domain::<khora_data::ecs::Transform>;
    let _ = khora_data::ecs::World::get_mut::<khora_data::ecs::Transform>;
    let _ = khora_data::ecs::World::get_many_mut::<khora_data::ecs::Transform, 2>;
    let _ = khora_data::ecs::World::get::<khora_data::ecs::Transform>;
    let _ = khora_data::ecs::World::clone_component::<khora_data::ecs::Transform>;
    let _ = khora_data::ecs::World::set_component::<khora_data::ecs::Transform>;
    let _ = khora_data::ecs::World::iter_entities;
    let _ = khora_data::ecs::World::serialize_archetype;
    let _ = khora_data::ecs::World::deserialize_archetype;
    is_ui_layout_view::<khora_data::ecs::World>();
    is_default::<khora_data::ecs::World>();
}

#[test]
fn module_flow_paths_still_resolve() {
    let _ = type_name::<khora_data::flow::audio::AudioSourceSnapshot>();
    let _ = type_name::<khora_data::flow::AudioSourceSnapshot>();
    same_type(
        PhantomData::<khora_data::flow::AudioSourceSnapshot>,
        PhantomData::<khora_data::flow::audio::AudioSourceSnapshot>,
    );
    let _ = audio_source_snapshot_fields as fn(&khora_data::flow::audio::AudioSourceSnapshot);
    is_clone::<khora_data::flow::audio::AudioSourceSnapshot>();
    is_debug::<khora_data::flow::audio::AudioSourceSnapshot>();
    let _ = type_name::<khora_data::flow::audio::AudioPlaybackUpdate>();
    let _ = type_name::<khora_data::flow::AudioPlaybackUpdate>();
    same_type(
        PhantomData::<khora_data::flow::AudioPlaybackUpdate>,
        PhantomData::<khora_data::flow::audio::AudioPlaybackUpdate>,
    );
    let _ = audio_playback_update_fields as fn(&khora_data::flow::audio::AudioPlaybackUpdate);
    is_clone::<khora_data::flow::audio::AudioPlaybackUpdate>();
    is_debug::<khora_data::flow::audio::AudioPlaybackUpdate>();
    let _ = type_name::<khora_data::flow::audio::AudioPlaybackWriteback>();
    let _ = type_name::<khora_data::flow::AudioPlaybackWriteback>();
    same_type(
        PhantomData::<khora_data::flow::AudioPlaybackWriteback>,
        PhantomData::<khora_data::flow::audio::AudioPlaybackWriteback>,
    );
    let _ = audio_playback_writeback_fields as fn(&khora_data::flow::audio::AudioPlaybackWriteback);
    is_clone::<khora_data::flow::audio::AudioPlaybackWriteback>();
    is_debug::<khora_data::flow::audio::AudioPlaybackWriteback>();
    is_default::<khora_data::flow::audio::AudioPlaybackWriteback>();
    let _ = type_name::<khora_data::flow::audio::AudioView>();
    let _ = type_name::<khora_data::flow::AudioView>();
    same_type(
        PhantomData::<khora_data::flow::AudioView>,
        PhantomData::<khora_data::flow::audio::AudioView>,
    );
    let _ = audio_view_fields as fn(&khora_data::flow::audio::AudioView);
    is_clone::<khora_data::flow::audio::AudioView>();
    is_debug::<khora_data::flow::audio::AudioView>();
    is_default::<khora_data::flow::audio::AudioView>();
    let _ = type_name::<khora_data::flow::audio::AudioFlow>();
    let _ = type_name::<khora_data::flow::AudioFlow>();
    same_type(
        PhantomData::<khora_data::flow::AudioFlow>,
        PhantomData::<khora_data::flow::audio::AudioFlow>,
    );
    is_default::<khora_data::flow::audio::AudioFlow>();
    let _ = type_name::<khora_data::flow::physics::PhysicsView>();
    let _ = type_name::<khora_data::flow::PhysicsView>();
    same_type(
        PhantomData::<khora_data::flow::PhysicsView>,
        PhantomData::<khora_data::flow::physics::PhysicsView>,
    );
    let _ = physics_view_fields as fn(&khora_data::flow::physics::PhysicsView);
    is_clone::<khora_data::flow::physics::PhysicsView>();
    is_debug::<khora_data::flow::physics::PhysicsView>();
    is_default::<khora_data::flow::physics::PhysicsView>();
    let _ = type_name::<khora_data::flow::physics::PhysicsStepResult>();
    let _ = type_name::<khora_data::flow::PhysicsStepResult>();
    same_type(
        PhantomData::<khora_data::flow::PhysicsStepResult>,
        PhantomData::<khora_data::flow::physics::PhysicsStepResult>,
    );
    let _ = physics_step_result_fields as fn(&khora_data::flow::physics::PhysicsStepResult);
    is_clone::<khora_data::flow::physics::PhysicsStepResult>();
    is_copy::<khora_data::flow::physics::PhysicsStepResult>();
    is_debug::<khora_data::flow::physics::PhysicsStepResult>();
    is_default::<khora_data::flow::physics::PhysicsStepResult>();
    let _ = type_name::<khora_data::flow::physics::PhysicsFlow>();
    let _ = type_name::<khora_data::flow::PhysicsFlow>();
    same_type(
        PhantomData::<khora_data::flow::PhysicsFlow>,
        PhantomData::<khora_data::flow::physics::PhysicsFlow>,
    );
    is_default::<khora_data::flow::physics::PhysicsFlow>();
    let _ = type_name::<khora_data::flow::render::RenderFlow>();
    let _ = type_name::<khora_data::flow::RenderFlow>();
    same_type(
        PhantomData::<khora_data::flow::RenderFlow>,
        PhantomData::<khora_data::flow::render::RenderFlow>,
    );
    is_default::<khora_data::flow::render::RenderFlow>();
    let _ = type_name::<khora_data::flow::script::ScriptProgram>();
    let _ = type_name::<khora_data::flow::ScriptProgram>();
    same_type(
        PhantomData::<khora_data::flow::ScriptProgram>,
        PhantomData::<khora_data::flow::script::ScriptProgram>,
    );
    let _ = script_program_fields as fn(&khora_data::flow::script::ScriptProgram);
    is_clone::<khora_data::flow::script::ScriptProgram>();
    is_debug::<khora_data::flow::script::ScriptProgram>();
    is_eq::<khora_data::flow::script::ScriptProgram>();
    is_hash::<khora_data::flow::script::ScriptProgram>();
    is_partial_eq::<khora_data::flow::script::ScriptProgram>();
    let _ = type_name::<khora_data::flow::script::ScriptInstance>();
    let _ = type_name::<khora_data::flow::ScriptInstance>();
    same_type(
        PhantomData::<khora_data::flow::ScriptInstance>,
        PhantomData::<khora_data::flow::script::ScriptInstance>,
    );
    let _ = script_instance_fields as fn(&khora_data::flow::script::ScriptInstance);
    is_clone::<khora_data::flow::script::ScriptInstance>();
    is_debug::<khora_data::flow::script::ScriptInstance>();
    is_partial_eq::<khora_data::flow::script::ScriptInstance>();
    let _ = type_name::<khora_data::flow::script::ScriptView>();
    let _ = type_name::<khora_data::flow::ScriptView>();
    same_type(
        PhantomData::<khora_data::flow::ScriptView>,
        PhantomData::<khora_data::flow::script::ScriptView>,
    );
    let _ = script_view_fields as fn(&khora_data::flow::script::ScriptView);
    let _ = khora_data::flow::script::ScriptView::program_of;
    let _ = khora_data::flow::script::ScriptView::len;
    let _ = khora_data::flow::script::ScriptView::is_empty;
    is_clone::<khora_data::flow::script::ScriptView>();
    is_debug::<khora_data::flow::script::ScriptView>();
    is_default::<khora_data::flow::script::ScriptView>();
    is_partial_eq::<khora_data::flow::script::ScriptView>();
    let _ = type_name::<khora_data::flow::script::ScriptFlow>();
    let _ = type_name::<khora_data::flow::ScriptFlow>();
    same_type(
        PhantomData::<khora_data::flow::ScriptFlow>,
        PhantomData::<khora_data::flow::script::ScriptFlow>,
    );
    is_default::<khora_data::flow::script::ScriptFlow>();
    let _ = type_name::<khora_data::flow::shadow::ShadowMatrices>();
    let _ = type_name::<khora_data::flow::ShadowMatrices>();
    same_type(
        PhantomData::<khora_data::flow::ShadowMatrices>,
        PhantomData::<khora_data::flow::shadow::ShadowMatrices>,
    );
    let _ = shadow_matrices_variants as fn(&khora_data::flow::shadow::ShadowMatrices);
    is_clone::<khora_data::flow::shadow::ShadowMatrices>();
    is_debug::<khora_data::flow::shadow::ShadowMatrices>();
    let _ = type_name::<khora_data::flow::shadow::ShadowView>();
    let _ = type_name::<khora_data::flow::ShadowView>();
    same_type(
        PhantomData::<khora_data::flow::ShadowView>,
        PhantomData::<khora_data::flow::shadow::ShadowView>,
    );
    let _ = shadow_view_fields as fn(&khora_data::flow::shadow::ShadowView);
    is_clone::<khora_data::flow::shadow::ShadowView>();
    is_debug::<khora_data::flow::shadow::ShadowView>();
    is_default::<khora_data::flow::shadow::ShadowView>();
    let _ = type_name::<khora_data::flow::shadow::ShadowFlow>();
    let _ = type_name::<khora_data::flow::ShadowFlow>();
    same_type(
        PhantomData::<khora_data::flow::ShadowFlow>,
        PhantomData::<khora_data::flow::shadow::ShadowFlow>,
    );
    is_default::<khora_data::flow::shadow::ShadowFlow>();
    let _ = type_name::<khora_data::flow::ui::UiFlow>();
    let _ = type_name::<khora_data::flow::UiFlow>();
    same_type(
        PhantomData::<khora_data::flow::UiFlow>,
        PhantomData::<khora_data::flow::ui::UiFlow>,
    );
    is_default::<khora_data::flow::ui::UiFlow>();
    let _ = type_name::<khora_data::flow::Selection>();
    let _ = selection_fields as fn(&khora_data::flow::Selection);
    let _ = khora_data::flow::Selection::new;
    let _ = khora_data::flow::Selection::from_entities;
    let _ = khora_data::flow::Selection::len;
    let _ = khora_data::flow::Selection::is_empty;
    is_clone::<khora_data::flow::Selection>();
    is_debug::<khora_data::flow::Selection>();
    is_default::<khora_data::flow::Selection>();
    flow_trait_items::<khora_data::flow::AudioFlow>();
    flow_trait_items::<khora_data::flow::PhysicsFlow>();
    flow_trait_items::<khora_data::flow::RenderFlow>();
    flow_trait_items::<khora_data::flow::ScriptFlow>();
    flow_trait_items::<khora_data::flow::ShadowFlow>();
    flow_trait_items::<khora_data::flow::UiFlow>();
    let _ = khora_data::flow::combine_cache_key::<[u64; 2]>;
    let _ = type_name::<khora_data::flow::FlowRegistration>();
    let _ = flow_registration_fields as fn(&khora_data::flow::FlowRegistration);
    let _ = khora_data::flow::run_flow_cached::<khora_data::flow::ScriptFlow>;
}

#[test]
fn module_gpu_paths_still_resolve() {
    let _ = type_name::<khora_data::gpu::eviction::AssetEviction>();
    let _ = type_name::<khora_data::gpu::AssetEviction>();
    let _ = type_name::<khora_data::AssetEviction>();
    same_type(
        PhantomData::<khora_data::gpu::AssetEviction>,
        PhantomData::<khora_data::gpu::eviction::AssetEviction>,
    );
    same_type(
        PhantomData::<khora_data::AssetEviction>,
        PhantomData::<khora_data::gpu::eviction::AssetEviction>,
    );
    let _ = khora_data::gpu::eviction::AssetEviction::new;
    let _ = khora_data::gpu::eviction::AssetEviction::with_budget;
    let _ = khora_data::gpu::eviction::AssetEviction::tick;
    let _ = khora_data::gpu::eviction::AssetEviction::last_evicted_count;
    let _ = khora_data::gpu::eviction::AssetEviction::max_per_frame;
    is_default::<khora_data::gpu::eviction::AssetEviction>();
    let _ = type_name::<khora_data::gpu::ibl::EnvironmentMap>();
    let _ = type_name::<khora_data::gpu::EnvironmentMap>();
    let _ = type_name::<khora_data::EnvironmentMap>();
    same_type(
        PhantomData::<khora_data::gpu::EnvironmentMap>,
        PhantomData::<khora_data::gpu::ibl::EnvironmentMap>,
    );
    same_type(
        PhantomData::<khora_data::EnvironmentMap>,
        PhantomData::<khora_data::gpu::ibl::EnvironmentMap>,
    );
    let _ = environment_map_fields as fn(&khora_data::gpu::ibl::EnvironmentMap);
    let _ = khora_data::gpu::ibl::EnvironmentMap::from_asset;
    is_clone::<khora_data::gpu::ibl::EnvironmentMap>();
    is_debug::<khora_data::gpu::ibl::EnvironmentMap>();
    is_default::<khora_data::gpu::ibl::EnvironmentMap>();
    let _ = type_name::<khora_data::gpu::ibl::IblBaker>();
    let _ = type_name::<khora_data::gpu::IblBaker>();
    let _ = type_name::<khora_data::IblBaker>();
    same_type(
        PhantomData::<khora_data::gpu::IblBaker>,
        PhantomData::<khora_data::gpu::ibl::IblBaker>,
    );
    same_type(
        PhantomData::<khora_data::IblBaker>,
        PhantomData::<khora_data::gpu::ibl::IblBaker>,
    );
    let _ = khora_data::gpu::ibl::IblBaker::new;
    let _ = khora_data::gpu::ibl::IblBaker::is_baked;
    let _ = khora_data::gpu::ibl::IblBaker::wait_for_environment;
    let _ = khora_data::gpu::ibl::IblBaker::ensure_baked;
    let _ = khora_data::gpu::ibl::IblBaker::bindings;
    is_default::<khora_data::gpu::ibl::IblBaker>();
    let _ = type_name::<khora_data::gpu::projection::ProjectionRegistry>();
    let _ = type_name::<khora_data::gpu::ProjectionRegistry>();
    let _ = type_name::<khora_data::ProjectionRegistry>();
    same_type(
        PhantomData::<khora_data::gpu::ProjectionRegistry>,
        PhantomData::<khora_data::gpu::projection::ProjectionRegistry>,
    );
    same_type(
        PhantomData::<khora_data::ProjectionRegistry>,
        PhantomData::<khora_data::gpu::projection::ProjectionRegistry>,
    );
    let _ = khora_data::gpu::projection::ProjectionRegistry::new;
    let _ = khora_data::gpu::projection::ProjectionRegistry::sync_all;
    let _ = khora_data::gpu::projection::ProjectionRegistry::sync_materials;
    is_clone::<khora_data::gpu::projection::ProjectionRegistry>();
}

#[test]
fn module_scene_material_registration_paths_still_resolve() {
    let _ = type_name::<khora_data::scene::material_registration::SerializableMaterialData>();
    same_type(
        PhantomData::<khora_data::scene::SerializableMaterialData>,
        PhantomData::<khora_data::scene::material_registration::SerializableMaterialData>,
    );
    let _ = serializable_material_data_fields
        as fn(&khora_data::scene::material_registration::SerializableMaterialData);
    is_clone::<khora_data::scene::material_registration::SerializableMaterialData>();
    is_debug::<khora_data::scene::material_registration::SerializableMaterialData>();
    let _ = type_name::<khora_data::scene::material_registration::MaterialDeserializeFn>();
    same_type(
        PhantomData::<khora_data::scene::MaterialDeserializeFn>,
        PhantomData::<khora_data::scene::material_registration::MaterialDeserializeFn>,
    );
    let _ = type_name::<khora_data::scene::material_registration::MaterialRegistration>();
    same_type(
        PhantomData::<khora_data::scene::MaterialRegistration>,
        PhantomData::<khora_data::scene::material_registration::MaterialRegistration>,
    );
    let _ = material_registration_fields
        as fn(&khora_data::scene::material_registration::MaterialRegistration);
    same_item(
        &khora_data::scene::serialize_material_component,
        &khora_data::scene::material_registration::serialize_material_component,
    );
    same_item(
        &khora_data::scene::deserialize_material_component,
        &khora_data::scene::material_registration::deserialize_material_component,
    );
    same_item(
        &khora_data::scene::material_to_json,
        &khora_data::scene::material_registration::material_to_json,
    );
    same_item(
        &khora_data::scene::material_from_json,
        &khora_data::scene::material_registration::material_from_json,
    );
}

#[test]
fn module_physics_paths_still_resolve() {
    let _ = type_name::<khora_data::physics::collision::CollisionPair>();
    let _ = type_name::<khora_data::physics::CollisionPair>();
    same_type(
        PhantomData::<khora_data::physics::CollisionPair>,
        PhantomData::<khora_data::physics::collision::CollisionPair>,
    );
    let _ = collision_pair_fields as fn(&khora_data::physics::collision::CollisionPair);
    is_clone::<khora_data::physics::collision::CollisionPair>();
    is_copy::<khora_data::physics::collision::CollisionPair>();
    is_debug::<khora_data::physics::collision::CollisionPair>();
    is_eq::<khora_data::physics::collision::CollisionPair>();
    is_partial_eq::<khora_data::physics::collision::CollisionPair>();
    let _ = type_name::<khora_data::physics::collision::CollisionPairs>();
    let _ = type_name::<khora_data::physics::CollisionPairs>();
    same_type(
        PhantomData::<khora_data::physics::CollisionPairs>,
        PhantomData::<khora_data::physics::collision::CollisionPairs>,
    );
    let _ = collision_pairs_fields as fn(&khora_data::physics::collision::CollisionPairs);
    is_clone::<khora_data::physics::collision::CollisionPairs>();
    is_debug::<khora_data::physics::collision::CollisionPairs>();
    is_default::<khora_data::physics::collision::CollisionPairs>();
}

#[test]
fn module_render_paths_still_resolve() {
    let _ = type_name::<khora_data::render::EditorViewportOverride>();
    let _ = khora_data::render::EditorViewportOverride::new;
    let _ = khora_data::render::EditorViewportOverride::set;
    let _ = khora_data::render::EditorViewportOverride::get;
    let _ = khora_data::render::EditorViewportOverride::clear;
    is_clone::<khora_data::render::EditorViewportOverride>();
    is_default::<khora_data::render::EditorViewportOverride>();
    let _ = khora_data::render::submit_frame_graph;
    let _ = type_name::<khora_data::render::FrameGraph>();
    let _ = khora_data::render::FrameGraph::new;
    let _ = khora_data::render::FrameGraph::add_pass;
    let _ = khora_data::render::FrameGraph::len;
    let _ = khora_data::render::FrameGraph::is_empty;
    let _ = khora_data::render::FrameGraph::clear;
    let _ = khora_data::render::FrameGraph::compile;
    is_default::<khora_data::render::FrameGraph>();
    let _ = type_name::<khora_data::render::OverlayPassSlot>();
    let _ = overlay_pass_slot_fields as fn(&khora_data::render::OverlayPassSlot);
    is_default::<khora_data::render::OverlayPassSlot>();
    let _ = type_name::<khora_data::render::PassContribution>();
    let _ = pass_contribution_fields as fn(&khora_data::render::PassContribution);
    let _ = type_name::<khora_data::render::PassDescriptor>();
    let _ = pass_descriptor_fields as fn(&khora_data::render::PassDescriptor);
    let _ = khora_data::render::PassDescriptor::new;
    let _ = khora_data::render::PassDescriptor::reads;
    let _ = khora_data::render::PassDescriptor::writes;
    is_clone::<khora_data::render::PassDescriptor>();
    is_debug::<khora_data::render::PassDescriptor>();
    let _ = type_name::<khora_data::render::ResourceId>();
    let _ = resource_id_variants as fn(&khora_data::render::ResourceId);
    is_clone::<khora_data::render::ResourceId>();
    is_copy::<khora_data::render::ResourceId>();
    is_debug::<khora_data::render::ResourceId>();
    is_eq::<khora_data::render::ResourceId>();
    is_hash::<khora_data::render::ResourceId>();
    is_partial_eq::<khora_data::render::ResourceId>();
    let _ = type_name::<khora_data::render::ScenePassSlot>();
    let _ = scene_pass_slot_fields as fn(&khora_data::render::ScenePassSlot);
    is_default::<khora_data::render::ScenePassSlot>();
    let _ = type_name::<khora_data::render::SharedFrameGraph>();
    let _ = type_name::<khora_data::render::SharedGizmoFrame>();
    same_type(
        PhantomData::<khora_data::render::SharedGizmoFrame>,
        PhantomData::<std::sync::Arc<std::sync::Mutex<khora_data::render::GizmoFrame>>>,
    );
    let _ = type_name::<khora_data::render::SharedGridConfig>();
    same_type(
        PhantomData::<khora_data::render::SharedGridConfig>,
        PhantomData::<std::sync::Arc<std::sync::Mutex<khora_data::render::GridConfig>>>,
    );
    let _ = type_name::<khora_data::render::SharedWireframeConfig>();
    same_type(
        PhantomData::<khora_data::render::SharedWireframeConfig>,
        PhantomData::<std::sync::Arc<std::sync::Mutex<khora_data::render::WireframeConfig>>>,
    );
    let _ = type_name::<khora_data::render::SkyboxPassSlot>();
    let _ = skybox_pass_slot_fields as fn(&khora_data::render::SkyboxPassSlot);
    is_default::<khora_data::render::SkyboxPassSlot>();
    let _ = type_name::<khora_data::render::TransparentEncoder>();
    let _ = type_name::<khora_data::render::TransparentPassSlot>();
    let _ = transparent_pass_slot_fields as fn(&khora_data::render::TransparentPassSlot);
    is_default::<khora_data::render::TransparentPassSlot>();
    let _ = type_name::<khora_data::render::UiPassSlot>();
    let _ = ui_pass_slot_fields as fn(&khora_data::render::UiPassSlot);
    is_default::<khora_data::render::UiPassSlot>();
    let _ = type_name::<khora_data::render::GizmoFrame>();
    let _ = gizmo_frame_fields as fn(&khora_data::render::GizmoFrame);
    let _ = khora_data::render::GizmoFrame::is_empty;
    let _ = khora_data::render::GizmoFrame::len;
    let _ = khora_data::render::GizmoFrame::clear;
    is_clone::<khora_data::render::GizmoFrame>();
    is_debug::<khora_data::render::GizmoFrame>();
    is_default::<khora_data::render::GizmoFrame>();
    let _ = type_name::<khora_data::render::GridConfig>();
    let _ = grid_config_fields as fn(&khora_data::render::GridConfig);
    is_clone::<khora_data::render::GridConfig>();
    is_debug::<khora_data::render::GridConfig>();
    is_default::<khora_data::render::GridConfig>();
    let _ = type_name::<khora_data::render::WireframeConfig>();
    let _ = wireframe_config_fields as fn(&khora_data::render::WireframeConfig);
    is_clone::<khora_data::render::WireframeConfig>();
    is_debug::<khora_data::render::WireframeConfig>();
    is_default::<khora_data::render::WireframeConfig>();
    let _ = type_name::<khora_data::render::ExtractedLight>();
    let _ = extracted_light_fields as fn(&khora_data::render::ExtractedLight);
    is_clone::<khora_data::render::ExtractedLight>();
    is_debug::<khora_data::render::ExtractedLight>();
    let _ = type_name::<khora_data::render::ExtractedMesh>();
    let _ = extracted_mesh_fields as fn(&khora_data::render::ExtractedMesh);
    is_clone::<khora_data::render::ExtractedMesh>();
    let _ = type_name::<khora_data::render::ExtractedView>();
    let _ = extracted_view_fields as fn(&khora_data::render::ExtractedView);
    is_clone::<khora_data::render::ExtractedView>();
    is_debug::<khora_data::render::ExtractedView>();
    let _ = type_name::<khora_data::render::RenderWorld>();
    let _ = render_world_fields as fn(&khora_data::render::RenderWorld);
    let _ = khora_data::render::RenderWorld::new;
    let _ = khora_data::render::RenderWorld::clear;
    let _ = khora_data::render::RenderWorld::directional_light_count;
    let _ = khora_data::render::RenderWorld::point_light_count;
    let _ = khora_data::render::RenderWorld::spot_light_count;
    is_clone::<khora_data::render::RenderWorld>();
    is_default::<khora_data::render::RenderWorld>();
    let _ = type_name::<khora_data::render::ShadowResult>();
    let _ = khora_data::render::extract_active_camera_view;
    let _ = khora_data::render::primary_view;
}

fn record_error_fields(x: &khora_data::scene::record::RecordError) {
    let _ = (&x.0,);
}

fn record_variants(x: &khora_data::scene::record::Record) {
    match x {
        khora_data::scene::record::Record::Unit => {}
        khora_data::scene::record::Record::Bool(..) => {}
        khora_data::scene::record::Record::I64(..) => {}
        khora_data::scene::record::Record::U64(..) => {}
        khora_data::scene::record::Record::F32(..) => {}
        khora_data::scene::record::Record::F64(..) => {}
        khora_data::scene::record::Record::Char(..) => {}
        khora_data::scene::record::Record::Str(..) => {}
        khora_data::scene::record::Record::Bytes(..) => {}
        khora_data::scene::record::Record::None => {}
        khora_data::scene::record::Record::Some(..) => {}
        khora_data::scene::record::Record::Seq(..) => {}
        khora_data::scene::record::Record::Map(..) => {}
        khora_data::scene::record::Record::UnitStruct { name } => {
            let _ = name;
        }
        khora_data::scene::record::Record::Struct { name, fields } => {
            let _ = (name, fields);
        }
        khora_data::scene::record::Record::TupleStruct { name, fields } => {
            let _ = (name, fields);
        }
        khora_data::scene::record::Record::Newtype { name, value } => {
            let _ = (name, value);
        }
        khora_data::scene::record::Record::Variant {
            enum_name,
            variant,
            payload,
        } => {
            let _ = (enum_name, variant, payload);
        }
        khora_data::scene::record::Record::Entity(..) => {}
        khora_data::scene::record::Record::Asset(..) => {}
    }
}

fn variant_payload_variants(x: &khora_data::scene::record::VariantPayload) {
    match x {
        khora_data::scene::record::VariantPayload::Unit => {}
        khora_data::scene::record::VariantPayload::Newtype(..) => {}
        khora_data::scene::record::VariantPayload::Tuple(..) => {}
        khora_data::scene::record::VariantPayload::Struct(..) => {}
    }
}

fn entity_ref_variants(x: &khora_data::scene::record::EntityRef) {
    match x {
        khora_data::scene::record::EntityRef::Id(..) => {}
        khora_data::scene::record::EntityRef::Outside => {}
    }
}

fn load_report_fields(x: &khora_data::scene::record::LoadReport) {
    let _ = (&x.entries,);
}

fn report_entry_fields(x: &khora_data::scene::record::ReportEntry) {
    let _ = (&x.entity, &x.component, &x.path, &x.kind);
}

fn report_kind_variants(x: &khora_data::scene::record::ReportKind) {
    match x {
        khora_data::scene::record::ReportKind::Defaulted => {}
        khora_data::scene::record::ReportKind::Dropped => {}
        khora_data::scene::record::ReportKind::Renamed { from } => {
            let _ = from;
        }
        khora_data::scene::record::ReportKind::Widened => {}
        khora_data::scene::record::ReportKind::Retired => {}
        khora_data::scene::record::ReportKind::DeadReference => {}
    }
}

fn reference_writer_trait_items<W: khora_data::scene::record::ReferenceWriter + ?Sized>() {
    let _ = <W as khora_data::scene::record::ReferenceWriter>::write_entity;
}

fn reference_reader_trait_items<R: khora_data::scene::record::ReferenceReader + ?Sized>() {
    let _ = <R as khora_data::scene::record::ReferenceReader>::read_entity;
}

#[test]
fn module_scene_record_paths_still_resolve() {
    let _ = type_name::<khora_data::scene::record::Record>();
    let _ = record_variants as fn(&khora_data::scene::record::Record);
    is_debug::<khora_data::scene::record::Record>();
    is_clone::<khora_data::scene::record::Record>();
    is_partial_eq::<khora_data::scene::record::Record>();
    let _ = type_name::<khora_data::scene::record::VariantPayload>();
    let _ = variant_payload_variants as fn(&khora_data::scene::record::VariantPayload);
    is_debug::<khora_data::scene::record::VariantPayload>();
    is_clone::<khora_data::scene::record::VariantPayload>();
    is_partial_eq::<khora_data::scene::record::VariantPayload>();
    let _ = type_name::<khora_data::scene::record::EntityRef>();
    let _ = entity_ref_variants as fn(&khora_data::scene::record::EntityRef);
    is_debug::<khora_data::scene::record::EntityRef>();
    is_clone::<khora_data::scene::record::EntityRef>();
    is_copy::<khora_data::scene::record::EntityRef>();
    is_eq::<khora_data::scene::record::EntityRef>();
    is_hash::<khora_data::scene::record::EntityRef>();
    let _ = type_name::<khora_data::scene::record::RecordError>();
    let _ = record_error_fields as fn(&khora_data::scene::record::RecordError);
    is_debug::<khora_data::scene::record::RecordError>();
    is_clone::<khora_data::scene::record::RecordError>();
    is_partial_eq::<khora_data::scene::record::RecordError>();
    is_display::<khora_data::scene::record::RecordError>();
    is_error::<khora_data::scene::record::RecordError>();
    let _ = type_name::<khora_data::scene::record::LoadReport>();
    let _ = load_report_fields as fn(&khora_data::scene::record::LoadReport);
    let _ = khora_data::scene::record::LoadReport::is_clean;
    is_debug::<khora_data::scene::record::LoadReport>();
    is_clone::<khora_data::scene::record::LoadReport>();
    is_default::<khora_data::scene::record::LoadReport>();
    is_partial_eq::<khora_data::scene::record::LoadReport>();
    let _ = type_name::<khora_data::scene::record::ReportEntry>();
    let _ = report_entry_fields as fn(&khora_data::scene::record::ReportEntry);
    is_debug::<khora_data::scene::record::ReportEntry>();
    is_clone::<khora_data::scene::record::ReportEntry>();
    is_partial_eq::<khora_data::scene::record::ReportEntry>();
    let _ = type_name::<khora_data::scene::record::ReportKind>();
    let _ = report_kind_variants as fn(&khora_data::scene::record::ReportKind);
    is_debug::<khora_data::scene::record::ReportKind>();
    is_clone::<khora_data::scene::record::ReportKind>();
    is_partial_eq::<khora_data::scene::record::ReportKind>();
    reference_writer_trait_items::<dyn khora_data::scene::record::ReferenceWriter>();
    reference_reader_trait_items::<dyn khora_data::scene::record::ReferenceReader>();
    let _ = khora_data::scene::record::to_record::<khora_core::ecs::entity::EntityId>;
    let _ = khora_data::scene::record::from_record::<khora_core::ecs::entity::EntityId>;
    let _ = khora_data::scene::record::resolve::<khora_core::ecs::entity::EntityId>;
}

#[test]
fn module_scene_paths_still_resolve() {
    let _ = type_name::<khora_data::scene::component_registration::ComponentRegistration>();
    let _ = type_name::<khora_data::scene::ComponentRegistration>();
    same_type(
        PhantomData::<khora_data::scene::ComponentRegistration>,
        PhantomData::<khora_data::scene::component_registration::ComponentRegistration>,
    );
    let _ = component_registration_fields
        as fn(&khora_data::scene::component_registration::ComponentRegistration);
    let _ = khora_data::scene::component_registration::serialize_all_components;
    let _ = khora_data::scene::serialize_all_components;
    same_item(
        &khora_data::scene::serialize_all_components,
        &khora_data::scene::component_registration::serialize_all_components,
    );
    let _ = khora_data::scene::component_registration::link_parent_child;
    let _ = khora_data::scene::link_parent_child;
    same_item(
        &khora_data::scene::link_parent_child,
        &khora_data::scene::component_registration::link_parent_child,
    );
    let _ = khora_data::scene::component_registration::registration_of;
    let _ = khora_data::scene::registration_of;
    same_item(
        &khora_data::scene::registration_of,
        &khora_data::scene::component_registration::registration_of,
    );
    let _ = khora_data::scene::component_registration::provenance_of;
    let _ = khora_data::scene::provenance_of;
    same_item(
        &khora_data::scene::provenance_of,
        &khora_data::scene::component_registration::provenance_of,
    );
    let _ = type_name::<khora_data::scene::shape::ComponentShape>();
    let _ = type_name::<khora_data::scene::ComponentShape>();
    same_type(
        PhantomData::<khora_data::scene::ComponentShape>,
        PhantomData::<khora_data::scene::shape::ComponentShape>,
    );
    let _ = component_shape_variants as fn(&khora_data::scene::shape::ComponentShape);
    let _ = khora_data::scene::shape::ComponentShape::fields;
    is_clone::<khora_data::scene::shape::ComponentShape>();
    is_copy::<khora_data::scene::shape::ComponentShape>();
    is_debug::<khora_data::scene::shape::ComponentShape>();
    is_eq::<khora_data::scene::shape::ComponentShape>();
    is_partial_eq::<khora_data::scene::shape::ComponentShape>();
    let _ = type_name::<khora_data::scene::shape::FieldSchema>();
    let _ = type_name::<khora_data::scene::FieldSchema>();
    same_type(
        PhantomData::<khora_data::scene::FieldSchema>,
        PhantomData::<khora_data::scene::shape::FieldSchema>,
    );
    let _ = field_schema_fields as fn(&khora_data::scene::shape::FieldSchema);
    is_clone::<khora_data::scene::shape::FieldSchema>();
    is_copy::<khora_data::scene::shape::FieldSchema>();
    is_debug::<khora_data::scene::shape::FieldSchema>();
    is_eq::<khora_data::scene::shape::FieldSchema>();
    is_partial_eq::<khora_data::scene::shape::FieldSchema>();
    let _ = type_name::<dyn khora_data::scene::migrations::SceneMigration>();
    let _ = type_name::<dyn khora_data::scene::SceneMigration>();
    same_type(
        PhantomData::<dyn khora_data::scene::SceneMigration>,
        PhantomData::<dyn khora_data::scene::migrations::SceneMigration>,
    );
    scene_migration_trait_items::<dyn khora_data::scene::SceneMigration>();
    let _ = type_name::<khora_data::scene::migrations::MigrationError>();
    let _ = type_name::<khora_data::scene::MigrationError>();
    same_type(
        PhantomData::<khora_data::scene::MigrationError>,
        PhantomData::<khora_data::scene::migrations::MigrationError>,
    );
    let _ = migration_error_variants as fn(&khora_data::scene::migrations::MigrationError);
    is_debug::<khora_data::scene::migrations::MigrationError>();
    is_display::<khora_data::scene::migrations::MigrationError>();
    let _ = type_name::<khora_data::scene::migrations::SceneMigrationRegistration>();
    let _ = type_name::<khora_data::scene::SceneMigrationRegistration>();
    same_type(
        PhantomData::<khora_data::scene::SceneMigrationRegistration>,
        PhantomData::<khora_data::scene::migrations::SceneMigrationRegistration>,
    );
    let _ = scene_migration_registration_fields
        as fn(&khora_data::scene::migrations::SceneMigrationRegistration);
    let _ = khora_data::scene::migrations::migrate_payload;
    let _ = khora_data::scene::migrate_payload;
    same_item(
        &khora_data::scene::migrate_payload,
        &khora_data::scene::migrations::migrate_payload,
    );
    let _ = type_name::<khora_data::scene::SceneRecipe>();
    let _ = scene_recipe_fields as fn(&khora_data::scene::SceneRecipe);
    is_debug::<khora_data::scene::SceneRecipe>();
    let _ = type_name::<khora_data::scene::SceneCommand>();
    let _ = scene_command_variants as fn(&khora_data::scene::SceneCommand);
    is_debug::<khora_data::scene::SceneCommand>();
    let _ = type_name::<khora_data::scene::ArchetypeSerializationStrategy>();
    let _ = khora_data::scene::ArchetypeSerializationStrategy::new;
    is_default::<khora_data::scene::ArchetypeSerializationStrategy>();
    let _ = type_name::<khora_data::scene::ComponentDefinition>();
    let _ = component_definition_fields as fn(&khora_data::scene::ComponentDefinition);
    is_clone::<khora_data::scene::ComponentDefinition>();
    is_debug::<khora_data::scene::ComponentDefinition>();
    let _ = type_name::<khora_data::scene::EntityDefinition>();
    let _ = entity_definition_fields as fn(&khora_data::scene::EntityDefinition);
    is_clone::<khora_data::scene::EntityDefinition>();
    is_debug::<khora_data::scene::EntityDefinition>();
    let _ = type_name::<khora_data::scene::SceneDefinition>();
    let _ = scene_definition_fields as fn(&khora_data::scene::SceneDefinition);
    is_clone::<khora_data::scene::SceneDefinition>();
    is_debug::<khora_data::scene::SceneDefinition>();
    let _ = type_name::<khora_data::scene::DefinitionSerializationStrategy>();
    let _ = khora_data::scene::DefinitionSerializationStrategy::new;
    is_default::<khora_data::scene::DefinitionSerializationStrategy>();
    let _ = type_name::<khora_data::scene::MessagePackSerializationStrategy>();
    let _ = khora_data::scene::MessagePackSerializationStrategy::new;
    is_default::<khora_data::scene::MessagePackSerializationStrategy>();
    let _ = type_name::<khora_data::scene::RecipeSerializationStrategy>();
    let _ = khora_data::scene::RecipeSerializationStrategy::new;
    is_default::<khora_data::scene::RecipeSerializationStrategy>();
    let _ = khora_data::scene::serialize_subtree;
    let _ = khora_data::scene::instantiate_subtree;
    let _ = type_name::<khora_data::scene::SerializationError>();
    let _ = serialization_error_variants as fn(&khora_data::scene::SerializationError);
    is_debug::<khora_data::scene::SerializationError>();
    is_display::<khora_data::scene::SerializationError>();
    let _ = type_name::<khora_data::scene::DeserializationError>();
    let _ = deserialization_error_variants as fn(&khora_data::scene::DeserializationError);
    is_debug::<khora_data::scene::DeserializationError>();
    is_display::<khora_data::scene::DeserializationError>();
    let _ = type_name::<dyn khora_data::scene::SerializationStrategy>();
    serialization_strategy_trait_items::<khora_data::scene::ArchetypeSerializationStrategy>();
    serialization_strategy_trait_items::<khora_data::scene::DefinitionSerializationStrategy>();
    serialization_strategy_trait_items::<khora_data::scene::MessagePackSerializationStrategy>();
    serialization_strategy_trait_items::<khora_data::scene::RecipeSerializationStrategy>();
}

#[test]
fn module_ui_paths_still_resolve() {
    same_type(
        PhantomData::<khora_data::ui::components::UiFlexDirection>,
        PhantomData::<khora_core::ui::types::UiFlexDirection>,
    );
    same_type(
        PhantomData::<khora_data::ui::UiFlexDirection>,
        PhantomData::<khora_core::ui::types::UiFlexDirection>,
    );
    same_type(
        PhantomData::<khora_data::ui::components::UiRect<f32>>,
        PhantomData::<khora_core::ui::types::UiRect<f32>>,
    );
    same_type(
        PhantomData::<khora_data::ui::UiRect<f32>>,
        PhantomData::<khora_core::ui::types::UiRect<f32>>,
    );
    same_type(
        PhantomData::<khora_data::ui::components::UiVal>,
        PhantomData::<khora_core::ui::types::UiVal>,
    );
    same_type(
        PhantomData::<khora_data::ui::UiVal>,
        PhantomData::<khora_core::ui::types::UiVal>,
    );
    let _ = type_name::<khora_data::ui::components::UiNode>();
    let _ = type_name::<khora_data::ui::UiNode>();
    let _ = type_name::<khora_data::UiNode>();
    same_type(
        PhantomData::<khora_data::ui::UiNode>,
        PhantomData::<khora_data::ui::components::UiNode>,
    );
    same_type(
        PhantomData::<khora_data::UiNode>,
        PhantomData::<khora_data::ui::components::UiNode>,
    );
    let _ = ui_node_fields as fn(&khora_data::ui::components::UiNode);
    is_clone::<khora_data::ui::components::UiNode>();
    is_component::<khora_data::ui::components::UiNode>();
    is_debug::<khora_data::ui::components::UiNode>();
    is_default::<khora_data::ui::components::UiNode>();
    is_partial_eq::<khora_data::ui::components::UiNode>();
    let _ = type_name::<khora_data::ui::components::UiTransform>();
    let _ = type_name::<khora_data::ui::UiTransform>();
    let _ = type_name::<khora_data::UiTransform>();
    same_type(
        PhantomData::<khora_data::ui::UiTransform>,
        PhantomData::<khora_data::ui::components::UiTransform>,
    );
    same_type(
        PhantomData::<khora_data::UiTransform>,
        PhantomData::<khora_data::ui::components::UiTransform>,
    );
    let _ = ui_transform_fields as fn(&khora_data::ui::components::UiTransform);
    let _ = khora_data::ui::components::UiTransform::rect;
    let _ = khora_data::ui::components::UiTransform::contains;
    is_clone::<khora_data::ui::components::UiTransform>();
    is_component::<khora_data::ui::components::UiTransform>();
    is_copy::<khora_data::ui::components::UiTransform>();
    is_debug::<khora_data::ui::components::UiTransform>();
    is_default::<khora_data::ui::components::UiTransform>();
    is_partial_eq::<khora_data::ui::components::UiTransform>();
    let _ = type_name::<khora_data::ui::components::UiStyle>();
    let _ = type_name::<khora_data::ui::UiStyle>();
    let _ = type_name::<khora_data::UiStyle>();
    same_type(
        PhantomData::<khora_data::ui::UiStyle>,
        PhantomData::<khora_data::ui::components::UiStyle>,
    );
    same_type(
        PhantomData::<khora_data::UiStyle>,
        PhantomData::<khora_data::ui::components::UiStyle>,
    );
    let _ = ui_style_fields as fn(&khora_data::ui::components::UiStyle);
    is_clone::<khora_data::ui::components::UiStyle>();
    is_component::<khora_data::ui::components::UiStyle>();
    is_debug::<khora_data::ui::components::UiStyle>();
    is_default::<khora_data::ui::components::UiStyle>();
    is_partial_eq::<khora_data::ui::components::UiStyle>();
    let _ = type_name::<khora_data::ui::components::UiColor>();
    let _ = type_name::<khora_data::ui::UiColor>();
    let _ = type_name::<khora_data::UiColor>();
    same_type(
        PhantomData::<khora_data::ui::UiColor>,
        PhantomData::<khora_data::ui::components::UiColor>,
    );
    same_type(
        PhantomData::<khora_data::UiColor>,
        PhantomData::<khora_data::ui::components::UiColor>,
    );
    let _ = ui_color_fields as fn(&khora_data::ui::components::UiColor);
    let _ = khora_data::ui::components::UiColor::to_core;
    is_clone::<khora_data::ui::components::UiColor>();
    is_component::<khora_data::ui::components::UiColor>();
    is_copy::<khora_data::ui::components::UiColor>();
    is_debug::<khora_data::ui::components::UiColor>();
    is_default::<khora_data::ui::components::UiColor>();
    is_partial_eq::<khora_data::ui::components::UiColor>();
    let _ = type_name::<khora_data::ui::components::UiImage>();
    let _ = type_name::<khora_data::ui::UiImage>();
    let _ = type_name::<khora_data::UiImage>();
    same_type(
        PhantomData::<khora_data::ui::UiImage>,
        PhantomData::<khora_data::ui::components::UiImage>,
    );
    same_type(
        PhantomData::<khora_data::UiImage>,
        PhantomData::<khora_data::ui::components::UiImage>,
    );
    let _ = ui_image_fields as fn(&khora_data::ui::components::UiImage);
    let _ = khora_data::ui::components::UiImage::to_core;
    is_clone::<khora_data::ui::components::UiImage>();
    is_component::<khora_data::ui::components::UiImage>();
    is_copy::<khora_data::ui::components::UiImage>();
    is_debug::<khora_data::ui::components::UiImage>();
    is_default::<khora_data::ui::components::UiImage>();
    is_partial_eq::<khora_data::ui::components::UiImage>();
    let _ = type_name::<khora_data::ui::components::UiBorder>();
    let _ = type_name::<khora_data::ui::UiBorder>();
    let _ = type_name::<khora_data::UiBorder>();
    same_type(
        PhantomData::<khora_data::ui::UiBorder>,
        PhantomData::<khora_data::ui::components::UiBorder>,
    );
    same_type(
        PhantomData::<khora_data::UiBorder>,
        PhantomData::<khora_data::ui::components::UiBorder>,
    );
    let _ = ui_border_fields as fn(&khora_data::ui::components::UiBorder);
    let _ = khora_data::ui::components::UiBorder::to_core;
    is_clone::<khora_data::ui::components::UiBorder>();
    is_component::<khora_data::ui::components::UiBorder>();
    is_copy::<khora_data::ui::components::UiBorder>();
    is_debug::<khora_data::ui::components::UiBorder>();
    is_default::<khora_data::ui::components::UiBorder>();
    is_partial_eq::<khora_data::ui::components::UiBorder>();
    let _ = type_name::<khora_data::ui::components::UiInteractionState>();
    let _ = type_name::<khora_data::ui::UiInteractionState>();
    let _ = type_name::<khora_data::UiInteractionState>();
    same_type(
        PhantomData::<khora_data::ui::UiInteractionState>,
        PhantomData::<khora_data::ui::components::UiInteractionState>,
    );
    same_type(
        PhantomData::<khora_data::UiInteractionState>,
        PhantomData::<khora_data::ui::components::UiInteractionState>,
    );
    let _ = ui_interaction_state_variants as fn(&khora_data::ui::components::UiInteractionState);
    is_clone::<khora_data::ui::components::UiInteractionState>();
    is_copy::<khora_data::ui::components::UiInteractionState>();
    is_debug::<khora_data::ui::components::UiInteractionState>();
    is_default::<khora_data::ui::components::UiInteractionState>();
    is_eq::<khora_data::ui::components::UiInteractionState>();
    is_partial_eq::<khora_data::ui::components::UiInteractionState>();
    let _ = type_name::<khora_data::ui::components::UiInteraction>();
    let _ = type_name::<khora_data::ui::UiInteraction>();
    let _ = type_name::<khora_data::UiInteraction>();
    same_type(
        PhantomData::<khora_data::ui::UiInteraction>,
        PhantomData::<khora_data::ui::components::UiInteraction>,
    );
    same_type(
        PhantomData::<khora_data::UiInteraction>,
        PhantomData::<khora_data::ui::components::UiInteraction>,
    );
    let _ = ui_interaction_fields as fn(&khora_data::ui::components::UiInteraction);
    is_clone::<khora_data::ui::components::UiInteraction>();
    is_component::<khora_data::ui::components::UiInteraction>();
    is_debug::<khora_data::ui::components::UiInteraction>();
    is_default::<khora_data::ui::components::UiInteraction>();
    let _ = type_name::<khora_data::ui::components::UiText>();
    let _ = type_name::<khora_data::ui::UiText>();
    let _ = type_name::<khora_data::UiText>();
    same_type(
        PhantomData::<khora_data::ui::UiText>,
        PhantomData::<khora_data::ui::components::UiText>,
    );
    same_type(
        PhantomData::<khora_data::UiText>,
        PhantomData::<khora_data::ui::components::UiText>,
    );
    let _ = ui_text_fields as fn(&khora_data::ui::components::UiText);
    let _ = khora_data::ui::components::UiText::to_core;
    is_clone::<khora_data::ui::components::UiText>();
    is_component::<khora_data::ui::components::UiText>();
    is_debug::<khora_data::ui::components::UiText>();
    is_default::<khora_data::ui::components::UiText>();
    is_partial_eq::<khora_data::ui::components::UiText>();
    let _ = type_name::<khora_data::ui::image_atlas::UiImageAtlas>();
    let _ = type_name::<khora_data::ui::UiImageAtlas>();
    same_type(
        PhantomData::<khora_data::ui::UiImageAtlas>,
        PhantomData::<khora_data::ui::image_atlas::UiImageAtlas>,
    );
    let _ = khora_data::ui::image_atlas::UiImageAtlas::new;
    let _ = khora_data::ui::image_atlas::UiImageAtlas::ensure_atlas;
    let _ = khora_data::ui::image_atlas::UiImageAtlas::lock_atlas;
    let _ = khora_data::ui::image_atlas::UiImageAtlas::get_rect;
    let _ = khora_data::ui::image_atlas::UiImageAtlas::insert_rect;
    let _ = khora_data::ui::image_atlas::UiImageAtlas::has_atlas;
    let _ = khora_data::ui::image_atlas::UiImageAtlas::cache_len;
    is_default::<khora_data::ui::image_atlas::UiImageAtlas>();
    let _ = type_name::<khora_data::ui::ExtractedUiNode>();
    let _ = extracted_ui_node_fields as fn(&khora_data::ui::ExtractedUiNode);
    is_clone::<khora_data::ui::ExtractedUiNode>();
    is_debug::<khora_data::ui::ExtractedUiNode>();
    let _ = type_name::<khora_data::ui::ExtractedUiText>();
    let _ = extracted_ui_text_fields as fn(&khora_data::ui::ExtractedUiText);
    is_clone::<khora_data::ui::ExtractedUiText>();
    let _ = type_name::<khora_data::ui::UiAtlasMap>();
    let _ = ui_atlas_map_fields as fn(&khora_data::ui::UiAtlasMap);
    let _ = khora_data::ui::UiAtlasMap::new;
    let _ = khora_data::ui::UiAtlasMap::get;
    let _ = khora_data::ui::UiAtlasMap::insert;
    is_clone::<khora_data::ui::UiAtlasMap>();
    is_debug::<khora_data::ui::UiAtlasMap>();
    is_default::<khora_data::ui::UiAtlasMap>();
    let _ = type_name::<khora_data::ui::UiScene>();
    let _ = ui_scene_fields as fn(&khora_data::ui::UiScene);
    let _ = khora_data::ui::UiScene::new;
    is_clone::<khora_data::ui::UiScene>();
    is_default::<khora_data::ui::UiScene>();
}

#[test]
fn crate_root_paths_still_resolve() {
    // The crate-root `pub use gpu::{..}` and `pub use ui::components::*` are
    // checked beside their items, in the `gpu` and `ui` tests; what is left
    // here are the root's re-exports of `khora_core` and the exported macro.
    same_type(
        PhantomData::<khora_data::UiFlexDirection>,
        PhantomData::<khora_core::ui::types::UiFlexDirection>,
    );
    same_type(
        PhantomData::<khora_data::UiRect<f32>>,
        PhantomData::<khora_core::ui::types::UiRect<f32>>,
    );
    same_type(
        PhantomData::<khora_data::UiVal>,
        PhantomData::<khora_core::ui::types::UiVal>,
    );
    // `register_flow!` is `#[macro_export]`ed at the crate root; the probe flow
    // below is registered through it, and its registration must be collected.
    assert!(
        inventory::iter::<khora_data::flow::FlowRegistration>
            .into_iter()
            .any(|r| r.name == <ProbeFlow as khora_data::flow::Flow>::NAME),
        "register_flow! no longer registers through khora_data::flow::FlowRegistration"
    );
}

// ---------------------------------------------------------------------------
// Paths other crates of the workspace use today (`crates/`, `examples/`,
// `xtask/`, brace imports included; the editor's and the
// sandbox's `khora_sdk::khora_data::...` spellings reach the same items through
// the SDK's `pub use khora_data;`). The trailing comment names the users.
// ---------------------------------------------------------------------------

fn is_component_via_ecs<T: khora_data::ecs::Component>() {}

#[test]
fn paths_used_by_other_crates_still_resolve() {
    let _ = khora_data::AssetEviction::new; // khora-sdk
    let _ = type_name::<khora_data::AssetStore>(); // khora-agents, khora-io, sandbox
    let _ = khora_data::AssetStore::new; // khora-sdk
    let _ = type_name::<khora_data::EnvironmentMap>(); // khora-sdk
    let _ = type_name::<khora_data::IblBaker>(); // khora-agents
    let _ = khora_data::IblBaker::new; // khora-sdk
    let _ = khora_data::ProjectionRegistry::new; // khora-sdk
    let _ = type_name::<khora_data::assets::Assets<khora_data::assets::SoundData>>(); // khora-agents, khora-io, khora-lanes
    let _ = type_name::<khora_data::assets::SoundData>(); // khora-io, khora-lanes, khora-sdk
    let _ = type_name::<khora_data::ecs::AudioSource>(); // khora-sdk
    let _ = type_name::<khora_data::ecs::BodyMotion>(); // khora-agents
    let _ = type_name::<khora_data::ecs::Camera>(); // khora-agents, khora-sdk
    let _ = type_name::<khora_data::ecs::Children>(); // khora-sdk
    let _ = type_name::<khora_data::ecs::Collider>(); // khora-sdk
    let _ = khora_data::ecs::Collider::new_box; // khora-agents
    let _ = khora_data::ecs::Collider::new_sphere; // khora-agents
    is_component_via_ecs::<khora_data::ecs::Transform>(); // khora-agents, khora-sdk
    component_bundle_trait_items::<khora_data::ecs::Transform>(); // khora-sdk
    let _ = type_name::<khora_data::ecs::DataSystemRegistration>(); // khora-control, khora-io
    let _ = type_name::<khora_data::ecs::EcsMaintenance>(); // khora-agents
    let _ = khora_data::ecs::EcsMaintenance::new; // khora-sdk
    let _ = type_name::<khora_data::ecs::GlobalTransform>(); // khora-agents, khora-io, khora-sdk
    let _ = khora_data::ecs::GlobalTransform::at_position; // khora-agents
    let _ = khora_data::ecs::GlobalTransform::default; // khora-agents
    let _ = type_name::<khora_data::ecs::HandleComponent<khora_data::assets::SoundData>>(); // khora-agents, khora-io, khora-sdk
    let _ = type_name::<khora_data::ecs::KinematicCharacterController>(); // khora-agents
    let _ = type_name::<khora_data::ecs::Light>(); // khora-agents, khora-sdk
    let _ = type_name::<khora_data::ecs::MaterialRef>(); // khora-io, khora-sdk
    let _ = khora_data::ecs::MaterialRef::inline; // khora-sdk
    let _ = type_name::<khora_data::scene::MaterialRegistration>(); // khora-io
    let _ = type_name::<khora_data::ecs::MeshRef>(); // khora-io, khora-sdk
    let _ = type_name::<khora_data::ecs::Name>(); // khora-sdk
    let _ = type_name::<khora_data::ecs::Parent>(); // khora-agents, khora-io, khora-sdk
    let _ = type_name::<khora_data::ecs::PlaybackState>(); // khora-lanes
    let _ = type_name::<khora_data::ecs::ProceduralMeshKind>(); // khora-io, khora-sdk
    let _ = type_name::<khora_data::ecs::ProjectionType>(); // khora-sdk
    let _ = type_name::<khora_data::ecs::Query<'static, &'static khora_data::ecs::Transform>>(); // khora-sdk
    let _ =
        type_name::<khora_data::ecs::QueryMut<'static, &'static mut khora_data::ecs::Transform>>(); // khora-sdk
    let _ = type_name::<khora_data::ecs::RigidBody>(); // khora-agents, khora-sdk
    let _ = type_name::<khora_data::ecs::Script>(); // khora-sdk
    let _ = type_name::<khora_data::ecs::SemanticDomain>(); // khora-agents, khora-editor
    let _ = type_name::<khora_data::ecs::SimulatedTransform>(); // khora-agents
    let _ = type_name::<khora_data::ecs::Tag>(); // khora-editor, khora-sdk
    let _ = type_name::<khora_data::ecs::Teleported>(); // khora-agents, khora-editor
    let _ = type_name::<khora_data::ecs::TickPhase>(); // khora-agents, khora-control, khora-io, khora-sdk
    let _ = khora_data::ecs::TickPhase::Maintenance; // khora-agents
    let _ = khora_data::ecs::TickPhase::PostSimulation; // khora-agents
    let _ = khora_data::ecs::TickPhase::PreExtract; // khora-agents
    let _ = khora_data::ecs::TickPhase::PreSimulation; // khora-agents
    let _ = type_name::<khora_data::ecs::Transform>(); // khora-agents, khora-io, khora-sdk
    let _ = type_name::<khora_data::ecs::Without<khora_data::ecs::Transform>>(); // khora-io, khora-sdk
    let _ = type_name::<khora_data::ecs::World>(); // khora-agents, khora-control, khora-io, khora-sdk
    world_query_trait_items::<&'static khora_data::ecs::Transform>(); // khora-sdk
    let _ = type_name::<khora_data::ecs::layout::LayoutAdvisor>(); // khora-control
    let _ = type_name::<khora_data::ecs::layout::LayoutRecommendation>(); // khora-control
    let _ = khora_data::scene::material_to_json; // khora-editor, khora-io
    let _ = khora_data::ecs::reconstruct_procedural_mesh; // khora-io
    let _ = khora_data::ecs::systems::collision_to_script::TOUCHED; // khora-agents
    let _ = type_name::<khora_data::flow::AudioPlaybackUpdate>(); // khora-lanes
    let _ = type_name::<khora_data::flow::AudioPlaybackWriteback>(); // khora-lanes
    let _ = type_name::<khora_data::flow::AudioSourceSnapshot>(); // khora-lanes
    let _ = type_name::<khora_data::flow::AudioView>(); // khora-agents, khora-lanes
    flow_trait_items::<khora_data::flow::ScriptFlow>(); // khora-agents
    let _ = type_name::<khora_data::flow::FlowRegistration>(); // khora-control
    let _ = type_name::<khora_data::flow::PhysicsStepResult>(); // khora-lanes
    let _ = type_name::<khora_data::flow::ScriptFlow>(); // khora-agents
    let _ = type_name::<khora_data::flow::ScriptInstance>(); // khora-agents, khora-lanes
    let _ = type_name::<khora_data::flow::ScriptProgram>(); // khora-agents, khora-lanes
    let _ = type_name::<khora_data::flow::ScriptView>(); // khora-agents, khora-lanes
    let _ = khora_data::flow::Selection::new; // khora-agents
    let _ = type_name::<khora_data::flow::ShadowMatrices>(); // khora-agents, khora-lanes
    let _ = type_name::<khora_data::flow::ShadowView>(); // khora-agents, khora-lanes
    let _ = type_name::<khora_data::render::EditorViewportOverride>(); // khora-editor
    let _ = khora_data::render::EditorViewportOverride::new; // khora-editor
    let _ = type_name::<khora_data::render::ExtractedLight>(); // khora-lanes
    let _ = type_name::<khora_data::render::ExtractedMesh>(); // khora-lanes
    let _ = type_name::<khora_data::render::ExtractedView>(); // khora-editor, khora-lanes
    let _ = type_name::<khora_data::render::FrameGraph>(); // khora-sdk
    let _ = type_name::<khora_data::render::GizmoFrame>(); // khora-lanes
    let _ = khora_data::render::GizmoFrame::default; // khora-sdk
    let _ = type_name::<khora_data::render::GridConfig>(); // khora-lanes
    let _ = khora_data::render::GridConfig::default; // khora-sdk
    let _ = type_name::<khora_data::render::OverlayPassSlot>(); // khora-agents, khora-sdk
    let _ = type_name::<khora_data::render::PassContribution>(); // khora-agents
    let _ = type_name::<khora_data::render::PassDescriptor>(); // khora-agents
    let _ = type_name::<khora_data::render::RenderWorld>(); // khora-agents, khora-lanes
    let _ = type_name::<khora_data::render::ResourceId>(); // khora-agents
    let _ = type_name::<khora_data::render::ScenePassSlot>(); // khora-agents, khora-sdk
    let _ = type_name::<khora_data::render::SharedFrameGraph>(); // khora-sdk
    let _ = type_name::<khora_data::render::SharedGizmoFrame>(); // khora-agents, khora-editor (as khora_sdk::khora_data::…), khora-lanes, khora-sdk
    let _ = type_name::<khora_data::render::SharedGridConfig>(); // khora-agents, khora-editor (as khora_sdk::khora_data::…), khora-lanes, khora-sdk
    let _ = type_name::<khora_data::render::SharedWireframeConfig>(); // khora-agents, khora-editor (as khora_sdk::khora_data::…), khora-lanes, khora-sdk
    let _ = type_name::<khora_data::render::SkyboxPassSlot>(); // khora-agents, khora-sdk
    let _ = type_name::<khora_data::render::TransparentEncoder>(); // khora-agents, khora-lanes
    let _ = type_name::<khora_data::render::TransparentPassSlot>(); // khora-agents, khora-sdk
    let _ = type_name::<khora_data::render::UiPassSlot>(); // khora-agents, khora-sdk
    let _ = type_name::<khora_data::render::WireframeConfig>(); // khora-lanes
    let _ = khora_data::render::WireframeConfig::default; // khora-sdk
    let _ = khora_data::render::extract_active_camera_view; // khora-agents, khora-editor
    let _ = khora_data::render::submit_frame_graph; // khora-sdk
    let _ = type_name::<khora_data::scene::ArchetypeSerializationStrategy>(); // khora-io
    let _ = type_name::<khora_data::scene::ComponentRegistration>(); // khora-io, khora-sdk
    let _ = type_name::<khora_data::scene::ComponentShape>(); // khora-io
    let _ = type_name::<khora_data::scene::DefinitionSerializationStrategy>(); // khora-io
    let _ = type_name::<khora_data::scene::DeserializationError>(); // khora-io
    let _ = type_name::<khora_data::scene::MessagePackSerializationStrategy>(); // khora-io
    let _ = type_name::<khora_data::scene::MigrationError>(); // khora-io
    let _ = type_name::<khora_data::scene::RecipeSerializationStrategy>(); // khora-io
    let _ = type_name::<dyn khora_data::scene::SceneMigration>(); // khora-io
    let _ = type_name::<khora_data::scene::SceneMigrationRegistration>(); // khora-io
    let _ = type_name::<khora_data::scene::SerializationError>(); // khora-io
    let _ = type_name::<dyn khora_data::scene::SerializationStrategy>(); // khora-io
    let _ = khora_data::scene::instantiate_subtree; // khora-sdk
    let _ = khora_data::scene::migrate_payload; // khora-io
    let _ = khora_data::scene::provenance_of; // khora-editor
    let _ = khora_data::scene::serialize_subtree; // khora-sdk
    let _ = type_name::<khora_data::ui::UiAtlasMap>(); // khora-agents, khora-lanes
    let _ = type_name::<khora_data::ui::UiImageAtlas>(); // khora-agents
    let _ = khora_data::ui::UiImageAtlas::new; // khora-sdk
    let _ = type_name::<khora_data::ui::UiScene>(); // khora-agents, khora-lanes

    // Named by doc links and by `docs/src`.
    let _ = type_name::<khora_data::ecs::ComponentProvenance>(); // khora-editor (doc link)
    let _ = type_name::<khora_data::ecs::AudioListener>(); // docs/src
    let _ = type_name::<khora_data::ui::UiText>(); // docs/src
    let _ = khora_data::flow::combine_cache_key::<[u64; 2]>; // docs/src
}

// ---------------------------------------------------------------------------
// `#[derive(Component)]` emits a `#[doc(hidden)]` `pub struct SerializableX`
// mirror next to each component, with `From` both ways. Rustdoc does not list
// them, but they are public paths through the same glob re-exports.
// ---------------------------------------------------------------------------

fn is_from<A: From<B>, B: From<A>>() {}

#[test]
fn derive_generated_mirrors_still_resolve() {
    let _ = type_name::<khora_data::ecs::SerializableAudioListener>();
    is_from::<khora_data::ecs::AudioListener, khora_data::ecs::SerializableAudioListener>();
    let _ = type_name::<khora_data::ecs::SerializableAudioSource>();
    is_from::<khora_data::ecs::AudioSource, khora_data::ecs::SerializableAudioSource>();
    let _ = type_name::<khora_data::ecs::SerializableCamera>();
    is_from::<khora_data::ecs::Camera, khora_data::ecs::SerializableCamera>();
    let _ = type_name::<khora_data::ecs::SerializableChildren>();
    is_from::<khora_data::ecs::Children, khora_data::ecs::SerializableChildren>();
    let _ = type_name::<khora_data::ecs::SerializableGlobalTransform>();
    is_from::<khora_data::ecs::GlobalTransform, khora_data::ecs::SerializableGlobalTransform>();
    let _ = type_name::<khora_data::ecs::SerializableLight>();
    is_from::<khora_data::ecs::Light, khora_data::ecs::SerializableLight>();
    let _ = type_name::<khora_data::ecs::SerializableName>();
    is_from::<khora_data::ecs::Name, khora_data::ecs::SerializableName>();
    let _ = type_name::<khora_data::ecs::SerializableParent>();
    is_from::<khora_data::ecs::Parent, khora_data::ecs::SerializableParent>();
    let _ = type_name::<khora_data::ecs::SerializableActiveEvents>();
    is_from::<khora_data::ecs::ActiveEvents, khora_data::ecs::SerializableActiveEvents>();
    let _ = type_name::<khora_data::ecs::SerializableBodyMotion>();
    is_from::<khora_data::ecs::BodyMotion, khora_data::ecs::SerializableBodyMotion>();
    let _ = type_name::<khora_data::ecs::SerializableCollider>();
    is_from::<khora_data::ecs::Collider, khora_data::ecs::SerializableCollider>();
    let _ = type_name::<khora_data::ecs::SerializableKinematicCharacterController>();
    is_from::<
        khora_data::ecs::KinematicCharacterController,
        khora_data::ecs::SerializableKinematicCharacterController,
    >();
    let _ = type_name::<khora_data::ecs::SerializablePhysicsDebugData>();
    is_from::<khora_data::ecs::PhysicsDebugData, khora_data::ecs::SerializablePhysicsDebugData>();
    let _ = type_name::<khora_data::ecs::SerializablePhysicsMaterial>();
    is_from::<khora_data::ecs::PhysicsMaterial, khora_data::ecs::SerializablePhysicsMaterial>();
    let _ = type_name::<khora_data::ecs::SerializableRigidBody>();
    is_from::<khora_data::ecs::RigidBody, khora_data::ecs::SerializableRigidBody>();
    let _ = type_name::<khora_data::ecs::SerializableScript>();
    is_from::<khora_data::ecs::Script, khora_data::ecs::SerializableScript>();
    let _ = type_name::<khora_data::ecs::SerializableSimulatedTransform>();
    is_from::<khora_data::ecs::SimulatedTransform, khora_data::ecs::SerializableSimulatedTransform>(
    );
    let _ = type_name::<khora_data::ecs::SerializableTag>();
    is_from::<khora_data::ecs::Tag, khora_data::ecs::SerializableTag>();
    let _ = type_name::<khora_data::ecs::SerializableTeleported>();
    is_from::<khora_data::ecs::Teleported, khora_data::ecs::SerializableTeleported>();
    let _ = type_name::<khora_data::ecs::SerializableTransform>();
    is_from::<khora_data::ecs::Transform, khora_data::ecs::SerializableTransform>();
    let _ = type_name::<khora_data::ui::components::SerializableUiNode>();
    let _ = type_name::<khora_data::ui::SerializableUiNode>();
    let _ = type_name::<khora_data::SerializableUiNode>();
    is_from::<khora_data::ui::components::UiNode, khora_data::ui::components::SerializableUiNode>();
    let _ = type_name::<khora_data::ui::components::SerializableUiTransform>();
    let _ = type_name::<khora_data::ui::SerializableUiTransform>();
    let _ = type_name::<khora_data::SerializableUiTransform>();
    is_from::<
        khora_data::ui::components::UiTransform,
        khora_data::ui::components::SerializableUiTransform,
    >();
    let _ = type_name::<khora_data::ui::components::SerializableUiStyle>();
    let _ = type_name::<khora_data::ui::SerializableUiStyle>();
    let _ = type_name::<khora_data::SerializableUiStyle>();
    is_from::<khora_data::ui::components::UiStyle, khora_data::ui::components::SerializableUiStyle>(
    );
    let _ = type_name::<khora_data::ui::components::SerializableUiColor>();
    let _ = type_name::<khora_data::ui::SerializableUiColor>();
    let _ = type_name::<khora_data::SerializableUiColor>();
    is_from::<khora_data::ui::components::UiColor, khora_data::ui::components::SerializableUiColor>(
    );
    let _ = type_name::<khora_data::ui::components::SerializableUiImage>();
    let _ = type_name::<khora_data::ui::SerializableUiImage>();
    let _ = type_name::<khora_data::SerializableUiImage>();
    is_from::<khora_data::ui::components::UiImage, khora_data::ui::components::SerializableUiImage>(
    );
    let _ = type_name::<khora_data::ui::components::SerializableUiBorder>();
    let _ = type_name::<khora_data::ui::SerializableUiBorder>();
    let _ = type_name::<khora_data::SerializableUiBorder>();
    is_from::<khora_data::ui::components::UiBorder, khora_data::ui::components::SerializableUiBorder>(
    );
    let _ = type_name::<khora_data::ui::components::SerializableUiInteraction>();
    let _ = type_name::<khora_data::ui::SerializableUiInteraction>();
    let _ = type_name::<khora_data::SerializableUiInteraction>();
    is_from::<
        khora_data::ui::components::UiInteraction,
        khora_data::ui::components::SerializableUiInteraction,
    >();
    let _ = type_name::<khora_data::ui::components::SerializableUiText>();
    let _ = type_name::<khora_data::ui::SerializableUiText>();
    let _ = type_name::<khora_data::SerializableUiText>();
    is_from::<khora_data::ui::components::UiText, khora_data::ui::components::SerializableUiText>();
}
