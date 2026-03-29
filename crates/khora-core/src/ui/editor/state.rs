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

//! Editor state shared between the application logic and UI panels.
//!
//! The [`EditorState`] is populated by `Application::update()` every frame
//! with a lightweight snapshot of the ECS world. Editor panels read this
//! snapshot through a shared `Arc<Mutex<EditorState>>` retrieved from the
//! `ServiceRegistry`.

use crate::ecs::entity::EntityId;
use crate::math::{LinearRgba, Quaternion, Vec3};
use std::collections::HashSet;

/// The active gizmo tool in the viewport toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GizmoMode {
    /// Selection / pointer tool.
    #[default]
    Select,
    /// Translation gizmo.
    Move,
    /// Rotation gizmo.
    Rotate,
    /// Scale gizmo.
    Scale,
}

/// A lightweight description of an entity in the scene tree.
///
/// This is a UI-oriented DTO extracted from the ECS world each frame.
/// It does not borrow any ECS data and can be freely shared between threads.
#[derive(Debug, Clone)]
pub struct SceneNode {
    /// The entity identifier.
    pub entity: EntityId,
    /// Human-readable name (`Name` component, or fallback like "Entity 42").
    pub name: String,
    /// Visual hint about what kind of entity this is.
    pub icon: EntityIcon,
    /// Direct children in the scene hierarchy (from the `Children` component).
    pub children: Vec<SceneNode>,
}

/// Icon hint for the scene tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityIcon {
    /// Generic / unknown entity.
    Empty,
    /// Entity has a `Camera` component.
    Camera,
    /// Entity has a `Light` component.
    Light,
    /// Entity has a mesh handle.
    Mesh,
    /// Entity has an `AudioSource` component.
    Audio,
}

/// Shared editor state populated by `Application::update()` each frame.
///
/// Panels read this to display the scene tree, selection highlights, etc.
/// All data is owned (snapshot) — no ECS borrows.
#[derive(Debug, Clone, Default)]
pub struct EditorState {
    /// Root-level scene nodes (entities without a `Parent` component).
    pub scene_roots: Vec<SceneNode>,
    /// Currently selected entity IDs.
    pub selection: HashSet<EntityId>,
    /// Total entity count in the world.
    pub entity_count: usize,
    /// Scene tree search / filter text.
    pub search_filter: String,
    /// Whether the Ctrl key is currently held (for multi-select).
    pub ctrl_held: bool,
    /// A pending spawn request tag (e.g. "Empty", "Cube", "Light", "Camera").
    /// The application reads and clears this each frame.
    pub pending_spawn: Option<String>,
    /// Pending delete request for a specific entity (from context menu).
    pub pending_delete: Option<EntityId>,
    /// Pending duplicate request for a specific entity (from context menu).
    pub pending_duplicate: Option<EntityId>,
    /// Entity currently being renamed (inline text editing).
    pub renaming_entity: Option<EntityId>,
    /// Buffer for the rename text input.
    pub rename_buffer: String,
    /// Pending rename to apply: (entity, new_name).
    pub pending_rename: Option<(EntityId, String)>,

    // ── Phase 4: Properties Inspector ──────────────────

    /// Snapshot of the single-selected entity's components (if any).
    pub inspected: Option<InspectedEntity>,
    /// Pending property edits to apply back to the ECS world.
    pub pending_edits: Vec<PropertyEdit>,

    // ── Phase 5: Console / Status Bar ──────────────────

    /// Log entries captured by the editor log sink.
    pub log_entries: Vec<LogEntry>,
    /// Status bar data (FPS, entity count, memory).
    pub status: StatusBarData,
    /// Asset entries for the asset browser (populated from the VFS).
    pub asset_entries: Vec<AssetEntry>,

    // ── Phase 6: Viewport interaction + Gizmo ──────────

    /// Whether the 3D viewport is currently hovered (for camera controls).
    pub viewport_hovered: bool,
    /// The active gizmo tool.
    pub gizmo_mode: GizmoMode,
    /// Index of the currently selected asset in the asset browser (if any).
    pub selected_asset: Option<usize>,
    /// Pending menu action (e.g. "new_scene", "save", "quit").
    pub pending_menu_action: Option<String>,
}

impl EditorState {
    /// Returns `true` if the entity is currently selected.
    pub fn is_selected(&self, entity: EntityId) -> bool {
        self.selection.contains(&entity)
    }

    /// Select a single entity (clears previous selection).
    pub fn select(&mut self, entity: EntityId) {
        self.selection.clear();
        self.selection.insert(entity);
    }

    /// Toggle an entity in the selection (Ctrl+click behavior).
    pub fn toggle_select(&mut self, entity: EntityId) {
        if !self.selection.remove(&entity) {
            self.selection.insert(entity);
        }
    }

    /// Clear the selection entirely.
    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// Returns the single selected entity, if exactly one is selected.
    pub fn single_selected(&self) -> Option<EntityId> {
        if self.selection.len() == 1 {
            self.selection.iter().next().copied()
        } else {
            None
        }
    }

    /// Push a property edit to be applied back to the ECS world next frame.
    pub fn push_edit(&mut self, edit: PropertyEdit) {
        self.pending_edits.push(edit);
    }

    /// Drain all pending edits (called by `Application::update()`).
    pub fn drain_edits(&mut self) -> Vec<PropertyEdit> {
        std::mem::take(&mut self.pending_edits)
    }
}

// ════════════════════════════════════════════════════════
//  Properties Inspector types
// ════════════════════════════════════════════════════════

/// Snapshot of all inspectable component data for one entity.
#[derive(Debug, Clone)]
pub struct InspectedEntity {
    pub entity: EntityId,
    pub name: String,
    pub transform: Option<TransformSnapshot>,
    pub camera: Option<CameraSnapshot>,
    pub light: Option<LightSnapshot>,
    pub rigid_body: Option<RigidBodySnapshot>,
    pub collider: Option<ColliderSnapshot>,
    pub audio_source: Option<AudioSourceSnapshot>,
}

/// Copy of `Transform` fields for inspector display.
#[derive(Debug, Clone, Copy)]
pub struct TransformSnapshot {
    pub translation: Vec3,
    pub rotation: Quaternion,
    pub scale: Vec3,
}

/// Copy of `Camera` fields.
#[derive(Debug, Clone, Copy)]
pub struct CameraSnapshot {
    pub projection_index: usize, // 0 = Perspective, 1 = Orthographic
    pub fov_y_radians: f32,
    pub ortho_width: f32,
    pub ortho_height: f32,
    pub aspect_ratio: f32,
    pub z_near: f32,
    pub z_far: f32,
    pub is_active: bool,
}

/// Copy of `Light` fields, flattened for inspector editing.
#[derive(Debug, Clone, Copy)]
pub struct LightSnapshot {
    pub light_kind: usize, // 0 = Directional, 1 = Point, 2 = Spot
    pub direction: Vec3,
    pub color: LinearRgba,
    pub intensity: f32,
    pub range: f32,
    pub inner_cone_angle: f32,
    pub outer_cone_angle: f32,
    pub shadow_enabled: bool,
    pub shadow_bias: f32,
    pub shadow_normal_bias: f32,
    pub enabled: bool,
}

/// Copy of `RigidBody` fields.
#[derive(Debug, Clone, Copy)]
pub struct RigidBodySnapshot {
    pub body_type_index: usize, // 0 = Dynamic, 1 = Static, 2 = Kinematic
    pub mass: f32,
    pub ccd_enabled: bool,
    pub linear_velocity: Vec3,
    pub angular_velocity: Vec3,
}

/// Copy of `Collider` fields.
#[derive(Debug, Clone, Copy)]
pub struct ColliderSnapshot {
    pub shape_index: usize, // 0 = Box, 1 = Sphere, 2 = Capsule
    pub box_half_extents: Vec3,
    pub sphere_radius: f32,
    pub capsule_radius: f32,
    pub capsule_half_height: f32,
    pub friction: f32,
    pub restitution: f32,
    pub is_sensor: bool,
}

/// Copy of `AudioSource` fields.
#[derive(Debug, Clone, Copy)]
pub struct AudioSourceSnapshot {
    pub volume: f32,
    pub looping: bool,
    pub autoplay: bool,
}

/// A property edit to apply back to the ECS world.
#[derive(Debug, Clone)]
pub enum PropertyEdit {
    SetName(EntityId, String),
    SetTransform(EntityId, TransformSnapshot),
    SetCamera(EntityId, CameraSnapshot),
    SetLight(EntityId, LightSnapshot),
    SetRigidBody(EntityId, RigidBodySnapshot),
    SetCollider(EntityId, ColliderSnapshot),
    SetAudioSource(EntityId, AudioSourceSnapshot),
}

// ════════════════════════════════════════════════════════
//  Console / Status Bar types
// ════════════════════════════════════════════════════════

/// A captured log entry for the console panel.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub target: String,
}

/// Log severity matching `log::Level`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// Status bar data displayed at the bottom of the editor.
#[derive(Debug, Clone)]
pub struct StatusBarData {
    pub fps: f32,
    pub frame_time_ms: f32,
    pub entity_count: usize,
    pub memory_used_mb: f32,
}

impl Default for StatusBarData {
    fn default() -> Self {
        Self {
            fps: 0.0,
            frame_time_ms: 0.0,
            entity_count: 0,
            memory_used_mb: 0.0,
        }
    }
}

// ════════════════════════════════════════════════════════
//  Asset Browser types
// ════════════════════════════════════════════════════════

/// A lightweight description of an asset for the asset browser panel.
#[derive(Debug, Clone)]
pub struct AssetEntry {
    /// Human-readable file/asset name.
    pub name: String,
    /// Asset type string (e.g. "Mesh", "Texture", "Shader", "Audio").
    pub asset_type: String,
    /// Source path (for display purposes).
    pub source_path: String,
}
