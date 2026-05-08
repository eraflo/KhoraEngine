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

#![warn(missing_docs)]

mod engine;
mod game_world;
mod run_default;
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

    pub use khora_core::math::{LinearRgba, Rect2D, Vec2};
    pub use khora_core::ui::editor::{FontFamilyHint, Icon, Interaction, TextAlign};
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
pub use khora_control::registry::AgentRegistry;
pub use khora_control::scheduler::ExecutionScheduler;
pub use khora_control::Context as DccContext;

// Core types
pub use khora_core::agent::{AgentImportance, ExecutionPhase, ExecutionTiming};
pub use khora_core::control::gorna::{AgentId, AgentStatus, StrategyId};
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
pub use khora_core::ServiceRegistry;

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
    AssetChangeEvent, AssetChangeKind, AssetIo, AssetService, AssetWatcher, AssetWriter,
    FileLoader, FileSystemResolver, IndexBuilder, MeshDispatcher, PackBuilder, PackHeader,
    PackLoader, PackOutput, PackProgress, PACK_FORMAT_VERSION, PACK_HEADER_SIZE, PACK_MAGIC,
};
pub use khora_io::serialization::SerializationService;
pub use khora_telemetry::MetricsRegistry;

// Mesh type (used by editor ops)
pub use khora_core::renderer::api::scene::mesh::Mesh;

/// Renderer sub-modules (used by editor gizmo)
pub mod renderer {
    pub use khora_core::renderer::api::resource;
    pub use khora_core::renderer::api::scene;
    pub use khora_core::renderer::light;
}

// WgpuRenderSystem (used by editor main)
pub use khora_infra::WgpuRenderSystem;

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

    // SDK types
    pub use crate::{WindowConfig, WindowIcon, PRIMARY_VIEWPORT};

    // Assets
    pub use khora_core::asset::{AssetHandle, AssetUUID};

    // Memory tracking (for `#[global_allocator]`)
    pub use khora_core::memory::SaaTrackingAllocator;

    // Input
    pub use khora_core::platform::{InputEvent, MouseButton};

    // ECS types
    pub mod ecs {
        //! Core ECS types for game logic.
        pub use khora_core::ecs::entity::EntityId;
        pub use khora_core::physics::{BodyType, ColliderShape};
        pub use khora_core::renderer::light::{DirectionalLight, LightType, PointLight, SpotLight};
        pub use khora_data::ecs::{
            AudioSource, Camera, Children, Collider, Component, ComponentBundle, GlobalTransform,
            Light, MaterialComponent, Name, Parent, ProjectionType, RigidBody, Transform, Without,
        };
    }

    // Materials
    pub mod materials {
        //! Built-in material types.
        pub use khora_core::asset::{
            EmissiveMaterial, StandardMaterial, UnlitMaterial, WireframeMaterial,
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
pub use khora_core::platform::{InputEvent, MouseButton};

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
