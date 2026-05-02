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
mod traits;
mod vessel;
pub mod winit_adapters;

pub use engine::EngineCore;
pub use game_world::GameWorld;
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
    pub use khora_core::ui::editor::*;
}

// ─────────────────────────────────────────────────────────────────────
// Re-exports from internal crates — the SDK is the single entry point
// ─────────────────────────────────────────────────────────────────────

// Control / DCC
pub use khora_control::{DccConfig, DccService, EngineMode, Context as EngineContext};
pub use khora_control::registry::AgentRegistry;
pub use khora_control::scheduler::ExecutionScheduler;

// Core types
pub use khora_core::agent::ExecutionPhase;
pub use khora_core::control::gorna::{AgentId, StrategyId};
pub use khora_core::ServiceRegistry;
pub use khora_core::telemetry::{TelemetryEvent, MonitoredResourceType};
pub use khora_core::ui::editor::{
    EditorCamera, EditorMode, EditorShell, EditorState, EditorTheme, PlayMode, PanelLocation,
    GizmoMode, EditorLogCapture, LogEntry,
    CommandHistory, EntityIcon, AssetEntry, LogLevel,
    EditorPanel, UiBuilder, SceneNode,
    PropertyEdit, InspectedEntity, EditorCommand,
    CameraSnapshot, LightSnapshot, TransformSnapshot, RigidBodySnapshot,
    ColliderSnapshot, AudioSourceSnapshot, StatusBarData,
};
pub use khora_core::ui::editor::viewport_texture::ViewportTextureHandle;
pub use khora_core::ui::editor::gizmo::GizmoKind;
pub use khora_core::ui::editor::gizmo::GizmoLineInstance;
pub use khora_core::ui::editor::generate_selection_gizmos;

// Telemetry service
pub use khora_telemetry::TelemetryService;

// Infra / monitors
pub use khora_infra::GpuMonitor;
pub use khora_infra::telemetry::memory_monitor::MemoryMonitor;

// I/O
pub use khora_io::serialization::SerializationService;
pub use khora_io::asset::{AssetIo, FileLoader};
pub use khora_core::asset::AssetSource;
pub use khora_core::scene::{SceneFile, SerializationGoal};

// Mesh type (used by editor ops)
pub use khora_core::renderer::api::scene::mesh::Mesh;

// Renderer sub-modules (used by editor gizmo)
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
        pub use khora_core::math::*;
        pub use khora_core::math::LinearRgba;
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
