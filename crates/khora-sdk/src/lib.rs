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
pub mod scripts;
mod traits;
mod vessel;
pub mod winit_adapters;

pub use engine::EngineCore;
pub use game_world::GameWorld;
pub use run_default::run_default;
pub use traits::{AgentProvider, EngineApp, PhaseProvider, WindowProvider};
pub use vessel::{spawn_cube_at, spawn_plane, spawn_sphere, Vessel};
pub use winit_adapters::{run_winit, WinitAppRunner};

// Re-export window provider for convenience
pub use winit_adapters::WinitWindowProvider;

// ─────────────────────────────────────────────────────────────────────
// Editor UI re-exports — so editor panels can import from SDK only
// ─────────────────────────────────────────────────────────────────────
pub mod editor_ui {
    //! Editor UI types re-exported from khora_core.
    //!
    //! Includes everything from `khora_core::ui::editor::*` plus the
    //! shared `UiTheme` and font types that live one level up in
    //! `khora_core::ui` (because the hub uses them too).
    pub use khora_core::ui::editor::*;
    pub use khora_core::ui::fonts::{FontHandle, FontPack, NamedFont};
    pub use khora_core::ui::theme::UiTheme;
}

pub mod tool_ui {
    //! UI surface for standalone Khora tools (the hub, future asset
    //! cookers, …).
    //!
    //! These tools depend on `khora-sdk` and reach the egui backend
    //! exclusively through this module — never directly via `egui`
    //! or `eframe`. The day the engine swaps backend, this re-export
    //! list moves to whichever crate provides the new
    //! [`run_native`] + [`AppContext`] implementation.
    //!
    //! This module is the **runtime** seam only. Khora's *look* — the brand
    //! palette and the shared widget vocabulary — is cosmetics and lives in
    //! the separate `khora-tool-ui` crate, which the SDK deliberately does
    //! **not** depend on, so a game built on Khora never compiles the engine
    //! vendor's brand. Tools depend on both.

    pub use khora_core::math::{LinearRgba, Rect2D, Vec2};
    pub use khora_core::ui::editor::{
        FontFamilyHint, Icon, InlineEditEvent, Interaction, TextAlign,
    };
    pub use khora_core::ui::{
        Align, Align2, App, AppContext, AppLifecycle, CornerRadius, FontHandle, FontPack, Margin,
        NamedFont, Stroke, UiBuilder, UiTheme,
    };
    pub use khora_infra::ui::egui::app::{run_native, WindowConfigInput, WindowIconInput};
}

// ─────────────────────────────────────────────────────────────────────
// Re-exports from internal crates — the SDK is the single entry point
// ─────────────────────────────────────────────────────────────────────

// Control / DCC
pub use khora_control::{Context as EngineContext, DccConfig, DccService, EngineMode};
// Re-export the same Context as `DccContext` so editor code can use the
// more descriptive name without a separate `use` line. (Same type — both
// re-exports point at `khora_control::Context`.)
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
// AgentRegistry is already re-exported above (line 51) via
// `pub use khora_control::registry::AgentRegistry`.

// Infra / monitors
pub use khora_infra::telemetry::memory_monitor::MemoryMonitor;
pub use khora_infra::GpuMonitor;

// I/O
pub use khora_core::asset::AssetSource;
pub use khora_core::scene::{SceneFile, SerializationGoal};
pub use khora_data::assets::SoundData;
pub use khora_io;
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
pub mod renderer {
    pub use khora_core::renderer::api::resource;
    pub use khora_core::renderer::api::scene;
    pub use khora_core::renderer::light;
}

// WgpuRenderSystem (used by editor main)
pub use khora_infra::WgpuRenderSystem;

// Backend implementations and their traits — apps insert these into
// `Runtime::backends` / `Runtime::resources` during the `run_winit`
// bootstrap closure to wire the engine to its physics, layout, text and
// audio backends.
pub use khora_core::audio::{AudioDevice, AudioMixBus, AudioStream, DefaultMixBus, StreamInfo};
pub use khora_core::physics::PhysicsProvider;
pub use khora_core::renderer::api::text::TextRenderer;
pub use khora_core::renderer::traits::PipelineSystem;
pub use khora_core::ui::LayoutSystem;
pub use khora_infra::audio::cpal::CpalAudioDevice;
pub use khora_infra::graphics::WgpuPipelineSystem;
pub use khora_infra::physics::rapier::RapierPhysicsWorld;
pub use khora_infra::renderer::StandardTextRenderer;
pub use khora_infra::ui::TaffyLayoutSystem;
pub use khora_lanes::render_lane::shaders::TEXT_WGSL;

// Data / ECS (needed for world restore)
pub use khora_data;
pub use khora_data::ecs::World as EcsWorld;

// Re-export types used by editor panels and gizmo code
pub use khora_core;
pub use khora_core::math::Mat4;
pub use khora_core::renderer::traits::RenderSystem;
pub use khora_data::ecs::HandleComponent;

// PropertyEdit is in khora_core::ui::editor, already re-exported via editor_ui
pub use khora_data::scene::ComponentRegistration;
pub use khora_data::scene::{instantiate_subtree, serialize_subtree};

// Agents (for when apps need to create their own)
pub use khora_agents;

// Lanes — re-exported so the editor can reach built-in shaders without
// taking a direct dependency on khora-lanes.
pub use khora_lanes;

// Winit — re-exported so the editor can downcast the opaque `&dyn Any`
// `event_loop` argument passed to the `run_winit` bootstrap closure.
pub use winit;

// Re-export inventory for editor
pub extern crate inventory;

pub mod prelude {
    //! Common imports for game development.
    //!
    //! Glob-import this module to bring the everyday game-dev types into scope:
    //! input ([`InputEvent`], [`KeyCode`], [`MouseButton`]), timing
    //! ([`Time`], [`SharedTime`]), assets ([`AssetHandle`], [`AssetUUID`]), and
    //! the [`ecs`], [`materials`], and [`math`] sub-modules. The window config
    //! types [`WindowConfig`] and [`WindowIcon`] come along too.
    //!
    //! # Examples
    //!
    //! ```rust
    //! use khora_sdk::prelude::*;
    //! use khora_sdk::prelude::math::Vec3;
    //!
    //! // ECS components, math, and materials are all reachable through the prelude.
    //! let _transform = ecs::Transform::from_translation(Vec3::new(0.0, 1.0, 0.0));
    //! let _material = materials::StandardMaterial::default();
    //! let _red = math::LinearRgba::RED;
    //! ```

    // SDK types
    pub use crate::{WindowConfig, WindowIcon, PRIMARY_VIEWPORT};

    // Assets
    pub use khora_core::asset::{AssetHandle, AssetUUID};

    // Memory tracking (for `#[global_allocator]`)
    pub use khora_core::memory::SaaTrackingAllocator;

    // Input
    pub use khora_core::platform::{InputEvent, KeyCode, MouseButton};

    // Per-frame timing — real frame delta, fixed sim step, interpolation alpha.
    // `SharedTime` is the interior-mutable handle to cache in `setup` and read
    // each frame in `update`.
    pub use khora_core::time::{SharedTime, Time};

    // ECS types
    pub mod ecs {
        //! Core ECS types for game logic.
        pub use khora_core::ecs::entity::EntityId;
        pub use khora_core::physics::{BodyType, ColliderShape};
        pub use khora_core::renderer::light::{DirectionalLight, LightType, PointLight, SpotLight};
        pub use khora_data::ecs::{
            AudioSource, Camera, Children, Collider, Component, ComponentBundle, GlobalTransform,
            Light, MaterialRef, MeshRef, Name, Parent, ProceduralMeshKind, ProjectionType,
            RigidBody, Script, Tag, Transform, Without,
        };
        // `Script` is how an entity gets gameplay logic. It was missing from
        // this list, which meant no game could attach a behaviour through the
        // SDK at all — the only road in was the editor's generic
        // "+ Add Component" card.
        pub use khora_core::script::ScriptValue;
    }

    // Materials
    pub mod materials {
        //! Built-in material types.
        //!
        //! [`AlphaMode`] is re-exported alongside them because it is the type of
        //! `StandardMaterial::alpha_mode`: without it a game could not select
        //! masked or blended transparency through the SDK.
        pub use khora_core::asset::{
            AlphaMode, EmissiveMaterial, StandardMaterial, UnlitMaterial, WireframeMaterial,
        };
    }

    // Math
    pub mod math {
        //! Math types and utilities.
        pub use khora_core::math::LinearRgba;
        pub use khora_core::math::*;
    }
}

// Re-export InputEvent at crate level for trait usage
pub use khora_core::platform::{InputEvent, KeyCode, MouseButton};

/// Well-known viewport handle for the primary 3D viewport.
pub const PRIMARY_VIEWPORT: ViewportTextureHandle = ViewportTextureHandle(0);

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
