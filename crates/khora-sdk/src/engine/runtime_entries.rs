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
//! setup so it can read them: the overlay views, the data layer's GPU
//! stores, input, time, the frame graph, the UI atlases, and what scripting
//! and loading read.

use khora_core::event::Channel;
use khora_core::Runtime;
use khora_data::render::{FrameGraph, SharedFrameGraph};
use std::sync::{Arc, Mutex};

use crate::InputEvent;

/// Installs in `runtime` the engine's resources: the overlay views, the
/// asset store and its projection, the IBL baker, input, the frame graph,
/// maintenance, time, the reserve, the families' activation, interpolation
/// and the UI atlases. `input_events` is the raw input stream published.
pub(super) fn install_engine_resources(runtime: &mut Runtime, input_events: &Channel<InputEvent>) {
    // ── Overlay channels (gizmo + grid) ──────────────────────────────────
    // Shared resources the host application (editor, debug tooling)
    // writes into each frame; the overlay families' lanes read them in
    // the OUTPUT phase. The engine only provides the slots — it never
    // produces the data itself, keeping the editor a pure consumer
    // of engine APIs (no engine-internal `EditorAgent`).
    //
    // Inserted BEFORE `app.setup` (like the DCC's observable handles)
    // so an app can configure them during setup — e.g. the editor
    // enables the grid via `GridConfig` there. They are standalone
    // `Arc<Mutex<Default>>` with no dependency on later-created
    // resources, so exposing them this early is safe.
    // Each overlay starts with its empty View: no gizmo line, the grid and
    // the wireframe off. The editor opts in by setting them again.
    let host_views = khora_core::lane::HostViews::new();
    host_views.set(khora_data::render::GizmoView::default());
    host_views.set(khora_data::render::GridView::default());
    host_views.set(khora_data::render::WireframeView::default());
    runtime.resources.insert(Arc::new(host_views));

    // ── Data-layer GPU resources ─────────────────────────────────────
    // AssetStore: the single engine-wide store of projected GPU assets
    // (Assets<GpuMesh> / GpuMaterial / CpuTexture sub-stores, created on
    // demand). The SDK/app fills the CpuTexture sub-store (it owns the
    // AssetService); the data-layer projection (ProjectionRegistry, runs
    // sync_all/sync_materials once per frame in PreExtract) only uploads
    // from it (khora-data must not depend on khora-io).
    //
    // Inserted BEFORE `app.setup` so an app can register assets (e.g.
    // decoded textures) into the store during setup. Neither needs a
    // graphics device at construction, so this is safe this early.
    let asset_store = khora_data::AssetStore::new();
    let proj_registry = khora_data::ProjectionRegistry::new(asset_store.clone());
    runtime.resources.insert(asset_store);
    runtime.resources.insert(proj_registry);

    // IBL bake service — projects the scene environment (procedural sky
    // for now) into the GPU cubes/LUT image-based lighting samples. Baked
    // once by the `ibl_bake` DataSystem on the first frame a device is
    // available; the lit lanes read the result at group 3.
    runtime.resources.insert(khora_data::IblBaker::new());

    // InputMap — engine-wide action / binding map. Inserted BEFORE
    // `app.setup` so apps can bind actions and cache the handle during
    // setup (e.g. the sandbox's PlayerController). The `input_map_update`
    // data system derives it from the channel below, once per frame in
    // `PreSimulation`.
    runtime
        .resources
        .insert(Arc::new(Mutex::new(khora_core::platform::InputMap::new())));

    // The raw stream `InputMap` is derived from, published so the descent
    // can reach it. `InputMap` answers "is jump held"; a consumer that
    // needs the order of two clicks, a double-tap, or the cursor's motion
    // needs the events, which the map's three `HashSet`s cannot express.
    // The engine's own handle is a clone of this one.
    runtime.resources.insert(input_events.clone());

    // Where the physics lane's contacts land, for anything that listens.
    // Inserted whether or not a consumer exists yet: a channel somebody has
    // to create before writing to it is a channel somebody forgets to
    // create, and the dispatch would then drop a frame of contacts with
    // nothing to say about it.
    runtime
        .resources
        .insert(khora_core::physics::collision_channel());

    // Frame graph — per-frame collection of render passes recorded by
    // agents during OUTPUT; `tick_with_services()` drains + submits it.
    let frame_graph: SharedFrameGraph = Arc::new(Mutex::new(FrameGraph::new()));
    runtime.resources.insert(frame_graph);

    // Shader/pipeline composition is owned by the `PipelineSystem` backend
    // (`WgpuPipelineSystem`), injected into `runtime.resources` by the app
    // bootstrap; render lanes resolve their layouts + pipelines through it.

    // EcsMaintenance — fetched + ticked each frame by the `ecs_maintenance`
    // DataSystem (Maintenance phase).
    runtime
        .resources
        .insert(Arc::new(Mutex::new(khora_data::ecs::EcsMaintenance::new())));

    // AssetEviction — fetched + ticked each frame by the `asset_eviction`
    // DataSystem (Maintenance phase). Reclaims orphaned GPU meshes/materials
    // (from despawns and inline material edits) that the insert-only
    // projection would otherwise leak, freeing their wgpu resources.
    runtime
        .resources
        .insert(Arc::new(Mutex::new(khora_data::AssetEviction::new())));

    // Time — the engine's per-frame clock. The scheduler publishes the
    // real frame delta + fixed step + render-interpolation alpha into it
    // each frame; Flows and game `update` read it (replacing hardcoded
    // deltas). Shared behind RwLock so the scheduler (holding Arc<Runtime>)
    // writes while readers borrow `&Runtime`. Inserted before `app.setup`
    // so apps can read it during setup.
    let time: khora_core::time::SharedTime =
        Arc::new(std::sync::RwLock::new(khora_core::time::Time::default()));
    runtime.resources.insert(time);

    // The reserve: what the families at the background staged, kept across
    // frames until a frame's `Boundary` systems take it.
    // Kept when the host installed one already: a mount before the bootstrap
    // staged into it.
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

    // TransformInterpolation — engine-owned per-entity "previous pose" store
    // the capture pass fills and the render projection blends by the
    // interpolation alpha. A resource, not an ECS component, so it never
    // appears in the editor or scene files (interpolation is render-only).
    let transform_interpolation: khora_core::interpolation::SharedTransformInterpolation = Arc::new(
        std::sync::RwLock::new(khora_core::interpolation::TransformInterpolation::new()),
    );
    runtime.resources.insert(transform_interpolation);

    // UiImageAtlas — `AssetUUID → AtlasRect` mapping for UI images; the GPU
    // atlas itself is allocated by the `image_on_gpu` residency, once an
    // image is wanted.
    runtime
        .resources
        .insert(Arc::new(khora_data::ui::UiImageAtlas::new()));
    // GlyphAtlas — the glyphs of the UI's texts, placed by `glyphs_on_gpu`
    // and drawn by the UI lane; its two textures are made on first need.
    runtime
        .resources
        .insert(Arc::new(khora_data::ui::GlyphAtlas::default()));
}

/// Installs in `runtime` what scripting and the `loading` family read: the
/// scripting world, the script mounts, the asset decoding and indexing, the
/// scene loads, and the readiness a tool's startup waits on.
pub(super) fn install_scripting_and_loading(runtime: &mut Runtime) {
    // The scripting world — compiled programs, live behavior instances, the
    // mail behaviors owe each other. Registered here rather than owned by
    // the script family's agent for the same reason the physics provider
    // is: an agent chooses a lane against a budget, and a subsystem's state
    // is not strategy state. It also means anything that legitimately needs to see
    // live behaviors — an inspector, a debugger — can, without going
    // through the agent.
    let script_runtime: Arc<Mutex<khora_lanes::script::ScriptRuntime>> =
        Arc::new(Mutex::new(khora_lanes::script::ScriptRuntime::new()));
    runtime.services.insert(script_runtime);

    // What scripting needs — the engine's `AssetWatch`, the queues that
    // feed the scripting world, the catalog — for **every** application
    // rather than by one binary's launcher. A windowing driver installs
    // them before the application's own bootstrap closure, which may mount
    // its scripts there; this finds them in place and replaces nothing, so
    // no program compiled at startup is dropped. Filling them is the
    // application's job — `khora_sdk::scripts::mount`.
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
