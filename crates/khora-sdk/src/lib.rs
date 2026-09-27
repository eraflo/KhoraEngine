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

//! The public-facing Software Development Kit (SDK) for the Khora Engine.
//!
//! This is the **only** crate that should be used by game developers.
//! All internal crates (khora-agents, khora-control, etc.) are implementation details.
//!
//! # Examples
//!
//! A minimal game is an [`EngineApp`] handed to [`run_winit`]. The SDK owns the
//! engine loop; your type owns the game logic. Everything you need for game code
//! is in [`prelude`].
//!
//! ```rust,no_run
//! use khora_sdk::prelude::*;
//! use khora_sdk::prelude::math::Vec3;
//! use khora_sdk::{
//!     run_winit, AgentProvider, DccService, EngineApp, GameWorld, PhaseProvider,
//!     Runtime, Vessel, WindowConfig,
//! };
//! use khora_sdk::winit_adapters::WinitWindowProvider;
//!
//! struct MyGame;
//!
//! impl EngineApp for MyGame {
//!     fn window_config() -> WindowConfig {
//!         WindowConfig { title: "My Game".into(), ..WindowConfig::default() }
//!     }
//!     fn new() -> Self {
//!         MyGame
//!     }
//!     fn setup(&mut self, world: &mut GameWorld, _runtime: &Runtime) {
//!         // Spawn a camera looking down the -Z axis.
//!         let camera = ecs::Camera::new_perspective(
//!             std::f32::consts::FRAC_PI_4,
//!             16.0 / 9.0,
//!             0.1,
//!             1000.0,
//!         );
//!         Vessel::at(world, Vec3::new(0.0, 2.0, 10.0))
//!             .with_component(camera)
//!             .build();
//!     }
//!     fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
//! }
//!
//! // `AgentProvider` / `PhaseProvider` are required super-traits; the default
//! // (no custom agents, no custom phases) is enough for most games.
//! impl AgentProvider for MyGame {
//!     fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
//! }
//! impl PhaseProvider for MyGame {}
//!
//! fn main() -> anyhow::Result<()> {
//!     run_winit::<WinitWindowProvider, MyGame>(|_window, _runtime, _event_loop| {
//!         // Wire backends (renderer, physics, audio, …) into `_runtime` here.
//!     })
//! }
//! ```

#![warn(missing_docs)]

mod engine;
mod game_world;
mod run_default;
mod runtime_config;
pub mod scripts;
mod traits;
mod vessel;
pub mod winit_adapters;

pub use engine::EngineCore;
pub use game_world::GameWorld;
pub use run_default::run_default;
pub use runtime_config::{RuntimeConfig, DEFAULT_SCENE_REL_PATH, RUNTIME_CONFIG_FILE};
pub use traits::{AgentProvider, EngineApp, PhaseProvider, WindowProvider};
pub use vessel::{spawn_cube_at, spawn_plane, spawn_sphere, Vessel};
pub use winit_adapters::{run_winit, WinitAppRunner};

// Re-export window provider for convenience
pub use winit_adapters::WinitWindowProvider;

// ─────────────────────────────────────────────────────────────────────
// Editor UI re-exports — so editor panels can import from SDK only
// ─────────────────────────────────────────────────────────────────────
pub mod editor_ui;

pub mod tool_ui;

// ─────────────────────────────────────────────────────────────────────
// Re-exports from internal crates — the SDK is the single entry point
// ─────────────────────────────────────────────────────────────────────

// Control / DCC
pub use khora_control::{DccConfig, DccService, EngineMode};
// The situational `khora_control::Context`, as `DccContext` for the editor.
//
// `AgentRegistry` is exposed as a read-only telemetry surface for the
// editor's Control Plane panel. The mutating side (`ExecutionScheduler`,
// `BudgetChannel`, `EnginePlugin`) stays internal — the SDK is a façade
// for game code, not for engine internals.
pub use khora_control::registry::AgentRegistry;
pub use khora_control::Context as DccContext;

// Core types
pub use khora_core::agent::{AgentImportance, ExecutionPhase, ExecutionTiming};
pub use khora_core::control::gorna::{AgentHints, AgentId, AgentStatus, EngineHint, StrategyId};
pub use khora_core::telemetry::{MonitoredResourceType, TelemetryEvent};
pub use khora_core::ui::editor::generate_selection_gizmos;
pub use khora_core::ui::editor::gizmo::GizmoKind;
pub use khora_core::ui::editor::gizmo::GizmoLineInstance;
pub use khora_core::ui::editor::viewport_texture::ViewportTextureHandle;
pub use khora_core::ui::editor::{
    AssetEntry, CommandHistory, ComponentJson, EditorCamera, EditorCommand, EditorLogCapture,
    EditorMode, EditorPanel, EditorShell, EditorState, EntityIcon, FontFamilyHint, GizmoMode, Icon,
    InspectedEntity, Interaction, LogEntry, LogLevel, PanelLocation, PlayMode, PropertyEdit,
    SceneNode, StatusBarData, TextAlign, UiBuilder,
};
pub use khora_core::ui::fonts::{FontHandle, FontPack, NamedFont};
pub use khora_core::ui::theme::UiTheme;
pub use khora_core::{Backends, Resources, Runtime, Services};

// Telemetry service
pub use khora_telemetry::MonitorRegistry;
pub use khora_telemetry::TelemetryService;
// AgentRegistry is already re-exported above, via
// `pub use khora_control::registry::AgentRegistry`.

// Infra / monitors
pub use khora_infra::telemetry::memory_monitor::MemoryMonitor;
pub use khora_infra::GpuMonitor;

// I/O
pub use khora_core::asset::AssetSource;
pub use khora_core::scene::{SceneFile, SerializationGoal};
pub use khora_data::assets::SoundData;
pub use khora_io::asset::decoders::audio::SymphoniaDecoder;
pub use khora_io::asset::{
    AssetChangeEvent, AssetChangeKind, AssetIdRegistry, AssetIo, AssetService, AssetWatcher,
    AssetWriter, FileLoader, FileSystemResolver, IndexBuilder, MeshDispatcher, PackBuilder,
    PackHeader, PackLoader, PackOutput, PackProgress, PACK_FORMAT_VERSION, PACK_HEADER_SIZE,
    PACK_MAGIC,
};
pub use khora_io::serialization::SerializationService;
pub use khora_telemetry::MetricsRegistry;

// Mesh type (used by editor ops)
pub use khora_core::renderer::api::scene::mesh::Mesh;

// Scene environment — selects the equirectangular map the IBL bake projects
// onto the environment cube (absent ⇒ the procedural sky is baked instead).
pub use khora_data::EnvironmentMap;

/// Renderer sub-modules (used by editor gizmo)
pub mod renderer;

// WgpuRenderSystem (used by editor main)
pub use khora_infra::WgpuRenderSystem;

// Backend implementations and their traits — apps insert these into
// `Runtime::backends` / `Runtime::resources` during the `run_winit`
// bootstrap closure to wire the engine to its physics, layout, text and
// audio backends.
pub use khora_core::audio::{AudioDevice, AudioMixBus, AudioStream, StreamInfo};
pub use khora_core::physics::PhysicsProvider;
pub use khora_core::renderer::api::text::TextRenderer;
pub use khora_core::renderer::traits::PipelineSystem;
pub use khora_core::ui::LayoutSystem;
pub use khora_infra::audio::cpal::CpalAudioDevice;
pub use khora_infra::graphics::WgpuPipelineSystem;
pub use khora_infra::graphics::{EGUI_WGSL, TEXT_WGSL};
pub use khora_infra::physics::rapier::RapierPhysicsWorld;
pub use khora_infra::ui::TaffyLayoutSystem;
pub use khora_infra::DefaultMixBus;
pub use khora_infra::StandardTextRenderer;

// Data / ECS (needed for world restore)
pub use khora_data;

// Re-export types used by editor panels and gizmo code
pub use khora_core;
pub use khora_core::math::Mat4;
pub use khora_core::renderer::traits::RenderSystem;
pub use khora_data::ecs::HandleComponent;

// PropertyEdit is in khora_core::ui::editor, already re-exported via editor_ui
pub use khora_data::scene::ComponentRegistration;
pub use khora_data::scene::{instantiate_subtree, serialize_subtree};

// Lanes — re-exported so the editor can reach built-in shaders without
// taking a direct dependency on khora-lanes.
pub use khora_lanes;

// Winit — re-exported so the editor can downcast the opaque `&dyn Any`
// `event_loop` argument passed to the `run_winit` bootstrap closure.
pub use winit;

// Re-export inventory for editor
pub extern crate inventory;

pub mod prelude;

// Re-export InputEvent at crate level for trait usage
pub use khora_core::platform::{InputEvent, KeyCode, MouseButton};

pub use engine::PRIMARY_VIEWPORT;

/// Raw window icon data for native window creation.
#[derive(Clone, Debug)]
pub struct WindowIcon {
    /// RGBA8 pixel buffer stored row-major.
    pub rgba: Vec<u8>,
    /// Icon width in pixels.
    pub width: u32,
    /// Icon height in pixels.
    pub height: u32,
}

/// Window configuration for applications.
#[derive(Clone, Debug)]
pub struct WindowConfig {
    /// Window title shown by the platform window manager.
    pub title: String,
    /// Initial window width in pixels.
    pub width: u32,
    /// Initial window height in pixels.
    pub height: u32,
    /// Optional custom window icon.
    pub icon: Option<WindowIcon>,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "Khora Engine".to_owned(),
            width: 1024,
            height: 768,
            icon: None,
        }
    }
}
