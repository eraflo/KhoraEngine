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

//! The engine's own runtime entries, installed before the application's
//! `setup` so it can read them.

use khora_core::event::Channel;
use khora_core::Runtime;
use khora_data::render::{FrameGraph, SharedFrameGraph};
use std::sync::{Arc, Mutex};

use crate::InputEvent;

/// Installs in `runtime` the engine's own resources, `input_events` published
/// as the raw input stream.
pub(super) fn install_engine_resources(runtime: &mut Runtime, input_events: &Channel<InputEvent>) {
    // The overlay views the host writes and the overlay lanes read. The engine
    // only provides them, each empty: no gizmo line, the grid and wireframe
    // off. The editor sets them, from its `setup` on.
    let host_views = khora_core::lane::HostViews::new();
    host_views.set(khora_data::render::GizmoView::default());
    host_views.set(khora_data::render::GridView::default());
    host_views.set(khora_data::render::WireframeView::default());
    runtime.resources.insert(Arc::new(host_views));

    // The engine-wide store of GPU assets and its projection. The app fills the
    // CPU textures, as it owns the `AssetService`; the projection only uploads
    // them, since `khora-data` cannot depend on `khora-io`. Neither needs a
    // device, so an app can register assets from its `setup`.
    let asset_store = khora_data::AssetStore::new();
    let proj_registry = khora_data::ProjectionRegistry::new(asset_store.clone());
    runtime.resources.insert(asset_store);
    runtime.resources.insert(proj_registry);

    // The scene environment baked into image-based lighting, once, by the
    // `ibl_bake` system on the first frame a device is available.
    runtime.resources.insert(khora_data::IblBaker::new());

    // The engine-wide action map, derived from the channel below by the
    // `input_map_update` system. An app binds actions and caches it in `setup`.
    runtime
        .resources
        .insert(Arc::new(Mutex::new(khora_core::platform::InputMap::new())));

    // The raw stream, for a consumer that needs what `InputMap` cannot say:
    // the order of two clicks, a double tap, the cursor's motion.
    runtime.resources.insert(input_events.clone());

    // Where the physics lane's contacts land, installed with no listener yet:
    // a channel left to its first writer to create is one forgotten, and a
    // frame of contacts dropped silently.
    runtime
        .resources
        .insert(khora_core::physics::collision_channel());

    // The frame's render passes, which `submit_passes` gathers and submits.
    let frame_graph: SharedFrameGraph = Arc::new(Mutex::new(FrameGraph::new()));
    runtime.resources.insert(frame_graph);

    // The `PipelineSystem` is not installed here: the app's bootstrap brings it.

    // Ticked each frame by the `ecs_maintenance` system.
    runtime
        .resources
        .insert(Arc::new(Mutex::new(khora_data::ecs::EcsMaintenance::new())));

    // Ticked each frame by the `asset_eviction` system: it frees the GPU
    // meshes and materials no entity uses any more, which the insert-only
    // projection would otherwise leak.
    runtime
        .resources
        .insert(Arc::new(Mutex::new(khora_data::AssetEviction::new())));

    // The engine's clock: the scheduler writes the frame's delta, fixed step
    // and interpolation alpha; Flows and the game's `update` read it. Behind a
    // lock, since the scheduler writes it through a shared `Runtime`.
    let time: khora_core::time::SharedTime =
        Arc::new(std::sync::RwLock::new(khora_core::time::Time::default()));
    runtime.resources.insert(time);

    // What the background families staged, kept until a frame's `Boundary`
    // systems take it. A host's, installed before the boot, is kept: a mount
    // may have staged into it.
    if runtime
        .resources
        .get::<khora_core::lane::Reserve>()
        .is_none()
    {
        runtime.resources.insert(khora_core::lane::Reserve::new());
    }

    // Which families run, known from setup on.
    runtime
        .resources
        .insert(super::families::engine_activation());

    // Each entity's previous pose, which the render projection blends by the
    // interpolation alpha. A resource, not a component, so it never shows in
    // the editor nor in scene files: interpolation is render-only.
    let transform_interpolation: khora_core::interpolation::SharedTransformInterpolation = Arc::new(
        std::sync::RwLock::new(khora_core::interpolation::TransformInterpolation::new()),
    );
    runtime.resources.insert(transform_interpolation);

    // Where each UI image sits in its atlas; the GPU atlas is allocated by the
    // `image_on_gpu` residency, once an image is wanted.
    runtime
        .resources
        .insert(Arc::new(khora_data::ui::UiImageAtlas::new()));
    // The glyphs of the UI's texts, placed by `glyphs_on_gpu` and drawn by the
    // UI lane; its two textures are made on first need.
    runtime
        .resources
        .insert(Arc::new(khora_data::ui::GlyphAtlas::default()));
}

/// Installs in `runtime` what scripting and the `loading` family read, and the
/// readiness a tool's startup waits on.
pub(super) fn install_scripting_and_loading(runtime: &mut Runtime) {
    // The scripting world is a service, not the script agent's: an agent holds
    // strategy state only, and an inspector or a debugger sees live behaviours
    // without going through it.
    let script_runtime: Arc<Mutex<khora_lanes::script::ScriptRuntime>> =
        Arc::new(Mutex::new(khora_lanes::script::ScriptRuntime::new()));
    runtime.services.insert(script_runtime);

    // What every application needs to mount scripts. A driver may install it
    // before its bootstrap closure mounts scripts there: this replaces nothing,
    // so no program compiled at startup is dropped.
    crate::scripts::install(runtime);

    // What the `loading` family reads: the asset service's request channel and
    // its decoding, or stand-ins for an engine without one.
    khora_io::asset::install_decoding(runtime);
    // What the `loading` family reads: the hub hosts file project indexes with.
    khora_io::asset::install_indexing(runtime);
    // What the `loading` family reads: the hub hosts file scene loads with.
    khora_io::scene_loads::install_scene_loading(runtime);
    // What a tool's startup waits on — the tickets it files with the hubs
    // above — settled at every frame's boundary.
    if runtime
        .resources
        .get::<Arc<super::readiness::Readiness>>()
        .is_none()
    {
        runtime
            .resources
            .insert(Arc::new(super::readiness::Readiness::new()));
    }
}
