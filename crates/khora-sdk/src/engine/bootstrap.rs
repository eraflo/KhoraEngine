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

//! Assembling the engine: services, agents, lanes and the scheduler, once,
//! before the first frame.

use khora_control::{DccConfig, DccService};
use khora_core::Runtime;
use khora_data::render::{FrameGraph, SharedFrameGraph};
use khora_telemetry::TelemetryService;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::EngineCore;
use crate::traits::EngineApp;
use crate::GameWorld;

impl<A: EngineApp> EngineCore<A> {
    /// Bootstraps the engine: creates DCC, telemetry, scheduler,
    /// registers agents, calls `app.setup()`, and initializes agents.
    ///
    /// This method takes ownership of the `runtime` populated
    /// by the windowing driver's bootstrap closure.  It wraps the runtime
    /// in an `Arc` once all built-in entries have been inserted.
    pub fn bootstrap(&mut self, mut app: A, mut runtime: Runtime) {
        // Create DCC + telemetry
        let (mut dcc, dcc_rx) = DccService::new(DccConfig::default());
        let telemetry =
            TelemetryService::new(Duration::from_secs(1)).with_dcc_sender(dcc.event_sender());

        // Register the system-RAM monitor so the tracking allocator's live
        // stats flow through telemetry into the DCC, where they drive memory
        // pressure + allocation-churn signals (it is no longer write-only).
        telemetry.monitor_registry().register(std::sync::Arc::new(
            khora_infra::telemetry::memory_monitor::MemoryMonitor::new("System_RAM".to_string()),
        ));

        // ── Expose observable handles via Resources ─────────────────────
        // Apps (e.g. the editor) read live engine state (monitors, agent
        // list, DCC context) through these handles. They're cheap clones of
        // internal Arc-shared structures, so doing so before `app.setup` is
        // safe.
        runtime
            .resources
            .insert(telemetry.monitor_registry().clone());
        runtime.resources.insert(dcc.agent_registry().clone());
        // Live DCC context: shared `Arc<RwLock<Context>>` updated by the
        // DCC cold thread, read by observers each frame.
        runtime.resources.insert(dcc.context_handle());

        // ── Overlay channels (gizmo + grid) ──────────────────────────────────
        // Shared resources the host application (editor, debug tooling)
        // writes into each frame; the `OverlayAgent` lanes read them in
        // the OUTPUT phase. The engine only provides the slots — it never
        // produces the data itself, keeping the editor a pure consumer
        // of engine APIs (no engine-internal `EditorAgent`).
        //
        // Inserted BEFORE `app.setup` (like the observable handles above)
        // so an app can configure them during setup — e.g. the editor
        // enables the grid via `GridConfig` there. They are standalone
        // `Arc<Mutex<Default>>` with no dependency on later-created
        // resources, so exposing them this early is safe.
        let gizmo_frame: khora_data::render::SharedGizmoFrame =
            Arc::new(Mutex::new(khora_data::render::GizmoFrame::default()));
        runtime.resources.insert(gizmo_frame);
        // Editor grid — disabled by default; the editor opts in.
        let grid_config: khora_data::render::SharedGridConfig =
            Arc::new(Mutex::new(khora_data::render::GridConfig::default()));
        runtime.resources.insert(grid_config);
        // Wireframe debug overlay — disabled by default; the editor opts in.
        let wireframe_config: khora_data::render::SharedWireframeConfig =
            Arc::new(Mutex::new(khora_data::render::WireframeConfig::default()));
        runtime.resources.insert(wireframe_config);

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
        runtime.resources.insert(self.input_events.clone());

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

        // TransformInterpolation — engine-owned per-entity "previous pose" store
        // the capture pass fills and the render projection blends by the
        // interpolation alpha. A resource, not an ECS component, so it never
        // appears in the editor or scene files (interpolation is render-only).
        let transform_interpolation: khora_core::interpolation::SharedTransformInterpolation =
            Arc::new(std::sync::RwLock::new(
                khora_core::interpolation::TransformInterpolation::new(),
            ));
        runtime.resources.insert(transform_interpolation);

        // UiImageAtlas — `AssetUUID → AtlasRect` mapping for UI images; the GPU
        // atlas itself is allocated lazily by `UiAgent::on_initialize`.
        runtime
            .resources
            .insert(Arc::new(khora_data::ui::UiImageAtlas::new()));

        // AgentFrameStatus map — per-agent execution metrics measured and
        // written by the scheduler each frame; agents read their own slot in
        // `report_status` instead of holding per-frame counters.
        let agent_frame_status: khora_core::agent::gorna::AgentFrameStatusMap =
            Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()));
        runtime.resources.insert(agent_frame_status);

        // The scripting world — compiled programs, live behavior instances, the
        // mail behaviors owe each other. Registered here rather than owned by
        // `ScriptAgent` for the same reason the physics provider is: an
        // agent chooses a lane against a budget, and a subsystem's state is not
        // strategy state. It also means anything that legitimately needs to see
        // live behaviors — an inspector, a debugger — can, without going
        // through the agent.
        let script_runtime: Arc<Mutex<khora_lanes::script_lane::ScriptRuntime>> =
            Arc::new(Mutex::new(khora_lanes::script_lane::ScriptRuntime::new()));
        runtime.services.insert(script_runtime);

        // The two queues that feed it, inserted for **every** application
        // rather than by one binary's launcher.
        //
        // They used to live in `run_default`, which only `khora-runtime` calls.
        // The editor and the sandbox therefore ran the scripting agent and its
        // lane over a runtime whose program table could never be filled — a
        // correct `.erg` on a correct entity produced no behaviour and no
        // message. An empty channel costs nothing; not having one costs a
        // subsystem that looks wired and is not.
        //
        // Filling them is the application's job, because the root to watch is
        // the exe's `assets/` for a game and the open project for the editor.
        // See `khora_sdk::scripts::mount`.
        // Inserted only if absent. A windowing driver runs the application's
        // bootstrap closure *before* this, and an app that mounted its scripts
        // there has already filled this queue — overwriting it with a fresh one
        // would drop every program compiled at startup, silently.
        if runtime
            .resources
            .get::<khora_io::script::hot_reload::PendingReloads>()
            .is_none()
        {
            runtime
                .resources
                .insert(khora_io::script::hot_reload::reload_channel());
        }
        if runtime
            .resources
            .get::<khora_core::event::Channel<khora_core::script::ScriptEvent>>()
            .is_none()
        {
            runtime
                .resources
                .insert(khora_core::script::engine_event_channel());
        }

        // Create the game world
        let mut game_world = GameWorld::new();

        // Call app setup. The runtime is now fully populated, so the app can
        // read any engine resource here. Mutation goes through `register_agents`.
        app.setup(&mut game_world, &runtime);

        // Register agents via the app's AgentProvider trait.
        app.register_agents(&dcc, &mut runtime);

        let runtime_arc = Arc::new(runtime);

        // Register built-in agents (always present). Agents implement only the
        // `Agent` trait + `Default`, so construction goes through `Default::default()`.
        dcc.register_agent(
            Arc::new(Mutex::new(
                khora_agents::render_agent::RenderAgent::default(),
            )),
            1.0,
        );
        dcc.register_agent(
            Arc::new(Mutex::new(
                khora_agents::shadow_agent::ShadowAgent::default(),
            )),
            1.0,
        );
        dcc.register_agent(
            Arc::new(Mutex::new(
                khora_agents::overlay_agent::OverlayAgent::default(),
            )),
            1.0,
        );
        dcc.register_agent(
            Arc::new(Mutex::new(
                khora_agents::skybox_agent::SkyboxAgent::default(),
            )),
            1.0,
        );
        dcc.register_agent(
            Arc::new(Mutex::new(
                khora_agents::physics_agent::PhysicsAgent::default(),
            )),
            1.0,
        );
        dcc.register_agent(
            Arc::new(Mutex::new(khora_agents::ui_agent::UiAgent::default())),
            1.0,
        );
        dcc.register_agent(
            Arc::new(Mutex::new(khora_agents::audio_agent::AudioAgent::default())),
            1.0,
        );
        // Gameplay. Registered like any other consumer, which is the point of
        // giving scripting an agent at all: the DCC can tell it "you have
        // 0.4ms, hand back control", and Ergon is the only scripting language
        // in reach that can be told that.
        dcc.register_agent(
            Arc::new(Mutex::new(
                khora_agents::script_agent::ScriptAgent::default(),
            )),
            1.0,
        );

        // Initialize agents with the full runtime so on_initialize() can
        // find Arc<dyn GraphicsDevice>, Arc<Mutex<Box<dyn RenderSystem>>>,
        // AssetStore, etc. via the typed runtime containers.
        {
            let init_bus = khora_core::lane::LaneBus::new();
            let mut init_deck = khora_core::lane::OutputDeck::new();
            let mut init_ctx = khora_core::EngineContext::for_initialisation(
                Arc::clone(&runtime_arc),
                &init_bus,
                &mut init_deck,
            );
            dcc.initialize_agents(&mut init_ctx);
        }
        // Start the DCC background thread AFTER agents are initialized
        // so GORNA does not run health-checks before agents are ready.
        dcc.start(dcc_rx);

        // Build scheduler
        let agent_ids = vec![
            khora_core::agent::gorna::AgentId::Renderer,
            khora_core::agent::gorna::AgentId::ShadowRenderer,
            khora_core::agent::gorna::AgentId::Overlay,
            khora_core::agent::gorna::AgentId::Skybox,
            khora_core::agent::gorna::AgentId::Physics,
            khora_core::agent::gorna::AgentId::Ui,
            khora_core::agent::gorna::AgentId::Audio,
            khora_core::agent::gorna::AgentId::Script,
        ];

        let registry = dcc.agent_registry().clone();
        let mut scheduler =
            khora_control::ExecutionScheduler::new(registry, self.context.clone(), &agent_ids);

        // Connect the read-only observation tunnel: the scheduler publishes
        // per-agent cost samples + per-component access snapshots the DCC's
        // cost model and layout advisor consume.
        scheduler.set_telemetry_sender(dcc.event_sender());

        // Run phases through the parallel wave executor: agents declaring
        // `AgentAccess::Isolated` / `SharedWorld` (no `&mut World`, disjoint
        // outputs) run concurrently within a phase, the rest serially. With
        // the current agents this makes `[Ui, Overlay]` a concurrent wave;
        // pass submission stays deterministic (fixed scene→ui→overlay fold).
        scheduler.set_parallel_execution(true);

        // Inject custom phases from the app
        let custom_phases = app.custom_phases();
        for phase in custom_phases {
            scheduler.insert_after(khora_core::agent::ExecutionPhase::OUTPUT, phase);
        }

        let budget_channel = scheduler.budget_channel().clone();
        dcc.connect_budget_channel(budget_channel);

        let _ = dcc
            .event_sender()
            .send(khora_core::telemetry::TelemetryEvent::PhaseChange(
                "boot".to_string(),
            ));

        // Store everything
        self.app = Some(app);
        self.game_world = Some(game_world);
        self.telemetry = Some(telemetry);
        self.dcc = Some(dcc);
        self.scheduler = Some(scheduler);
        self.runtime = runtime_arc;
    }
}
