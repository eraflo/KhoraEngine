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

//! Compile-level guard over `khora_sdk`'s public surface, the prelude, plus the paths
//! other crates of the workspace use.
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
//! The list was generated from `khora_sdk`'s rustdoc JSON, so it is complete for the
//! tree it was written against. Nothing is constructed; the tests only have to
//! type-check.
//!
//! The re-exported crates (`khora_core`, `khora_data`, `khora_io`,
//! `khora_lanes`, `khora_agents`, `winit`, `inventory`) are checked to be the
//! dependency itself; the globs and module re-exports of `khora_core`
//! (`editor_ui`, `prelude::math`, `renderer::*`) item by item. A minimal
//! `EngineApp` implemented through the `khora_sdk::` paths pins the traits a
//! game implements.

use std::any::type_name;
use std::marker::PhantomData;

/// Compiles only when both arguments name the same type.
fn same_type<T: ?Sized>(_: PhantomData<T>, _: PhantomData<T>) {}

/// Compiles only when both references point at the same item (two distinct
/// `fn` items never share a type, and `&fn-item` does not coerce).
fn same_item<T>(_: &T, _: &T) {}

fn is_application_handler<T: winit::application::ApplicationHandler>() {}
fn is_clone<T: Clone>() {}
fn is_debug<T: std::fmt::Debug>() {}
fn is_default<T: Default>() {}
fn is_window_provider<T: khora_sdk::WindowProvider>() {}

// ---------------------------------------------------------------------------
// Every `pub mod`, down to the leaves.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod every_pub_mod {
    use khora_sdk::editor_ui as _;
    use khora_sdk::prelude as _;
    use khora_sdk::prelude::ecs as _;
    use khora_sdk::prelude::materials as _;
    use khora_sdk::prelude::math as _;
    use khora_sdk::renderer as _;
    use khora_sdk::scripts as _;
    use khora_sdk::tool_ui as _;
    use khora_sdk::winit_adapters as _;
}

// ---------------------------------------------------------------------------
// Public fields (read through a reference) and enum variants (matched
// exhaustively). Nothing is constructed.
// ---------------------------------------------------------------------------

fn window_config_fields(x: &khora_sdk::WindowConfig) {
    let _ = (&x.title, &x.width, &x.height, &x.icon);
}

fn window_icon_fields(x: &khora_sdk::WindowIcon) {
    let _ = (&x.rgba, &x.width, &x.height);
}

fn runtime_config_fields(x: &khora_sdk::RuntimeConfig) {
    let _: &String = &x.project_name;
    let _: &String = &x.default_scene;
    let _: &Option<String> = &x.window_title;
    let _: &Option<String> = &x.preset;
    let _: &bool = &x.verify_integrity;
}

// ---------------------------------------------------------------------------
// Trait items, named through a generic parameter. A generic body is
// type-checked where it is written, so these need no implementor and are
// never instantiated. A re-exported trait is checked for identity by
// forwarding a bound both ways.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
mod trait_items {
    fn agent_provider_trait_items<T: khora_sdk::AgentProvider>() {
        let _ = <T as khora_sdk::AgentProvider>::register_agents;
    }

    fn khora_sdk_asset_io_same<T: ?Sized + khora_sdk::AssetIo>() {
        khora_sdk_asset_io_same_rev::<T>();
    }

    fn khora_sdk_asset_io_same_rev<T: ?Sized + khora_io::asset::AssetIo>() {
        khora_sdk_asset_io_same::<T>();
    }

    fn khora_sdk_asset_writer_same<T: ?Sized + khora_sdk::AssetWriter>() {
        khora_sdk_asset_writer_same_rev::<T>();
    }

    fn khora_sdk_asset_writer_same_rev<T: ?Sized + khora_io::asset::AssetWriter>() {
        khora_sdk_asset_writer_same::<T>();
    }

    fn khora_sdk_audio_device_same<T: ?Sized + khora_sdk::AudioDevice>() {
        khora_sdk_audio_device_same_rev::<T>();
    }

    fn khora_sdk_audio_device_same_rev<T: ?Sized + khora_core::audio::AudioDevice>() {
        khora_sdk_audio_device_same::<T>();
    }

    fn khora_sdk_audio_mix_bus_same<T: ?Sized + khora_sdk::AudioMixBus>() {
        khora_sdk_audio_mix_bus_same_rev::<T>();
    }

    fn khora_sdk_audio_mix_bus_same_rev<T: ?Sized + khora_core::audio::AudioMixBus>() {
        khora_sdk_audio_mix_bus_same::<T>();
    }

    fn khora_sdk_audio_stream_same<T: ?Sized + khora_sdk::AudioStream>() {
        khora_sdk_audio_stream_same_rev::<T>();
    }

    fn khora_sdk_audio_stream_same_rev<T: ?Sized + khora_core::audio::AudioStream>() {
        khora_sdk_audio_stream_same::<T>();
    }

    fn khora_sdk_editor_panel_same<T: ?Sized + khora_sdk::EditorPanel>() {
        khora_sdk_editor_panel_same_rev::<T>();
    }

    fn khora_sdk_editor_panel_same_rev<T: ?Sized + khora_core::ui::EditorPanel>() {
        khora_sdk_editor_panel_same::<T>();
    }

    fn khora_sdk_editor_shell_same<T: ?Sized + khora_sdk::EditorShell>() {
        khora_sdk_editor_shell_same_rev::<T>();
    }

    fn khora_sdk_editor_shell_same_rev<T: ?Sized + khora_core::ui::EditorShell>() {
        khora_sdk_editor_shell_same::<T>();
    }

    fn engine_app_trait_items<T: khora_sdk::EngineApp>() {
        let _ = <T as khora_sdk::EngineApp>::window_config;
        let _ = <T as khora_sdk::EngineApp>::new;
        let _ = <T as khora_sdk::EngineApp>::setup;
        let _ = <T as khora_sdk::EngineApp>::update;
        let _ = <T as khora_sdk::EngineApp>::on_shutdown;
        let _ = <T as khora_sdk::EngineApp>::intercept_window_event;
        let _ = <T as khora_sdk::EngineApp>::before_frame;
        let _ = <T as khora_sdk::EngineApp>::before_agents;
        let _ = <T as khora_sdk::EngineApp>::after_agents;
    }

    fn khora_sdk_layout_system_same<T: ?Sized + khora_sdk::LayoutSystem>() {
        khora_sdk_layout_system_same_rev::<T>();
    }

    fn khora_sdk_layout_system_same_rev<T: ?Sized + khora_core::ui::LayoutSystem>() {
        khora_sdk_layout_system_same::<T>();
    }

    fn phase_provider_trait_items<T: khora_sdk::PhaseProvider>() {
        let _ = <T as khora_sdk::PhaseProvider>::custom_phases;
        let _ = <T as khora_sdk::PhaseProvider>::removed_phases;
    }

    fn khora_sdk_physics_provider_same<T: ?Sized + khora_sdk::PhysicsProvider>() {
        khora_sdk_physics_provider_same_rev::<T>();
    }

    fn khora_sdk_physics_provider_same_rev<T: ?Sized + khora_core::physics::PhysicsProvider>() {
        khora_sdk_physics_provider_same::<T>();
    }

    fn khora_sdk_pipeline_system_same<T: ?Sized + khora_sdk::PipelineSystem>() {
        khora_sdk_pipeline_system_same_rev::<T>();
    }

    fn khora_sdk_pipeline_system_same_rev<
        T: ?Sized + khora_core::renderer::traits::PipelineSystem,
    >() {
        khora_sdk_pipeline_system_same::<T>();
    }

    fn khora_sdk_render_system_same<T: ?Sized + khora_sdk::RenderSystem>() {
        khora_sdk_render_system_same_rev::<T>();
    }

    fn khora_sdk_render_system_same_rev<T: ?Sized + khora_core::renderer::RenderSystem>() {
        khora_sdk_render_system_same::<T>();
    }

    fn khora_sdk_text_renderer_same<T: ?Sized + khora_sdk::TextRenderer>() {
        khora_sdk_text_renderer_same_rev::<T>();
    }

    fn khora_sdk_text_renderer_same_rev<
        T: ?Sized + khora_core::renderer::api::text::TextRenderer,
    >() {
        khora_sdk_text_renderer_same::<T>();
    }

    fn khora_sdk_tool_ui_ui_builder_same<T: ?Sized + khora_sdk::tool_ui::UiBuilder>() {
        khora_sdk_tool_ui_ui_builder_same_rev::<T>();
    }

    fn khora_sdk_tool_ui_ui_builder_same_rev<T: ?Sized + khora_core::ui::UiBuilder>() {
        khora_sdk_tool_ui_ui_builder_same::<T>();
    }

    fn khora_sdk_ui_builder_same<T: ?Sized + khora_sdk::UiBuilder>() {
        khora_sdk_ui_builder_same_rev::<T>();
    }

    fn khora_sdk_ui_builder_same_rev<T: ?Sized + khora_core::ui::UiBuilder>() {
        khora_sdk_ui_builder_same::<T>();
    }

    fn window_provider_trait_items<T: khora_sdk::WindowProvider>() {
        let _ = <T as khora_sdk::WindowProvider>::create;
        let _ = <T as khora_sdk::WindowProvider>::request_redraw;
        let _ = <T as khora_sdk::WindowProvider>::inner_size;
        let _ = <T as khora_sdk::WindowProvider>::scale_factor;
        let _ = <T as khora_sdk::WindowProvider>::as_khora_window;
        let _ = <T as khora_sdk::WindowProvider>::translate_event;
        let _ = <T as khora_sdk::WindowProvider>::clone_raw_window_arc;
    }

    fn khora_sdk_tool_ui_app_same<T: ?Sized + khora_sdk::tool_ui::App>() {
        khora_sdk_tool_ui_app_same_rev::<T>();
    }

    fn khora_sdk_tool_ui_app_same_rev<T: ?Sized + khora_core::ui::App>() {
        khora_sdk_tool_ui_app_same::<T>();
    }

    fn khora_sdk_tool_ui_app_context_same<T: ?Sized + khora_sdk::tool_ui::AppContext>() {
        khora_sdk_tool_ui_app_context_same_rev::<T>();
    }

    fn khora_sdk_tool_ui_app_context_same_rev<T: ?Sized + khora_core::ui::AppContext>() {
        khora_sdk_tool_ui_app_context_same::<T>();
    }

    fn khora_sdk_tool_ui_app_lifecycle_same<T: ?Sized + khora_sdk::tool_ui::AppLifecycle>() {
        khora_sdk_tool_ui_app_lifecycle_same_rev::<T>();
    }

    fn khora_sdk_tool_ui_app_lifecycle_same_rev<T: ?Sized + khora_core::ui::AppLifecycle>() {
        khora_sdk_tool_ui_app_lifecycle_same::<T>();
    }

    fn khora_sdk_prelude_ecs_component_same<T: khora_sdk::prelude::ecs::Component>() {
        khora_sdk_prelude_ecs_component_same_rev::<T>();
    }

    fn khora_sdk_prelude_ecs_component_same_rev<T: khora_data::ecs::Component>() {
        khora_sdk_prelude_ecs_component_same::<T>();
    }

    fn khora_sdk_prelude_ecs_component_bundle_same<
        T: ?Sized + khora_sdk::prelude::ecs::ComponentBundle,
    >() {
        khora_sdk_prelude_ecs_component_bundle_same_rev::<T>();
    }

    fn khora_sdk_prelude_ecs_component_bundle_same_rev<
        T: ?Sized + khora_data::ecs::ComponentBundle,
    >() {
        khora_sdk_prelude_ecs_component_bundle_same::<T>();
    }
}

#[test]
fn module_crate_root_paths_still_resolve() {
    let _ = type_name::<khora_sdk::AgentHints>();
    same_type(
        PhantomData::<khora_sdk::AgentHints>,
        PhantomData::<khora_core::agent::gorna::AgentHints>,
    );
    let _ = type_name::<khora_sdk::AgentId>();
    same_type(
        PhantomData::<khora_sdk::AgentId>,
        PhantomData::<khora_core::agent::gorna::AgentId>,
    );
    let _ = type_name::<khora_sdk::AgentImportance>();
    same_type(
        PhantomData::<khora_sdk::AgentImportance>,
        PhantomData::<khora_core::agent::AgentImportance>,
    );
    // trait `khora_sdk::AgentProvider`: see `agent_provider_trait_items`
    let _ = type_name::<khora_sdk::AgentRegistry>();
    same_type(
        PhantomData::<khora_sdk::AgentRegistry>,
        PhantomData::<khora_control::AgentRegistry>,
    );
    let _ = type_name::<khora_sdk::AgentStatus>();
    same_type(
        PhantomData::<khora_sdk::AgentStatus>,
        PhantomData::<khora_core::agent::gorna::AgentStatus>,
    );
    let _ = type_name::<khora_sdk::AssetChangeEvent>();
    same_type(
        PhantomData::<khora_sdk::AssetChangeEvent>,
        PhantomData::<khora_io::asset::AssetChangeEvent>,
    );
    let _ = type_name::<khora_sdk::AssetChangeKind>();
    same_type(
        PhantomData::<khora_sdk::AssetChangeKind>,
        PhantomData::<khora_io::asset::AssetChangeKind>,
    );
    let _ = type_name::<khora_sdk::AssetEntry>();
    same_type(
        PhantomData::<khora_sdk::AssetEntry>,
        PhantomData::<khora_core::ui::editor::AssetEntry>,
    );
    let _ = type_name::<khora_sdk::AssetIdRegistry>();
    same_type(
        PhantomData::<khora_sdk::AssetIdRegistry>,
        PhantomData::<khora_io::asset::AssetIdRegistry>,
    );
    // trait `khora_sdk::AssetIo` is `khora_io::asset::AssetIo`: see `trait_items::khora_sdk_asset_io_same`
    let _ = type_name::<khora_sdk::AssetService>();
    same_type(
        PhantomData::<khora_sdk::AssetService>,
        PhantomData::<khora_io::asset::AssetService>,
    );
    let _ = type_name::<khora_sdk::AssetSource>();
    same_type(
        PhantomData::<khora_sdk::AssetSource>,
        PhantomData::<khora_core::asset::AssetSource>,
    );
    let _ = type_name::<khora_sdk::AssetWatcher>();
    same_type(
        PhantomData::<khora_sdk::AssetWatcher>,
        PhantomData::<khora_io::asset::AssetWatcher>,
    );
    // trait `khora_sdk::AssetWriter` is `khora_io::asset::AssetWriter`: see `trait_items::khora_sdk_asset_writer_same`
    // trait `khora_sdk::AudioDevice` is `khora_core::audio::AudioDevice`: see `trait_items::khora_sdk_audio_device_same`
    // trait `khora_sdk::AudioMixBus` is `khora_core::audio::AudioMixBus`: see `trait_items::khora_sdk_audio_mix_bus_same`
    // trait `khora_sdk::AudioStream` is `khora_core::audio::AudioStream`: see `trait_items::khora_sdk_audio_stream_same`
    let _ = type_name::<khora_sdk::Backends>();
    same_type(
        PhantomData::<khora_sdk::Backends>,
        PhantomData::<khora_core::Backends>,
    );
    let _ = type_name::<khora_sdk::ComponentJson>();
    same_type(
        PhantomData::<khora_sdk::ComponentJson>,
        PhantomData::<khora_core::ui::editor::ComponentJson>,
    );
    let _ = type_name::<khora_sdk::ComponentRegistration>();
    same_type(
        PhantomData::<khora_sdk::ComponentRegistration>,
        PhantomData::<khora_data::scene::ComponentRegistration>,
    );
    let _ = type_name::<khora_sdk::CpalAudioDevice>();
    same_type(
        PhantomData::<khora_sdk::CpalAudioDevice>,
        PhantomData::<khora_infra::audio::cpal::CpalAudioDevice>,
    );
    let _ = type_name::<khora_sdk::DccConfig>();
    same_type(
        PhantomData::<khora_sdk::DccConfig>,
        PhantomData::<khora_control::DccConfig>,
    );
    let _ = type_name::<khora_sdk::DccContext>();
    same_type(
        PhantomData::<khora_sdk::DccContext>,
        PhantomData::<khora_control::Context>,
    );
    let _ = type_name::<khora_sdk::DccService>();
    same_type(
        PhantomData::<khora_sdk::DccService>,
        PhantomData::<khora_control::DccService>,
    );
    let _ = type_name::<khora_sdk::DefaultMixBus>();
    same_type(
        PhantomData::<khora_sdk::DefaultMixBus>,
        PhantomData::<khora_infra::audio::DefaultMixBus>,
    );
    let _ = type_name::<khora_sdk::EditorMode>();
    same_type(
        PhantomData::<khora_sdk::EditorMode>,
        PhantomData::<khora_core::ui::editor::EditorMode>,
    );
    // trait `khora_sdk::EditorPanel` is `khora_core::ui::EditorPanel`: see `trait_items::khora_sdk_editor_panel_same`
    // trait `khora_sdk::EditorShell` is `khora_core::ui::EditorShell`: see `trait_items::khora_sdk_editor_shell_same`
    let _ = type_name::<khora_sdk::EditorState>();
    same_type(
        PhantomData::<khora_sdk::EditorState>,
        PhantomData::<khora_core::ui::editor::EditorState>,
    );
    // trait `khora_sdk::EngineApp`: see `engine_app_trait_items`
    let _ = type_name::<khora_sdk::EngineCore<ProbeApp>>();
    let _ = <khora_sdk::EngineCore<ProbeApp>>::new;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::bootstrap;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::feed_input;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::tick;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::tick_with_runtime;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::run_maintenance;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::drain_inputs;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::run_app_update;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::begin_render_frame;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::run_scheduler;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::submit_passes;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::present_frame;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::end_render_frame;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::app_mut;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::with_app_and_world::<
        fn(&mut ProbeApp, &mut khora_sdk::GameWorld),
    >;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::set_runtime;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::runtime;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::game_world_mut;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::dcc;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::set_engine_hint;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::clear_agent_hints;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::engine_hints;
    let _ = <khora_sdk::EngineCore<ProbeApp>>::shutdown;
    is_default::<khora_sdk::EngineCore<ProbeApp>>();
    let _ = type_name::<khora_sdk::EngineHint>();
    same_type(
        PhantomData::<khora_sdk::EngineHint>,
        PhantomData::<khora_core::agent::gorna::EngineHint>,
    );
    let _ = type_name::<khora_sdk::EngineMode>();
    same_type(
        PhantomData::<khora_sdk::EngineMode>,
        PhantomData::<khora_core::agent::EngineMode>,
    );
    let _ = type_name::<khora_sdk::EntityIcon>();
    same_type(
        PhantomData::<khora_sdk::EntityIcon>,
        PhantomData::<khora_core::ui::editor::EntityIcon>,
    );
    let _ = type_name::<khora_sdk::EnvironmentMap>();
    same_type(
        PhantomData::<khora_sdk::EnvironmentMap>,
        PhantomData::<khora_data::EnvironmentMap>,
    );
    let _ = type_name::<khora_sdk::ExecutionPhase>();
    same_type(
        PhantomData::<khora_sdk::ExecutionPhase>,
        PhantomData::<khora_core::agent::ExecutionPhase>,
    );
    let _ = type_name::<khora_sdk::ExecutionTiming>();
    same_type(
        PhantomData::<khora_sdk::ExecutionTiming>,
        PhantomData::<khora_core::agent::ExecutionTiming>,
    );
    let _ = type_name::<khora_sdk::FileLoader>();
    same_type(
        PhantomData::<khora_sdk::FileLoader>,
        PhantomData::<khora_io::asset::FileLoader>,
    );
    let _ = type_name::<khora_sdk::FileSystemResolver>();
    same_type(
        PhantomData::<khora_sdk::FileSystemResolver>,
        PhantomData::<khora_io::asset::FileSystemResolver>,
    );
    let _ = type_name::<khora_sdk::tool_ui::FontFamilyHint>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::FontFamilyHint>,
        PhantomData::<khora_core::ui::editor::FontFamilyHint>,
    );
    let _ = type_name::<khora_sdk::FontFamilyHint>();
    same_type(
        PhantomData::<khora_sdk::FontFamilyHint>,
        PhantomData::<khora_core::ui::editor::FontFamilyHint>,
    );
    let _ = type_name::<khora_sdk::editor_ui::FontHandle>();
    same_type(
        PhantomData::<khora_sdk::editor_ui::FontHandle>,
        PhantomData::<khora_core::ui::FontHandle>,
    );
    let _ = type_name::<khora_sdk::tool_ui::FontHandle>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::FontHandle>,
        PhantomData::<khora_core::ui::FontHandle>,
    );
    let _ = type_name::<khora_sdk::FontHandle>();
    same_type(
        PhantomData::<khora_sdk::FontHandle>,
        PhantomData::<khora_core::ui::FontHandle>,
    );
    let _ = type_name::<khora_sdk::editor_ui::FontPack>();
    same_type(
        PhantomData::<khora_sdk::editor_ui::FontPack>,
        PhantomData::<khora_core::ui::FontPack>,
    );
    let _ = type_name::<khora_sdk::tool_ui::FontPack>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::FontPack>,
        PhantomData::<khora_core::ui::FontPack>,
    );
    let _ = type_name::<khora_sdk::FontPack>();
    same_type(
        PhantomData::<khora_sdk::FontPack>,
        PhantomData::<khora_core::ui::FontPack>,
    );
    let _ = type_name::<khora_sdk::GameWorld>();
    let _ = khora_sdk::GameWorld::new;
    let _ = khora_sdk::GameWorld::from_world;
    let _ = khora_sdk::GameWorld::spawn::<khora_sdk::prelude::ecs::Transform>;
    let _ = khora_sdk::GameWorld::despawn;
    let _ = khora_sdk::GameWorld::spawn_camera;
    let _ = khora_sdk::GameWorld::add_mesh;
    let _ = khora_sdk::GameWorld::add_component::<khora_sdk::prelude::ecs::Transform>;
    let _ = khora_sdk::GameWorld::remove_component::<khora_sdk::prelude::ecs::Transform>;
    let _ = khora_sdk::GameWorld::query::<&'static khora_sdk::prelude::ecs::Transform>;
    let _ = khora_sdk::GameWorld::query_mut::<&'static mut khora_sdk::prelude::ecs::Transform>;
    let _ = khora_sdk::GameWorld::spawn_entity;
    let _ = khora_sdk::GameWorld::iter_entities;
    let _ = khora_sdk::GameWorld::get_transform_mut;
    let _ = khora_sdk::GameWorld::get_transform;
    let _ = khora_sdk::GameWorld::get_component_mut::<khora_sdk::prelude::ecs::Transform>;
    let _ = khora_sdk::GameWorld::get_component::<khora_sdk::prelude::ecs::Transform>;
    let _ = khora_sdk::GameWorld::sync_global_transform;
    let _ = khora_sdk::GameWorld::update_transform::<fn(&mut khora_sdk::prelude::ecs::Transform)>;
    let _ = khora_sdk::GameWorld::set_parent;
    let _ = khora_sdk::GameWorld::add_material::<khora_sdk::prelude::materials::StandardMaterial>;
    let _ = khora_sdk::GameWorld::inner_world;
    let _ = khora_sdk::GameWorld::inner_world_mut;
    is_default::<khora_sdk::GameWorld>();
    let _ = type_name::<khora_sdk::GizmoLineInstance>();
    same_type(
        PhantomData::<khora_sdk::GizmoLineInstance>,
        PhantomData::<khora_core::ui::editor::GizmoLineInstance>,
    );
    let _ = type_name::<khora_sdk::GizmoMode>();
    same_type(
        PhantomData::<khora_sdk::GizmoMode>,
        PhantomData::<khora_core::ui::editor::GizmoMode>,
    );
    let _ = type_name::<khora_sdk::GpuMonitor>();
    same_type(
        PhantomData::<khora_sdk::GpuMonitor>,
        PhantomData::<khora_infra::GpuMonitor>,
    );
    let _ = type_name::<khora_sdk::HandleComponent<khora_sdk::SoundData>>();
    same_type(
        PhantomData::<khora_sdk::HandleComponent<khora_sdk::SoundData>>,
        PhantomData::<khora_data::ecs::HandleComponent<khora_sdk::SoundData>>,
    );
    let _ = type_name::<khora_sdk::tool_ui::Icon>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::Icon>,
        PhantomData::<khora_core::ui::editor::Icon>,
    );
    let _ = type_name::<khora_sdk::Icon>();
    same_type(
        PhantomData::<khora_sdk::Icon>,
        PhantomData::<khora_core::ui::editor::Icon>,
    );
    let _ = type_name::<khora_sdk::IndexBuilder>();
    same_type(
        PhantomData::<khora_sdk::IndexBuilder>,
        PhantomData::<khora_io::asset::IndexBuilder>,
    );
    let _ = type_name::<khora_sdk::prelude::InputEvent>();
    same_type(
        PhantomData::<khora_sdk::prelude::InputEvent>,
        PhantomData::<khora_core::platform::InputEvent>,
    );
    let _ = type_name::<khora_sdk::InputEvent>();
    same_type(
        PhantomData::<khora_sdk::InputEvent>,
        PhantomData::<khora_core::platform::InputEvent>,
    );
    let _ = type_name::<khora_sdk::InspectedEntity>();
    same_type(
        PhantomData::<khora_sdk::InspectedEntity>,
        PhantomData::<khora_core::ui::editor::InspectedEntity>,
    );
    let _ = type_name::<khora_sdk::tool_ui::Interaction>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::Interaction>,
        PhantomData::<khora_core::ui::editor::Interaction>,
    );
    let _ = type_name::<khora_sdk::Interaction>();
    same_type(
        PhantomData::<khora_sdk::Interaction>,
        PhantomData::<khora_core::ui::editor::Interaction>,
    );
    let _ = type_name::<khora_sdk::prelude::KeyCode>();
    same_type(
        PhantomData::<khora_sdk::prelude::KeyCode>,
        PhantomData::<khora_core::platform::KeyCode>,
    );
    let _ = type_name::<khora_sdk::KeyCode>();
    same_type(
        PhantomData::<khora_sdk::KeyCode>,
        PhantomData::<khora_core::platform::KeyCode>,
    );
    // trait `khora_sdk::LayoutSystem` is `khora_core::ui::LayoutSystem`: see `trait_items::khora_sdk_layout_system_same`
    let _ = type_name::<khora_sdk::LogEntry>();
    same_type(
        PhantomData::<khora_sdk::LogEntry>,
        PhantomData::<khora_core::ui::editor::LogEntry>,
    );
    let _ = type_name::<khora_sdk::LogLevel>();
    same_type(
        PhantomData::<khora_sdk::LogLevel>,
        PhantomData::<khora_core::ui::editor::LogLevel>,
    );
    let _ = type_name::<khora_sdk::Mat4>();
    same_type(
        PhantomData::<khora_sdk::Mat4>,
        PhantomData::<khora_core::math::Mat4>,
    );
    let _ = type_name::<khora_sdk::MemoryMonitor>();
    same_type(
        PhantomData::<khora_sdk::MemoryMonitor>,
        PhantomData::<khora_infra::MemoryMonitor>,
    );
    let _ = type_name::<khora_sdk::Mesh>();
    same_type(
        PhantomData::<khora_sdk::Mesh>,
        PhantomData::<khora_core::renderer::api::scene::Mesh>,
    );
    let _ = type_name::<khora_sdk::MeshDispatcher>();
    same_type(
        PhantomData::<khora_sdk::MeshDispatcher>,
        PhantomData::<khora_io::asset::MeshDispatcher>,
    );
    let _ = type_name::<khora_sdk::MetricsRegistry>();
    same_type(
        PhantomData::<khora_sdk::MetricsRegistry>,
        PhantomData::<khora_telemetry::MetricsRegistry>,
    );
    let _ = type_name::<khora_sdk::MonitorRegistry>();
    same_type(
        PhantomData::<khora_sdk::MonitorRegistry>,
        PhantomData::<khora_telemetry::MonitorRegistry>,
    );
    let _ = type_name::<khora_sdk::MonitoredResourceType>();
    same_type(
        PhantomData::<khora_sdk::MonitoredResourceType>,
        PhantomData::<khora_core::telemetry::MonitoredResourceType>,
    );
    let _ = type_name::<khora_sdk::prelude::MouseButton>();
    same_type(
        PhantomData::<khora_sdk::prelude::MouseButton>,
        PhantomData::<khora_core::platform::MouseButton>,
    );
    let _ = type_name::<khora_sdk::MouseButton>();
    same_type(
        PhantomData::<khora_sdk::MouseButton>,
        PhantomData::<khora_core::platform::MouseButton>,
    );
    let _ = type_name::<khora_sdk::editor_ui::NamedFont>();
    same_type(
        PhantomData::<khora_sdk::editor_ui::NamedFont>,
        PhantomData::<khora_core::ui::NamedFont>,
    );
    let _ = type_name::<khora_sdk::tool_ui::NamedFont>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::NamedFont>,
        PhantomData::<khora_core::ui::NamedFont>,
    );
    let _ = type_name::<khora_sdk::NamedFont>();
    same_type(
        PhantomData::<khora_sdk::NamedFont>,
        PhantomData::<khora_core::ui::NamedFont>,
    );
    let _ = khora_sdk::PACK_FORMAT_VERSION;
    assert_eq!(
        khora_sdk::PACK_FORMAT_VERSION,
        khora_io::asset::PACK_FORMAT_VERSION
    );
    let _ = khora_sdk::PACK_HEADER_SIZE;
    assert_eq!(
        khora_sdk::PACK_HEADER_SIZE,
        khora_io::asset::PACK_HEADER_SIZE
    );
    let _ = khora_sdk::PACK_MAGIC;
    assert_eq!(khora_sdk::PACK_MAGIC, khora_io::asset::PACK_MAGIC);
    let _ = khora_sdk::prelude::PRIMARY_VIEWPORT;
    // Spelled by the hub's generated `main.rs` template and by the editor and
    // runtime binaries, as a `static` global allocator built by a `const fn`
    // with the default inner allocator.
    const _: khora_sdk::prelude::SaaTrackingAllocator =
        khora_sdk::prelude::SaaTrackingAllocator::new(std::alloc::System);
    let _ = khora_sdk::PRIMARY_VIEWPORT;
    let _ = type_name::<khora_sdk::PackBuilder>();
    same_type(
        PhantomData::<khora_sdk::PackBuilder>,
        PhantomData::<khora_io::asset::PackBuilder>,
    );
    let _ = type_name::<khora_sdk::PackHeader>();
    same_type(
        PhantomData::<khora_sdk::PackHeader>,
        PhantomData::<khora_io::asset::PackHeader>,
    );
    let _ = type_name::<khora_sdk::PackLoader>();
    same_type(
        PhantomData::<khora_sdk::PackLoader>,
        PhantomData::<khora_io::asset::PackLoader>,
    );
    let _ = type_name::<khora_sdk::PackOutput>();
    same_type(
        PhantomData::<khora_sdk::PackOutput>,
        PhantomData::<khora_io::asset::PackOutput>,
    );
    let _ = type_name::<khora_sdk::PackProgress>();
    same_type(
        PhantomData::<khora_sdk::PackProgress>,
        PhantomData::<khora_io::asset::PackProgress>,
    );
    let _ = type_name::<khora_sdk::PanelLocation>();
    same_type(
        PhantomData::<khora_sdk::PanelLocation>,
        PhantomData::<khora_core::ui::PanelLocation>,
    );
    // trait `khora_sdk::PhaseProvider`: see `phase_provider_trait_items`
    // trait `khora_sdk::PhysicsProvider` is `khora_core::physics::PhysicsProvider`: see `trait_items::khora_sdk_physics_provider_same`
    // trait `khora_sdk::PipelineSystem` is `khora_core::renderer::traits::PipelineSystem`: see `trait_items::khora_sdk_pipeline_system_same`
    let _ = type_name::<khora_sdk::PlayMode>();
    same_type(
        PhantomData::<khora_sdk::PlayMode>,
        PhantomData::<khora_core::ui::editor::PlayMode>,
    );
    let _ = type_name::<khora_sdk::PropertyEdit>();
    same_type(
        PhantomData::<khora_sdk::PropertyEdit>,
        PhantomData::<khora_core::ui::editor::PropertyEdit>,
    );
    let _ = type_name::<khora_sdk::RapierPhysicsWorld>();
    same_type(
        PhantomData::<khora_sdk::RapierPhysicsWorld>,
        PhantomData::<khora_infra::physics::rapier::RapierPhysicsWorld>,
    );
    // trait `khora_sdk::RenderSystem` is `khora_core::renderer::RenderSystem`: see `trait_items::khora_sdk_render_system_same`
    let _ = type_name::<khora_sdk::Resources>();
    same_type(
        PhantomData::<khora_sdk::Resources>,
        PhantomData::<khora_core::Resources>,
    );
    let _ = type_name::<khora_sdk::Runtime>();
    same_type(
        PhantomData::<khora_sdk::Runtime>,
        PhantomData::<khora_core::Runtime>,
    );
    let _ = type_name::<khora_sdk::SceneFile>();
    same_type(
        PhantomData::<khora_sdk::SceneFile>,
        PhantomData::<khora_core::scene::SceneFile>,
    );
    let _ = type_name::<khora_sdk::SceneNode>();
    same_type(
        PhantomData::<khora_sdk::SceneNode>,
        PhantomData::<khora_core::ui::editor::SceneNode>,
    );
    let _ = type_name::<khora_sdk::SerializationGoal>();
    same_type(
        PhantomData::<khora_sdk::SerializationGoal>,
        PhantomData::<khora_core::scene::SerializationGoal>,
    );
    let _ = type_name::<khora_sdk::SerializationService>();
    same_type(
        PhantomData::<khora_sdk::SerializationService>,
        PhantomData::<khora_io::serialization::SerializationService>,
    );
    let _ = type_name::<khora_sdk::Services>();
    same_type(
        PhantomData::<khora_sdk::Services>,
        PhantomData::<khora_core::Services>,
    );
    let _ = type_name::<khora_sdk::SoundData>();
    same_type(
        PhantomData::<khora_sdk::SoundData>,
        PhantomData::<khora_data::assets::SoundData>,
    );
    let _ = type_name::<khora_sdk::StandardTextRenderer>();
    same_type(
        PhantomData::<khora_sdk::StandardTextRenderer>,
        PhantomData::<khora_infra::StandardTextRenderer>,
    );
    let _ = type_name::<khora_sdk::StatusBarData>();
    same_type(
        PhantomData::<khora_sdk::StatusBarData>,
        PhantomData::<khora_core::ui::editor::StatusBarData>,
    );
    let _ = type_name::<khora_sdk::StrategyId>();
    same_type(
        PhantomData::<khora_sdk::StrategyId>,
        PhantomData::<khora_core::agent::gorna::StrategyId>,
    );
    let _ = type_name::<khora_sdk::StreamInfo>();
    same_type(
        PhantomData::<khora_sdk::StreamInfo>,
        PhantomData::<khora_core::audio::StreamInfo>,
    );
    let _ = type_name::<khora_sdk::SymphoniaDecoder>();
    same_type(
        PhantomData::<khora_sdk::SymphoniaDecoder>,
        PhantomData::<khora_io::asset::SymphoniaDecoder>,
    );
    let _ = khora_sdk::EGUI_WGSL;
    assert_eq!(khora_sdk::EGUI_WGSL, khora_infra::graphics::EGUI_WGSL);
    let _ = khora_sdk::TEXT_WGSL;
    assert_eq!(khora_sdk::TEXT_WGSL, khora_infra::graphics::TEXT_WGSL);
    let _ = type_name::<khora_sdk::TaffyLayoutSystem>();
    same_type(
        PhantomData::<khora_sdk::TaffyLayoutSystem>,
        PhantomData::<khora_infra::TaffyLayoutSystem>,
    );
    let _ = type_name::<khora_sdk::TelemetryEvent>();
    same_type(
        PhantomData::<khora_sdk::TelemetryEvent>,
        PhantomData::<khora_core::telemetry::TelemetryEvent>,
    );
    let _ = type_name::<khora_sdk::TelemetryService>();
    same_type(
        PhantomData::<khora_sdk::TelemetryService>,
        PhantomData::<khora_telemetry::TelemetryService>,
    );
    let _ = type_name::<khora_sdk::tool_ui::TextAlign>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::TextAlign>,
        PhantomData::<khora_core::ui::editor::TextAlign>,
    );
    let _ = type_name::<khora_sdk::TextAlign>();
    same_type(
        PhantomData::<khora_sdk::TextAlign>,
        PhantomData::<khora_core::ui::editor::TextAlign>,
    );
    // trait `khora_sdk::TextRenderer` is `khora_core::renderer::api::text::TextRenderer`: see `trait_items::khora_sdk_text_renderer_same`
    // trait `khora_sdk::tool_ui::UiBuilder` is `khora_core::ui::UiBuilder`: see `trait_items::khora_sdk_tool_ui_ui_builder_same`
    // trait `khora_sdk::UiBuilder` is `khora_core::ui::UiBuilder`: see `trait_items::khora_sdk_ui_builder_same`
    let _ = type_name::<khora_sdk::editor_ui::UiTheme>();
    same_type(
        PhantomData::<khora_sdk::editor_ui::UiTheme>,
        PhantomData::<khora_core::ui::UiTheme>,
    );
    let _ = type_name::<khora_sdk::tool_ui::UiTheme>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::UiTheme>,
        PhantomData::<khora_core::ui::UiTheme>,
    );
    let _ = type_name::<khora_sdk::UiTheme>();
    same_type(
        PhantomData::<khora_sdk::UiTheme>,
        PhantomData::<khora_core::ui::UiTheme>,
    );
    let _ = type_name::<khora_sdk::Vessel<'static>>();
    let _ = khora_sdk::Vessel::new;
    let _ = khora_sdk::Vessel::at;
    let _ = khora_sdk::Vessel::with_transform;
    let _ = khora_sdk::Vessel::at_position;
    let _ = khora_sdk::Vessel::with_rotation;
    let _ = khora_sdk::Vessel::with_scale;
    let _ = khora_sdk::Vessel::with_component::<khora_sdk::prelude::ecs::Transform>;
    let _ = khora_sdk::Vessel::entity;
    let _ = khora_sdk::Vessel::build;
    let _ = type_name::<khora_sdk::ViewportTextureHandle>();
    same_type(
        PhantomData::<khora_sdk::ViewportTextureHandle>,
        PhantomData::<khora_core::ui::ViewportTextureHandle>,
    );
    let _ = type_name::<khora_sdk::WgpuPipelineSystem>();
    same_type(
        PhantomData::<khora_sdk::WgpuPipelineSystem>,
        PhantomData::<khora_infra::graphics::WgpuPipelineSystem>,
    );
    let _ = type_name::<khora_sdk::WgpuRenderSystem>();
    same_type(
        PhantomData::<khora_sdk::WgpuRenderSystem>,
        PhantomData::<khora_infra::WgpuRenderSystem>,
    );
    let _ = type_name::<khora_sdk::prelude::WindowConfig>();
    let _ = type_name::<khora_sdk::WindowConfig>();
    same_type(
        PhantomData::<khora_sdk::prelude::WindowConfig>,
        PhantomData::<khora_sdk::WindowConfig>,
    );
    let _ = window_config_fields as fn(&khora_sdk::WindowConfig);
    is_clone::<khora_sdk::WindowConfig>();
    is_debug::<khora_sdk::WindowConfig>();
    is_default::<khora_sdk::WindowConfig>();
    let _ = type_name::<khora_sdk::prelude::WindowIcon>();
    let _ = type_name::<khora_sdk::WindowIcon>();
    same_type(
        PhantomData::<khora_sdk::prelude::WindowIcon>,
        PhantomData::<khora_sdk::WindowIcon>,
    );
    let _ = window_icon_fields as fn(&khora_sdk::WindowIcon);
    is_clone::<khora_sdk::WindowIcon>();
    is_debug::<khora_sdk::WindowIcon>();
    // trait `khora_sdk::WindowProvider`: see `window_provider_trait_items`
    let _ = khora_sdk::instantiate_subtree;
    same_item(
        &khora_sdk::instantiate_subtree,
        &khora_data::scene::instantiate_subtree,
    );
    #[allow(unused_imports)]
    use khora_sdk::inventory as _; // `pub extern crate`
    #[allow(unused_imports)]
    use khora_sdk::khora_core as _; // module `khora_core`
    #[allow(unused_imports)]
    use khora_sdk::khora_data as _; // module `khora_data`
    #[allow(unused_imports)]
    use khora_sdk::khora_lanes as _; // module `khora_lanes`
    let _ = khora_sdk::run_default;
    let _ = type_name::<khora_sdk::RuntimeConfig>();
    let _ = runtime_config_fields as fn(&khora_sdk::RuntimeConfig);
    let _: fn(&std::path::Path) -> khora_sdk::RuntimeConfig =
        khora_sdk::RuntimeConfig::load_or_default;
    let _: fn() -> khora_sdk::RuntimeConfig = khora_sdk::RuntimeConfig::defaults;
    let _: fn(&khora_sdk::RuntimeConfig) -> String = khora_sdk::RuntimeConfig::window_title;
    is_debug::<khora_sdk::RuntimeConfig>();
    is_clone::<khora_sdk::RuntimeConfig>();
    let _: &str = khora_sdk::DEFAULT_SCENE_REL_PATH;
    let _: &str = khora_sdk::RUNTIME_CONFIG_FILE;
    let _ = khora_sdk::serialize_subtree;
    same_item(
        &khora_sdk::serialize_subtree,
        &khora_data::scene::serialize_subtree,
    );
    let _ = khora_sdk::spawn_cube_at;
    let _ = khora_sdk::spawn_plane;
    let _ = khora_sdk::spawn_sphere;
    #[allow(unused_imports)]
    use khora_sdk::winit as _; // module `winit`
}

#[test]
fn module_scripts_paths_still_resolve() {
    let _ = khora_sdk::scripts::mount;
}

#[test]
fn module_winit_adapters_paths_still_resolve() {
    let _ = type_name::<
        khora_sdk::winit_adapters::WinitAppRunner<khora_sdk::WinitWindowProvider, ProbeApp>,
    >();
    let _ = type_name::<khora_sdk::WinitAppRunner<khora_sdk::WinitWindowProvider, ProbeApp>>();
    same_type(
        PhantomData::<khora_sdk::WinitAppRunner<khora_sdk::WinitWindowProvider, ProbeApp>>,
        PhantomData::<
            khora_sdk::winit_adapters::WinitAppRunner<khora_sdk::WinitWindowProvider, ProbeApp>,
        >,
    );
    #[allow(clippy::type_complexity)]
    let _: fn(
        fn(&dyn khora_core::platform::KhoraWindow, &mut khora_sdk::Runtime, &dyn std::any::Any),
    ) -> khora_sdk::winit_adapters::WinitAppRunner<
        khora_sdk::WinitWindowProvider,
        ProbeApp,
    > = <khora_sdk::winit_adapters::WinitAppRunner<khora_sdk::WinitWindowProvider, ProbeApp>>::new;
    is_application_handler::<
        khora_sdk::winit_adapters::WinitAppRunner<khora_sdk::WinitWindowProvider, ProbeApp>,
    >();
    let _ = type_name::<khora_sdk::winit_adapters::WinitWindowProvider>();
    let _ = type_name::<khora_sdk::WinitWindowProvider>();
    same_type(
        PhantomData::<khora_sdk::WinitWindowProvider>,
        PhantomData::<khora_sdk::winit_adapters::WinitWindowProvider>,
    );
    is_window_provider::<khora_sdk::winit_adapters::WinitWindowProvider>();
    #[allow(clippy::type_complexity)]
    let _: fn(
        fn(&dyn khora_core::platform::KhoraWindow, &mut khora_sdk::Runtime, &dyn std::any::Any),
    ) -> anyhow::Result<()> =
        khora_sdk::winit_adapters::run_winit::<khora_sdk::WinitWindowProvider, ProbeApp>;
    #[allow(clippy::type_complexity)]
    let _: fn(
        fn(&dyn khora_core::platform::KhoraWindow, &mut khora_sdk::Runtime, &dyn std::any::Any),
    ) -> anyhow::Result<()> = khora_sdk::run_winit::<khora_sdk::WinitWindowProvider, ProbeApp>;
}

#[test]
fn module_tool_ui_paths_still_resolve() {
    let _ = type_name::<khora_sdk::tool_ui::Align>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::Align>,
        PhantomData::<khora_core::ui::Align>,
    );
    let _ = type_name::<khora_sdk::tool_ui::Align2>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::Align2>,
        PhantomData::<khora_core::ui::Align2>,
    );
    // trait `khora_sdk::tool_ui::App` is `khora_core::ui::App`: see `trait_items::khora_sdk_tool_ui_app_same`
    // trait `khora_sdk::tool_ui::AppContext` is `khora_core::ui::AppContext`: see `trait_items::khora_sdk_tool_ui_app_context_same`
    // trait `khora_sdk::tool_ui::AppLifecycle` is `khora_core::ui::AppLifecycle`: see `trait_items::khora_sdk_tool_ui_app_lifecycle_same`
    let _ = type_name::<khora_sdk::tool_ui::CornerRadius>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::CornerRadius>,
        PhantomData::<khora_core::ui::CornerRadius>,
    );
    let _ = type_name::<khora_sdk::tool_ui::InlineEditEvent>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::InlineEditEvent>,
        PhantomData::<khora_core::ui::editor::InlineEditEvent>,
    );
    let _ = type_name::<khora_sdk::tool_ui::LinearRgba>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::LinearRgba>,
        PhantomData::<khora_core::math::LinearRgba>,
    );
    let _ = type_name::<khora_sdk::prelude::math::LinearRgba>();
    same_type(
        PhantomData::<khora_sdk::prelude::math::LinearRgba>,
        PhantomData::<khora_core::math::LinearRgba>,
    );
    let _ = type_name::<khora_sdk::tool_ui::Margin>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::Margin>,
        PhantomData::<khora_core::ui::Margin>,
    );
    let _ = type_name::<khora_sdk::tool_ui::Rect2D>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::Rect2D>,
        PhantomData::<khora_core::math::Rect2D>,
    );
    let _ = type_name::<khora_sdk::tool_ui::Stroke>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::Stroke>,
        PhantomData::<khora_core::ui::Stroke>,
    );
    let _ = type_name::<khora_sdk::tool_ui::Vec2>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::Vec2>,
        PhantomData::<khora_core::math::Vec2>,
    );
    let _ = type_name::<khora_sdk::tool_ui::WindowConfigInput>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::WindowConfigInput>,
        PhantomData::<khora_infra::ui::egui::app::WindowConfigInput>,
    );
    let _ = type_name::<khora_sdk::tool_ui::WindowIconInput>();
    same_type(
        PhantomData::<khora_sdk::tool_ui::WindowIconInput>,
        PhantomData::<khora_infra::ui::egui::app::WindowIconInput>,
    );
    let _ = khora_sdk::tool_ui::run_native::<fn() -> Box<dyn khora_core::ui::App>>;
    same_item(
        &khora_sdk::tool_ui::run_native::<fn() -> Box<dyn khora_core::ui::App>>,
        &khora_infra::ui::egui::run_native::<fn() -> Box<dyn khora_core::ui::App>>,
    );
}

#[test]
fn module_renderer_paths_still_resolve() {
    #[allow(unused_imports)]
    use khora_sdk::renderer::light as _; // module `khora_core::renderer::light`
    #[allow(unused_imports)]
    use khora_sdk::renderer::resource as _; // module `khora_core::renderer::api::resource`
    #[allow(unused_imports)]
    use khora_sdk::renderer::scene as _; // module `khora_core::renderer::api::scene`
}

#[test]
fn module_prelude_paths_still_resolve() {
    let _ = type_name::<khora_sdk::prelude::AssetHandle<khora_sdk::SoundData>>();
    same_type(
        PhantomData::<khora_sdk::prelude::AssetHandle<khora_sdk::SoundData>>,
        PhantomData::<khora_core::asset::Handle<khora_sdk::SoundData>>,
    );
    let _ = type_name::<khora_sdk::prelude::AssetUUID>();
    same_type(
        PhantomData::<khora_sdk::prelude::AssetUUID>,
        PhantomData::<khora_core::asset::AssetUUID>,
    );
    let _ = type_name::<khora_sdk::prelude::SaaTrackingAllocator<u32>>();
    same_type(
        PhantomData::<khora_sdk::prelude::SaaTrackingAllocator<u32>>,
        PhantomData::<khora_infra::memory::SaaTrackingAllocator<u32>>,
    );
    let _ = type_name::<khora_sdk::prelude::SharedTime>();
    same_type(
        PhantomData::<khora_sdk::prelude::SharedTime>,
        PhantomData::<khora_core::time::SharedTime>,
    );
    let _ = type_name::<khora_sdk::prelude::Time>();
    same_type(
        PhantomData::<khora_sdk::prelude::Time>,
        PhantomData::<khora_core::time::Time>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::AudioSource>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::AudioSource>,
        PhantomData::<khora_data::ecs::AudioSource>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::BodyType>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::BodyType>,
        PhantomData::<khora_core::physics::BodyType>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::Camera>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::Camera>,
        PhantomData::<khora_data::ecs::Camera>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::Children>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::Children>,
        PhantomData::<khora_data::ecs::Children>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::Collider>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::Collider>,
        PhantomData::<khora_data::ecs::Collider>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::ColliderShape>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::ColliderShape>,
        PhantomData::<khora_core::physics::ColliderShape>,
    );
    // trait `khora_sdk::prelude::ecs::Component` is `khora_data::ecs::Component`: see `trait_items::khora_sdk_prelude_ecs_component_same`
    // trait `khora_sdk::prelude::ecs::ComponentBundle` is `khora_data::ecs::ComponentBundle`: see `trait_items::khora_sdk_prelude_ecs_component_bundle_same`
    let _ = type_name::<khora_sdk::prelude::ecs::DirectionalLight>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::DirectionalLight>,
        PhantomData::<khora_core::renderer::DirectionalLight>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::EntityId>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::EntityId>,
        PhantomData::<khora_core::ecs::entity::EntityId>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::GlobalTransform>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::GlobalTransform>,
        PhantomData::<khora_data::ecs::GlobalTransform>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::Light>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::Light>,
        PhantomData::<khora_data::ecs::Light>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::LightType>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::LightType>,
        PhantomData::<khora_core::renderer::LightType>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::MaterialRef>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::MaterialRef>,
        PhantomData::<khora_data::ecs::MaterialRef>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::MeshRef>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::MeshRef>,
        PhantomData::<khora_data::ecs::MeshRef>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::Name>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::Name>,
        PhantomData::<khora_data::ecs::Name>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::Parent>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::Parent>,
        PhantomData::<khora_data::ecs::Parent>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::PointLight>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::PointLight>,
        PhantomData::<khora_core::renderer::PointLight>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::ProceduralMeshKind>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::ProceduralMeshKind>,
        PhantomData::<khora_data::ecs::ProceduralMeshKind>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::ProjectionType>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::ProjectionType>,
        PhantomData::<khora_data::ecs::ProjectionType>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::RigidBody>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::RigidBody>,
        PhantomData::<khora_data::ecs::RigidBody>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::Script>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::Script>,
        PhantomData::<khora_data::ecs::Script>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::ScriptValue>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::ScriptValue>,
        PhantomData::<khora_core::script::ScriptValue>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::SpotLight>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::SpotLight>,
        PhantomData::<khora_core::renderer::SpotLight>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::Tag>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::Tag>,
        PhantomData::<khora_data::ecs::Tag>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::Transform>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::Transform>,
        PhantomData::<khora_data::ecs::Transform>,
    );
    let _ = type_name::<khora_sdk::prelude::ecs::Without<khora_sdk::prelude::ecs::Transform>>();
    same_type(
        PhantomData::<khora_sdk::prelude::ecs::Without<khora_sdk::prelude::ecs::Transform>>,
        PhantomData::<khora_data::ecs::Without<khora_sdk::prelude::ecs::Transform>>,
    );
    let _ = type_name::<khora_sdk::prelude::materials::AlphaMode>();
    same_type(
        PhantomData::<khora_sdk::prelude::materials::AlphaMode>,
        PhantomData::<khora_core::asset::AlphaMode>,
    );
    let _ = type_name::<khora_sdk::prelude::materials::EmissiveMaterial>();
    same_type(
        PhantomData::<khora_sdk::prelude::materials::EmissiveMaterial>,
        PhantomData::<khora_core::asset::EmissiveMaterial>,
    );
    let _ = type_name::<khora_sdk::prelude::materials::StandardMaterial>();
    same_type(
        PhantomData::<khora_sdk::prelude::materials::StandardMaterial>,
        PhantomData::<khora_core::asset::StandardMaterial>,
    );
    let _ = type_name::<khora_sdk::prelude::materials::UnlitMaterial>();
    same_type(
        PhantomData::<khora_sdk::prelude::materials::UnlitMaterial>,
        PhantomData::<khora_core::asset::UnlitMaterial>,
    );
    let _ = type_name::<khora_sdk::prelude::materials::WireframeMaterial>();
    same_type(
        PhantomData::<khora_sdk::prelude::materials::WireframeMaterial>,
        PhantomData::<khora_core::asset::WireframeMaterial>,
    );
}

// ---------------------------------------------------------------------------
// The smallest `EngineApp`, written the way a game writes one, so the generic
// SDK items (`EngineCore<A>`, `WinitAppRunner<W, A>`, `run_winit`) can be
// named with a concrete argument. Implementing the three traits through their
// `khora_sdk::` paths also pins every required item's signature. Never run.
// ---------------------------------------------------------------------------

struct ProbeApp;

impl khora_sdk::AgentProvider for ProbeApp {
    fn register_agents(&self, _dcc: &khora_sdk::DccService, _runtime: &mut khora_sdk::Runtime) {}
}

impl khora_sdk::PhaseProvider for ProbeApp {}

impl khora_sdk::EngineApp for ProbeApp {
    fn window_config() -> khora_sdk::WindowConfig {
        khora_sdk::WindowConfig::default()
    }

    fn new() -> Self {
        ProbeApp
    }

    fn setup(&mut self, _world: &mut khora_sdk::GameWorld, _runtime: &khora_sdk::Runtime) {}

    fn update(&mut self, _world: &mut khora_sdk::GameWorld, _inputs: &[khora_sdk::InputEvent]) {}
}

// ---------------------------------------------------------------------------
// Whole crates re-exported by the SDK (`pub use khora_io;`, …, `pub extern
// crate inventory;`): each is the dependency itself, checked through one item.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn inventory_collect_same<T: khora_sdk::inventory::Collect>() {
    inventory_collect_same_rev::<T>();
}

#[allow(dead_code)]
fn inventory_collect_same_rev<T: inventory::Collect>() {
    inventory_collect_same::<T>();
}

#[test]
fn whole_crate_reexports_are_the_dependencies() {
    same_type(
        PhantomData::<khora_sdk::khora_core::math::Vec3>,
        PhantomData::<khora_core::math::Vec3>,
    );
    same_type(
        PhantomData::<khora_sdk::khora_data::ecs::World>,
        PhantomData::<khora_data::ecs::World>,
    );
    same_type(
        PhantomData::<khora_sdk::khora_lanes::overlay_lane::GridLane>,
        PhantomData::<khora_lanes::overlay_lane::GridLane>,
    );
    same_type(
        PhantomData::<khora_sdk::winit::event_loop::EventLoop<()>>,
        PhantomData::<winit::event_loop::EventLoop<()>>,
    );
}

// ---------------------------------------------------------------------------
// Values games and the editor rely on. `PRIMARY_VIEWPORT` is defined once, in
// `engine/mod.rs` (the winit runner inserts it as a resource), and re-exported
// at the crate root and in the prelude; both paths must keep naming the first
// viewport. The `runtime.json` the editor writes and the runtime reads keeps
// its file name, its default scene and its defaults.
// ---------------------------------------------------------------------------

#[test]
fn primary_viewport_names_the_first_viewport() {
    assert_eq!(
        khora_sdk::PRIMARY_VIEWPORT,
        khora_sdk::ViewportTextureHandle(0)
    );
    assert_eq!(
        khora_sdk::prelude::PRIMARY_VIEWPORT,
        khora_sdk::PRIMARY_VIEWPORT
    );
}

#[test]
fn runtime_config_file_and_defaults_are_unchanged() {
    assert_eq!(khora_sdk::RUNTIME_CONFIG_FILE, "runtime.json");
    assert_eq!(khora_sdk::DEFAULT_SCENE_REL_PATH, "scenes/default.kscene");
    let defaults = khora_sdk::RuntimeConfig::defaults();
    assert_eq!(defaults.project_name, "Khora Runtime");
    assert_eq!(defaults.default_scene, khora_sdk::DEFAULT_SCENE_REL_PATH);
    assert_eq!(defaults.window_title, None);
    assert_eq!(defaults.window_title(), "Khora Runtime");
    assert_eq!(defaults.preset, None);
    assert!(!defaults.verify_integrity);
}

#[test]
fn window_config_defaults_are_unchanged() {
    let config = khora_sdk::WindowConfig::default();
    assert_eq!(config.title, "Khora Engine");
    assert_eq!((config.width, config.height), (1024, 768));
    assert!(config.icon.is_none());
    same_type(
        PhantomData::<khora_sdk::prelude::WindowConfig>,
        PhantomData::<khora_sdk::WindowConfig>,
    );
    same_type(
        PhantomData::<khora_sdk::prelude::WindowIcon>,
        PhantomData::<khora_sdk::WindowIcon>,
    );
}

// ---------------------------------------------------------------------------
// Glob and module re-exports of another crate's module: every item they
// bring in today, each checked to be the item of the source module
// (`same_type` / `same_item` / equal constants / bounds forwarded both ways
// for traits). A glob replaced by an explicit list that forgets one, or a
// module re-export pointed elsewhere, fails here.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod glob_reexports {
    use khora_sdk::editor_ui::gizmo as _; // module
    use khora_sdk::editor_ui::icons as _; // module
    use khora_sdk::editor_ui::overlay as _; // module
    use khora_sdk::editor_ui::panel as _; // module
    use khora_sdk::editor_ui::shell as _; // module
    use khora_sdk::editor_ui::state as _; // module
    use khora_sdk::editor_ui::ui_builder as _; // module
    use khora_sdk::editor_ui::viewport_texture as _; // module
    use khora_sdk::editor_ui::EditorPanel as _;
    use khora_sdk::editor_ui::EditorShell as _;
    use khora_sdk::editor_ui::UiBuilder as _;
    use khora_sdk::prelude::math::affine_transform as _; // module
    use khora_sdk::prelude::math::color as _; // module
    use khora_sdk::prelude::math::cube as _; // module
    use khora_sdk::prelude::math::dimension as _; // module
    use khora_sdk::prelude::math::geometry as _; // module
    use khora_sdk::prelude::math::matrix as _; // module
    use khora_sdk::prelude::math::quaternion as _; // module
    use khora_sdk::prelude::math::ray as _; // module
    use khora_sdk::prelude::math::simd as _; // module
    use khora_sdk::prelude::math::vector as _; // module
    use khora_sdk::renderer::resource::buffer as _; // module
    use khora_sdk::renderer::resource::shader_source as _; // module
    use khora_sdk::renderer::resource::texture as _; // module
    use khora_sdk::renderer::resource::view as _; // module
    use khora_sdk::renderer::scene::gpu_material as _; // module
    use khora_sdk::renderer::scene::lighting as _; // module
    use khora_sdk::renderer::scene::material_uniforms as _; // module
    use khora_sdk::renderer::scene::mesh as _; // module
    use khora_sdk::renderer::scene::render_object as _; // module
}

#[allow(dead_code)]
mod glob_reexported_traits {
    fn khora_sdk_editor_ui_editor_panel_same<T: ?Sized + khora_sdk::editor_ui::EditorPanel>() {
        khora_sdk_editor_ui_editor_panel_same_rev::<T>();
    }

    fn khora_sdk_editor_ui_editor_panel_same_rev<
        T: ?Sized + khora_core::ui::editor::EditorPanel,
    >() {
        khora_sdk_editor_ui_editor_panel_same::<T>();
    }

    fn khora_sdk_editor_ui_editor_shell_same<T: ?Sized + khora_sdk::editor_ui::EditorShell>() {
        khora_sdk_editor_ui_editor_shell_same_rev::<T>();
    }

    fn khora_sdk_editor_ui_editor_shell_same_rev<
        T: ?Sized + khora_core::ui::editor::EditorShell,
    >() {
        khora_sdk_editor_ui_editor_shell_same::<T>();
    }

    fn khora_sdk_editor_ui_ui_builder_same<T: ?Sized + khora_sdk::editor_ui::UiBuilder>() {
        khora_sdk_editor_ui_ui_builder_same_rev::<T>();
    }

    fn khora_sdk_editor_ui_ui_builder_same_rev<T: ?Sized + khora_core::ui::editor::UiBuilder>() {
        khora_sdk_editor_ui_ui_builder_same::<T>();
    }
}

#[test]
fn glob_reexports_still_name_the_same_items() {
    // `pub use khora_core::ui::editor::*` at `khora_sdk::editor_ui`
    same_type(
        PhantomData::<khora_sdk::editor_ui::AssetEntry>,
        PhantomData::<khora_core::ui::editor::AssetEntry>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::ComponentJson>,
        PhantomData::<khora_core::ui::editor::ComponentJson>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::EditorMode>,
        PhantomData::<khora_core::ui::editor::EditorMode>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::EditorState>,
        PhantomData::<khora_core::ui::editor::EditorState>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::EntityIcon>,
        PhantomData::<khora_core::ui::editor::EntityIcon>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::FontFamilyHint>,
        PhantomData::<khora_core::ui::editor::FontFamilyHint>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::GizmoLineInstance>,
        PhantomData::<khora_core::ui::editor::GizmoLineInstance>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::GizmoMode>,
        PhantomData::<khora_core::ui::editor::GizmoMode>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::Icon>,
        PhantomData::<khora_core::ui::editor::Icon>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::InlineEditEvent>,
        PhantomData::<khora_core::ui::editor::InlineEditEvent>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::InspectedEntity>,
        PhantomData::<khora_core::ui::editor::InspectedEntity>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::Interaction>,
        PhantomData::<khora_core::ui::editor::Interaction>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::LogEntry>,
        PhantomData::<khora_core::ui::editor::LogEntry>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::LogLevel>,
        PhantomData::<khora_core::ui::editor::LogLevel>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::PanelLocation>,
        PhantomData::<khora_core::ui::editor::PanelLocation>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::PlayMode>,
        PhantomData::<khora_core::ui::editor::PlayMode>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::PropertyEdit>,
        PhantomData::<khora_core::ui::editor::PropertyEdit>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::SceneNode>,
        PhantomData::<khora_core::ui::editor::SceneNode>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::StatusBarData>,
        PhantomData::<khora_core::ui::editor::StatusBarData>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::TextAlign>,
        PhantomData::<khora_core::ui::editor::TextAlign>,
    );
    same_type(
        PhantomData::<khora_sdk::editor_ui::ViewportTextureHandle>,
        PhantomData::<khora_core::ui::editor::ViewportTextureHandle>,
    );
    // `pub use khora_core::math::*` at `khora_sdk::prelude::math`
    same_type(
        PhantomData::<khora_sdk::prelude::math::Aabb>,
        PhantomData::<khora_core::math::Aabb>,
    );
    same_type(
        PhantomData::<khora_sdk::prelude::math::AffineTransform>,
        PhantomData::<khora_core::math::AffineTransform>,
    );
    same_type(
        PhantomData::<khora_sdk::prelude::math::CubeFace>,
        PhantomData::<khora_core::math::CubeFace>,
    );
    assert_eq!(
        khora_sdk::prelude::math::DEG_TO_RAD,
        khora_core::math::DEG_TO_RAD
    );
    assert_eq!(khora_sdk::prelude::math::E, khora_core::math::E);
    assert_eq!(khora_sdk::prelude::math::EPSILON, khora_core::math::EPSILON);
    same_type(
        PhantomData::<khora_sdk::prelude::math::Extent1D>,
        PhantomData::<khora_core::math::Extent1D>,
    );
    same_type(
        PhantomData::<khora_sdk::prelude::math::Extent2D>,
        PhantomData::<khora_core::math::Extent2D>,
    );
    same_type(
        PhantomData::<khora_sdk::prelude::math::Extent3D>,
        PhantomData::<khora_core::math::Extent3D>,
    );
    assert_eq!(
        khora_sdk::prelude::math::FRAC_PI_2,
        khora_core::math::FRAC_PI_2
    );
    assert_eq!(
        khora_sdk::prelude::math::FRAC_PI_3,
        khora_core::math::FRAC_PI_3
    );
    assert_eq!(
        khora_sdk::prelude::math::FRAC_PI_4,
        khora_core::math::FRAC_PI_4
    );
    assert_eq!(
        khora_sdk::prelude::math::FRAC_PI_6,
        khora_core::math::FRAC_PI_6
    );
    assert_eq!(
        khora_sdk::prelude::math::FRAC_PI_8,
        khora_core::math::FRAC_PI_8
    );
    assert_eq!(khora_sdk::prelude::math::LN_10, khora_core::math::LN_10);
    assert_eq!(khora_sdk::prelude::math::LN_2, khora_core::math::LN_2);
    assert_eq!(khora_sdk::prelude::math::LOG10_E, khora_core::math::LOG10_E);
    assert_eq!(khora_sdk::prelude::math::LOG2_E, khora_core::math::LOG2_E);
    same_type(
        PhantomData::<khora_sdk::prelude::math::LinearRgba>,
        PhantomData::<khora_core::math::LinearRgba>,
    );
    same_type(
        PhantomData::<khora_sdk::prelude::math::Mat3>,
        PhantomData::<khora_core::math::Mat3>,
    );
    same_type(
        PhantomData::<khora_sdk::prelude::math::Mat4>,
        PhantomData::<khora_core::math::Mat4>,
    );
    same_type(
        PhantomData::<khora_sdk::prelude::math::Origin2D>,
        PhantomData::<khora_core::math::Origin2D>,
    );
    same_type(
        PhantomData::<khora_sdk::prelude::math::Origin3D>,
        PhantomData::<khora_core::math::Origin3D>,
    );
    assert_eq!(khora_sdk::prelude::math::PI, khora_core::math::PI);
    same_type(
        PhantomData::<khora_sdk::prelude::math::Quat>,
        PhantomData::<khora_core::math::Quat>,
    );
    same_type(
        PhantomData::<khora_sdk::prelude::math::Quaternion>,
        PhantomData::<khora_core::math::Quaternion>,
    );
    assert_eq!(
        khora_sdk::prelude::math::RAD_TO_DEG,
        khora_core::math::RAD_TO_DEG
    );
    same_type(
        PhantomData::<khora_sdk::prelude::math::Ray>,
        PhantomData::<khora_core::math::Ray>,
    );
    same_type(
        PhantomData::<khora_sdk::prelude::math::Rect2D>,
        PhantomData::<khora_core::math::Rect2D>,
    );
    assert_eq!(khora_sdk::prelude::math::SQRT_2, khora_core::math::SQRT_2);
    assert_eq!(khora_sdk::prelude::math::TAU, khora_core::math::TAU);
    same_type(
        PhantomData::<khora_sdk::prelude::math::Vec2>,
        PhantomData::<khora_core::math::Vec2>,
    );
    same_type(
        PhantomData::<khora_sdk::prelude::math::Vec3>,
        PhantomData::<khora_core::math::Vec3>,
    );
    same_type(
        PhantomData::<khora_sdk::prelude::math::Vec4>,
        PhantomData::<khora_core::math::Vec4>,
    );
    same_item(
        &khora_sdk::prelude::math::approx_eq,
        &khora_core::math::approx_eq,
    );
    same_item(
        &khora_sdk::prelude::math::approx_eq_eps,
        &khora_core::math::approx_eq_eps,
    );
    same_item(
        &khora_sdk::prelude::math::clamp::<u32>,
        &khora_core::math::clamp::<u32>,
    );
    same_item(
        &khora_sdk::prelude::math::degrees_to_radians,
        &khora_core::math::degrees_to_radians,
    );
    same_item(
        &khora_sdk::prelude::math::radians_to_degrees,
        &khora_core::math::radians_to_degrees,
    );
    same_item(
        &khora_sdk::prelude::math::saturate,
        &khora_core::math::saturate,
    );
    // `khora_sdk::renderer::resource` is `khora_core::renderer::api::resource`: every item in it
    same_type(
        PhantomData::<khora_sdk::renderer::resource::AddressMode>,
        PhantomData::<khora_core::renderer::api::resource::AddressMode>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::BufferDescriptor>,
        PhantomData::<khora_core::renderer::api::resource::BufferDescriptor>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::BufferId>,
        PhantomData::<khora_core::renderer::api::resource::BufferId>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::BufferUsage>,
        PhantomData::<khora_core::renderer::api::resource::BufferUsage>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::CameraUniformData>,
        PhantomData::<khora_core::renderer::api::resource::CameraUniformData>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::CpuShaderSource>,
        PhantomData::<khora_core::renderer::api::resource::CpuShaderSource>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::CpuTexture>,
        PhantomData::<khora_core::renderer::api::resource::CpuTexture>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::FilterMode>,
        PhantomData::<khora_core::renderer::api::resource::FilterMode>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::ImageAspect>,
        PhantomData::<khora_core::renderer::api::resource::ImageAspect>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::MipmapFilterMode>,
        PhantomData::<khora_core::renderer::api::resource::MipmapFilterMode>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::SamplerBorderColor>,
        PhantomData::<khora_core::renderer::api::resource::SamplerBorderColor>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::SamplerDescriptor>,
        PhantomData::<khora_core::renderer::api::resource::SamplerDescriptor>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::SamplerId>,
        PhantomData::<khora_core::renderer::api::resource::SamplerId>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::TextureDescriptor>,
        PhantomData::<khora_core::renderer::api::resource::TextureDescriptor>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::TextureDimension>,
        PhantomData::<khora_core::renderer::api::resource::TextureDimension>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::TextureId>,
        PhantomData::<khora_core::renderer::api::resource::TextureId>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::TextureUsage>,
        PhantomData::<khora_core::renderer::api::resource::TextureUsage>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::TextureViewDescriptor>,
        PhantomData::<khora_core::renderer::api::resource::TextureViewDescriptor>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::TextureViewDimension>,
        PhantomData::<khora_core::renderer::api::resource::TextureViewDimension>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::TextureViewId>,
        PhantomData::<khora_core::renderer::api::resource::TextureViewId>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::resource::ViewInfo>,
        PhantomData::<khora_core::renderer::api::resource::ViewInfo>,
    );
    // `khora_sdk::renderer::scene` is `khora_core::renderer::api::scene`: every item in it
    same_type(
        PhantomData::<khora_sdk::renderer::scene::CullingUniformsData>,
        PhantomData::<khora_core::renderer::api::scene::CullingUniformsData>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::scene::DirectionalLightUniform>,
        PhantomData::<khora_core::renderer::api::scene::DirectionalLightUniform>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::scene::GpuMaterial>,
        PhantomData::<khora_core::renderer::api::scene::GpuMaterial>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::scene::GpuMesh>,
        PhantomData::<khora_core::renderer::api::scene::GpuMesh>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::scene::LightingUniforms>,
        PhantomData::<khora_core::renderer::api::scene::LightingUniforms>,
    );
    assert_eq!(
        khora_sdk::renderer::scene::MAX_DIRECTIONAL_LIGHTS,
        khora_core::renderer::api::scene::MAX_DIRECTIONAL_LIGHTS
    );
    assert_eq!(
        khora_sdk::renderer::scene::MAX_POINT_LIGHTS,
        khora_core::renderer::api::scene::MAX_POINT_LIGHTS
    );
    assert_eq!(
        khora_sdk::renderer::scene::MAX_SPOT_LIGHTS,
        khora_core::renderer::api::scene::MAX_SPOT_LIGHTS
    );
    same_type(
        PhantomData::<khora_sdk::renderer::scene::MaterialUniforms>,
        PhantomData::<khora_core::renderer::api::scene::MaterialUniforms>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::scene::Mesh>,
        PhantomData::<khora_core::renderer::api::scene::Mesh>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::scene::ModelUniforms>,
        PhantomData::<khora_core::renderer::api::scene::ModelUniforms>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::scene::PointLightUniform>,
        PhantomData::<khora_core::renderer::api::scene::PointLightUniform>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::scene::RenderObject>,
        PhantomData::<khora_core::renderer::api::scene::RenderObject>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::scene::SpotLightUniform>,
        PhantomData::<khora_core::renderer::api::scene::SpotLightUniform>,
    );
    // `khora_sdk::renderer::light` is `khora_core::renderer::light`: every item in it
    same_type(
        PhantomData::<khora_sdk::renderer::light::DirectionalLight>,
        PhantomData::<khora_core::renderer::light::DirectionalLight>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::light::LightType>,
        PhantomData::<khora_core::renderer::light::LightType>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::light::PointLight>,
        PhantomData::<khora_core::renderer::light::PointLight>,
    );
    same_type(
        PhantomData::<khora_sdk::renderer::light::SpotLight>,
        PhantomData::<khora_core::renderer::light::SpotLight>,
    );
}

// ---------------------------------------------------------------------------
// Paths other crates of the workspace spell today (`crates/`, `examples/`,
// `xtask/`; brace imports expanded, macro bodies included).
// The trailing comment names the users. Items, modules and enum variants
// are imported; associated items are named in the test below.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod paths_used_by_other_crates {
    use khora_sdk::editor_ui as _; // khora-editor (glob re-export)
    use khora_sdk::editor_ui::viewport_texture::ViewportTextureHandle as _; // khora-editor
    use khora_sdk::editor_ui::AssetEntry as _; // khora-editor
    use khora_sdk::editor_ui::EditorMode as _; // khora-editor
    use khora_sdk::editor_ui::EditorPanel as _; // khora-editor
    use khora_sdk::editor_ui::EditorState as _; // khora-editor
    use khora_sdk::editor_ui::FontFamilyHint as _; // khora-editor
    use khora_sdk::editor_ui::FontHandle as _; // khora-editor
    use khora_sdk::editor_ui::FontPack as _; // khora-editor
    use khora_sdk::editor_ui::GizmoLineInstance as _; // khora-editor
    use khora_sdk::editor_ui::Icon as _; // khora-editor
    use khora_sdk::editor_ui::InspectedEntity as _; // khora-editor
    use khora_sdk::editor_ui::Interaction as _; // khora-editor
    use khora_sdk::editor_ui::NamedFont as _; // khora-editor
    use khora_sdk::editor_ui::PropertyEdit as _; // khora-editor
    use khora_sdk::editor_ui::SceneNode as _; // khora-editor
    use khora_sdk::editor_ui::TextAlign as _; // khora-editor
    use khora_sdk::editor_ui::UiBuilder as _; // khora-editor
    use khora_sdk::editor_ui::UiTheme as _; // khora-editor
    use khora_sdk::instantiate_subtree as _; // khora-editor
    use khora_sdk::khora_core::asset::asset_key as _; // khora-editor
    use khora_sdk::khora_core::asset::AssetHandle as _; // sandbox
    use khora_sdk::khora_core::asset::AssetUUID as _; // khora-editor, sandbox
    use khora_sdk::khora_core::asset::CompressionKind as _; // khora-editor
    use khora_sdk::khora_core::math::Aabb as _; // khora-editor
    use khora_sdk::khora_core::math::Extent3D as _; // sandbox
    use khora_sdk::khora_core::math::Mat4 as _; // khora-editor
    use khora_sdk::khora_core::math::Ray as _; // khora-editor
    use khora_sdk::khora_core::math::Vec3 as _; // khora-editor
    use khora_sdk::khora_core::platform::InputBinding as _; // sandbox
    use khora_sdk::khora_core::platform::InputMap as _; // sandbox
    use khora_sdk::khora_core::platform::KhoraWindow as _; // khora-editor
    use khora_sdk::khora_core::renderer::api::resource::CpuTexture as _; // sandbox
    use khora_sdk::khora_core::renderer::api::resource::TextureDimension as _; // sandbox
    use khora_sdk::khora_core::renderer::api::resource::TextureUsage as _; // sandbox
    use khora_sdk::khora_core::renderer::api::resource::ViewInfo as _; // khora-editor
    use khora_sdk::khora_core::renderer::api::scene::mesh::Mesh as _; // khora-editor
    use khora_sdk::khora_core::renderer::api::scene::Mesh as _; // khora-editor
    use khora_sdk::khora_core::renderer::api::util::SampleCount as _; // sandbox
    use khora_sdk::khora_core::renderer::api::util::TextureFormat as _; // sandbox
    use khora_sdk::khora_core::renderer::light::DirectionalLight as _; // khora-editor
    use khora_sdk::khora_core::renderer::light::LightType as _; // khora-editor
    use khora_sdk::khora_core::renderer::light::PointLight as _; // khora-editor
    use khora_sdk::khora_core::renderer::light::SpotLight as _; // khora-editor
    use khora_sdk::khora_core::ui::EditorOverlay as _; // khora-editor
    use khora_sdk::khora_core::ui::OverlayScreenDescriptor as _; // khora-editor
    use khora_sdk::khora_data::ecs::material_to_json as _; // khora-editor
    use khora_sdk::khora_data::ecs::SemanticDomain as _; // khora-editor
    use khora_sdk::khora_data::ecs::Tag as _; // khora-editor
    use khora_sdk::khora_data::ecs::Teleported as _; // khora-editor
    use khora_sdk::khora_data::render::extract_active_camera_view as _; // khora-editor
    use khora_sdk::khora_data::render::EditorViewportOverride as _; // khora-editor
    use khora_sdk::khora_data::render::ExtractedView as _; // khora-editor
    use khora_sdk::khora_data::render::SharedGizmoFrame as _; // khora-editor
    use khora_sdk::khora_data::render::SharedGridConfig as _; // khora-editor
    use khora_sdk::khora_data::render::SharedWireframeConfig as _; // khora-editor
    use khora_sdk::khora_data::scene::provenance_of as _; // khora-editor
    use khora_sdk::khora_data::AssetStore as _; // sandbox
    use khora_sdk::prelude as _; // khora-editor, khora-runtime, sandbox (glob re-export)
    use khora_sdk::prelude::ecs as _; // khora-editor (glob re-export)
    use khora_sdk::prelude::ecs::AudioSource as _; // khora-editor
    use khora_sdk::prelude::ecs::Camera as _; // khora-editor
    use khora_sdk::prelude::ecs::EntityId as _; // khora-editor, sandbox
    use khora_sdk::prelude::ecs::GlobalTransform as _; // khora-editor
    use khora_sdk::prelude::ecs::Light as _; // khora-editor
    use khora_sdk::prelude::ecs::Name as _; // khora-editor
    use khora_sdk::prelude::ecs::Transform as _; // khora-editor, sandbox
    use khora_sdk::prelude::materials::StandardMaterial as _; // khora-editor, sandbox
    use khora_sdk::prelude::math::LinearRgba as _; // khora-editor
    use khora_sdk::prelude::math::Quaternion as _; // sandbox
    use khora_sdk::prelude::math::Vec3 as _; // khora-editor, sandbox
    use khora_sdk::prelude::MouseButton as _; // sandbox
    use khora_sdk::prelude::SaaTrackingAllocator as _; // khora-editor, khora-runtime, sandbox, khora-hub (project template)
    use khora_sdk::prelude::SharedTime as _; // khora-editor, sandbox
    use khora_sdk::run_default as _; // khora-runtime
    use khora_sdk::run_winit as _; // khora-editor, sandbox
    use khora_sdk::scripts::mount as _; // sandbox
    use khora_sdk::serialize_subtree as _; // khora-editor
    use khora_sdk::spawn_cube_at as _; // khora-editor
    use khora_sdk::spawn_plane as _; // khora-editor, sandbox
    use khora_sdk::spawn_sphere as _; // khora-editor, sandbox
    use khora_sdk::tool_ui as _; // hub
    use khora_sdk::tool_ui::App as _; // hub
    use khora_sdk::tool_ui::AppContext as _; // hub
    use khora_sdk::tool_ui::FontFamilyHint as _; // hub
    use khora_sdk::tool_ui::FontHandle as _; // hub
    use khora_sdk::tool_ui::FontPack as _; // hub
    use khora_sdk::tool_ui::Icon as _; // hub
    use khora_sdk::tool_ui::Interaction as _; // hub
    use khora_sdk::tool_ui::NamedFont as _; // hub
    use khora_sdk::tool_ui::UiBuilder as _; // hub
    use khora_sdk::tool_ui::UiTheme as _; // hub
    use khora_sdk::tool_ui::WindowConfigInput as _; // hub
    use khora_sdk::tool_ui::WindowIconInput as _; // hub
    use khora_sdk::winit as _; // khora-editor
    use khora_sdk::winit_adapters::WinitWindowProvider as _; // khora-editor, sandbox
    use khora_sdk::AgentId as _; // khora-editor
    use khora_sdk::AgentImportance as _; // khora-editor
    use khora_sdk::AgentProvider as _; // khora-editor, sandbox
    use khora_sdk::AgentRegistry as _; // khora-editor
    use khora_sdk::AgentStatus as _; // khora-editor
    use khora_sdk::AssetChangeEvent as _; // khora-editor
    use khora_sdk::AssetChangeKind as _; // khora-editor
    use khora_sdk::AssetIdRegistry as _; // khora-editor
    use khora_sdk::AssetIo as _; // khora-editor
    use khora_sdk::AssetService as _; // khora-editor
    use khora_sdk::AssetSource as _; // khora-editor
    use khora_sdk::AssetWatcher as _; // khora-editor
    use khora_sdk::AssetWriter as _; // khora-editor
    use khora_sdk::AudioDevice as _; // khora-editor, sandbox
    use khora_sdk::AudioMixBus as _; // khora-editor, sandbox
    use khora_sdk::AudioStream as _; // khora-editor, sandbox
    use khora_sdk::ComponentRegistration as _; // khora-editor
    use khora_sdk::CpalAudioDevice as _; // khora-editor, sandbox
    use khora_sdk::DccContext as _; // khora-editor
    use khora_sdk::DccService as _; // khora-editor, sandbox
    use khora_sdk::DefaultMixBus as _; // khora-editor, sandbox
    use khora_sdk::EditorMode as _; // khora-editor
    use khora_sdk::EditorShell as _; // khora-editor
    use khora_sdk::EditorState as _; // khora-editor
    use khora_sdk::EngineApp as _; // khora-editor, sandbox
    use khora_sdk::ExecutionPhase as _; // khora-editor, sandbox
    use khora_sdk::FileLoader as _; // khora-editor
    use khora_sdk::FileSystemResolver as _; // khora-editor
    use khora_sdk::GameWorld as _; // khora-editor, sandbox
    use khora_sdk::GizmoMode as _; // khora-editor
    use khora_sdk::HandleComponent as _; // khora-editor
    use khora_sdk::IndexBuilder as _; // khora-editor
    use khora_sdk::InputEvent as _; // khora-editor, sandbox
    use khora_sdk::KeyCode as _; // khora-editor, sandbox
    use khora_sdk::LayoutSystem as _; // khora-editor, sandbox
    use khora_sdk::LogEntry as _; // khora-editor
    use khora_sdk::MeshDispatcher as _; // khora-editor
    use khora_sdk::MetricsRegistry as _; // khora-editor
    use khora_sdk::MonitorRegistry as _; // khora-editor
    use khora_sdk::MonitoredResourceType as _; // khora-editor
    use khora_sdk::PackBuilder as _; // khora-editor
    use khora_sdk::PanelLocation as _; // khora-editor
    use khora_sdk::PhaseProvider as _; // khora-editor, sandbox
    use khora_sdk::PhysicsProvider as _; // khora-editor, sandbox
    use khora_sdk::PipelineSystem as _; // khora-editor, sandbox
    use khora_sdk::PlayMode as _; // khora-editor
    use khora_sdk::RapierPhysicsWorld as _; // khora-editor, sandbox
    use khora_sdk::RenderSystem as _; // khora-editor, sandbox
    use khora_sdk::Runtime as _; // khora-editor, sandbox
    use khora_sdk::RuntimeConfig as _; // khora-editor
    use khora_sdk::SceneFile as _; // khora-editor
    use khora_sdk::SerializationGoal as _; // khora-editor
    use khora_sdk::SerializationService as _; // khora-editor
    use khora_sdk::SoundData as _; // khora-editor
    use khora_sdk::StandardTextRenderer as _; // khora-editor, sandbox
    use khora_sdk::StrategyId as _; // khora-editor
    use khora_sdk::StreamInfo as _; // khora-editor, sandbox
    use khora_sdk::SymphoniaDecoder as _; // khora-editor
    use khora_sdk::TaffyLayoutSystem as _; // khora-editor, sandbox
    use khora_sdk::TextRenderer as _; // khora-editor, sandbox
    use khora_sdk::WgpuPipelineSystem as _; // khora-editor, sandbox
    use khora_sdk::WgpuRenderSystem as _; // khora-editor, sandbox
    use khora_sdk::WindowConfig as _; // sandbox
    use khora_sdk::DEFAULT_SCENE_REL_PATH as _; // khora-editor
    use khora_sdk::EGUI_WGSL as _; // khora-editor
    use khora_sdk::PRIMARY_VIEWPORT as _; // khora-editor
    use khora_sdk::RUNTIME_CONFIG_FILE as _; // khora-editor
    use khora_sdk::TEXT_WGSL as _; // khora-editor, sandbox
}

#[test]
fn associated_items_used_by_other_crates_still_resolve() {
    let _ = khora_sdk::editor_ui::FontFamilyHint::Monospace; // khora-editor
    let _ = khora_sdk::khora_core::asset::AssetUUID::new_v5; // sandbox
    let _ = khora_sdk::khora_core::ui::UiTheme::default; // khora-editor
    let _ = khora_sdk::khora_data::render::EditorViewportOverride::new; // khora-editor
    let _ = khora_sdk::prelude::ecs::Camera::new_perspective; // sandbox
    let _ = khora_sdk::prelude::ecs::GlobalTransform::default; // sandbox
    let _ = khora_sdk::prelude::ecs::Light::directional; // sandbox
    let _ = khora_sdk::prelude::ecs::Light::point; // sandbox
    let _ = khora_sdk::prelude::ecs::LightType::Directional; // sandbox
    let _ = khora_sdk::prelude::ecs::LightType::Point; // sandbox
    let _: fn(String, String) -> khora_sdk::prelude::ecs::Script =
        khora_sdk::prelude::ecs::Script::new; // sandbox
    let _ = khora_sdk::prelude::ecs::ScriptValue::Float; // sandbox
    let _ = khora_sdk::prelude::ecs::Transform::from_translation; // sandbox
    let _ = khora_sdk::prelude::materials::AlphaMode::Blend; // sandbox
    let _ = khora_sdk::prelude::math::LinearRgba::new; // khora-editor, sandbox
    let _ = khora_sdk::prelude::math::LinearRgba::rgb; // sandbox
    let _ = khora_sdk::prelude::math::LinearRgba::BLUE; // sandbox
    let _ = khora_sdk::prelude::math::LinearRgba::CYAN; // sandbox
    let _ = khora_sdk::prelude::math::LinearRgba::GREEN; // sandbox
    let _ = khora_sdk::prelude::math::LinearRgba::RED; // sandbox
    let _ = khora_sdk::prelude::math::LinearRgba::YELLOW; // sandbox
    let _ = khora_sdk::prelude::math::Vec3::new; // khora-editor
    let _ = khora_sdk::prelude::math::Vec3::ZERO; // khora-editor
    let _ = khora_sdk::prelude::SaaTrackingAllocator::<std::alloc::System>::new; // khora-editor
    let _ = khora_sdk::tool_ui::FontFamilyHint::Monospace; // hub
    let _ = khora_sdk::tool_ui::Icon::Github; // hub
    let _ = khora_sdk::tool_ui::TextAlign::Center; // hub
    let _: fn(std::path::PathBuf) -> anyhow::Result<khora_sdk::AssetWatcher> =
        khora_sdk::AssetWatcher::new; // sandbox
    let _ = khora_sdk::EngineMode::Custom; // khora-editor
    let _ = khora_sdk::EngineMode::Playing; // khora-editor
    let _ = khora_sdk::EnvironmentMap::from_asset; // sandbox
    let _ = khora_sdk::KeyCode::F2; // khora-editor
    let _ = khora_sdk::MetricsRegistry::new; // khora-editor
    let _ = khora_sdk::Vessel::at; // sandbox
}
