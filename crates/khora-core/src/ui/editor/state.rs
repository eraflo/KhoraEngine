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

/// The play-mode state of the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayMode {
    /// Normal editing mode — scene is static.
    #[default]
    Editing,
    /// Game simulation is running.
    Playing,
    /// Game simulation is paused (can resume or stop).
    Paused,
}

/// The active editing workspace, switched via the left "spine" mode bar.
///
/// Most modes are placeholders for now — the working ones in Phase 2 are
/// `Scene` (the default 3D dock) and `ControlPlane` (the DCC / agents
/// inspector, when implemented).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorMode {
    /// Default 3D viewport workspace.
    #[default]
    Scene,
    /// 2D canvas / UI authoring (placeholder).
    Canvas2D,
    /// Visual node-graph editor (placeholder).
    NodeGraph,
    /// Animation / timeline editor (placeholder).
    Animation,
    /// Shader graph editor (placeholder).
    Shader,
    /// Dynamic Context Core / agents control workspace.
    ControlPlane,
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
    /// Pending reparent to apply: (child, new_parent_or_None_for_root).
    /// Set by the scene tree drag-and-drop handler, drained by
    /// `process_reparents` in the editor's tick.
    pub pending_reparent: Option<(EntityId, Option<EntityId>)>,

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
    /// Updated each frame from `is_last_item_hovered()` after the viewport
    /// image is laid out — only meaningful for the current paint pass; UI
    /// code that reads input handlers should prefer `viewport_screen_rect`
    /// + the live cursor position.
    pub viewport_hovered: bool,
    /// Screen-space rect of the 3D viewport image (`[x, y, w, h]` in pixels)
    /// for the current frame. `None` when the editor isn't in Scene mode or
    /// the viewport hasn't been laid out yet. Used by the input pipeline to
    /// decide if a mouse event lives over the viewport — checked against
    /// the live cursor position so it doesn't suffer from the
    /// frame-of-latency `viewport_hovered` had.
    pub viewport_screen_rect: Option<[f32; 4]>,
    /// The active gizmo tool.
    pub gizmo_mode: GizmoMode,
    /// Index of the currently selected asset in the asset browser (if any).
    pub selected_asset: Option<usize>,
    /// Pending menu action (e.g. "new_scene", "save", "quit").
    pub pending_menu_action: Option<String>,
    /// Currently set project folder (used for asset scanning).
    pub project_folder: Option<String>,
    /// Human-readable project name (read from `project.json`).
    pub project_name: Option<String>,
    /// Engine version string read from `project.json::engine_version` (set
    /// by the hub at project creation time). The status bar and command
    /// palette display this so users see the engine they targeted, not the
    /// editor binary's own crate version.
    pub project_engine_version: Option<String>,
    /// Whether the asset browser should open a folder picker next frame.
    pub pending_browse_project_folder: bool,

    // ── Phase 7: Play Mode ─────────────────────────────
    /// Current play mode (Editing / Playing / Paused).
    pub play_mode: PlayMode,
    /// Serialised snapshot of the scene taken when entering play mode.
    /// Restored when the user presses Stop.
    pub scene_snapshot: Option<Vec<u8>>,

    // ── Scene file path ──────────────────────────────
    /// Path to the currently open scene file (for Save).
    pub current_scene_path: Option<String>,
    /// Pending scene load path (set by asset browser double-click, consumed by update).
    pub pending_scene_load: Option<String>,

    // ── Component addition ─────────────────────────────
    /// Pending component addition (set by properties panel or scene tree context menu, consumed by update).
    /// The String is the component type name (e.g., "Camera", "RigidBody").
    pub pending_add_component: Option<(EntityId, String)>,

    /// Registry mapping `type_name` → domain tag for every component the
    /// engine currently knows about. Populated by `extract_inspected` once
    /// per frame from the live `World`. Used by the inspector to bucket
    /// the "+ Add Component" menu by domain without re-querying the world.
    pub component_domain_registry: std::collections::HashMap<String, u8>,

    // ── Editor workspace ───────────────────────────────
    /// Currently active editing workspace (scene / control plane / …).
    pub active_mode: EditorMode,
    /// Whether the command palette modal is open.
    pub command_palette_open: bool,
    /// Inspector card expand/collapse state, keyed by stable card id
    /// (typically `"<entity_index>::<title>"` to avoid cross-entity bleed).
    pub inspector_card_open: std::collections::HashMap<String, bool>,
    /// Inspector card on/off toggle state. UI-only for now (no engine wiring).
    pub inspector_card_enabled: std::collections::HashMap<String, bool>,
    /// Current git branch name read from `.git/HEAD` of the project folder.
    /// `None` if the project isn't a git repository or we couldn't read it.
    pub current_git_branch: Option<String>,
    /// Entities the user has hidden via the eye icon in the scene tree.
    /// UI-only state today — see `pending_visibility_toggle` for the engine
    /// hook the editor consumes each frame.
    pub hidden_entities: HashSet<EntityId>,
    /// Pending visibility toggle from the scene tree eye icon. The editor
    /// pops this each frame and applies it to the engine (when an engine
    /// `Visible` component lands; for now it just flips `hidden_entities`).
    pub pending_visibility_toggle: Option<EntityId>,
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

/// Snapshot of one component on an inspected entity, captured generically
/// as JSON via the macro-generated `to_json` on `ComponentRegistration`.
///
/// This is the path adding a new component costs **zero** editor code: any
/// type that derives `Component` is auto-registered and shows up here.
/// Components with a hard-coded inspector card (Transform, Camera, etc.)
/// are skipped from the generic list to avoid double-rendering.
#[derive(Debug, Clone)]
pub struct ComponentJson {
    /// `ComponentRegistration::type_name` — card title and lookup key.
    pub type_name: String,
    /// Domain bucket, mirrored from `SemanticDomain` (Spatial=0, Render=1,
    /// Audio=2, Physics=3, Ui=4). `None` means the type isn't registered
    /// on a CRPECS page (e.g. types registered via inventory only).
    pub domain: Option<u8>,
    /// Live JSON value. The shape mirrors the component's
    /// `Serializable<Type>` form.
    pub value: serde_json::Value,
}

/// Snapshot of an inspected entity.
///
/// Components are captured generically as JSON via the macro-generated
/// `ComponentRegistration::to_json`. The inspector iterates `components_json`
/// and renders every entry through a single field-typed walker — there is
/// no per-component code path. The entity's `Name` lives in the inspector
/// header rather than as a component card.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub struct InspectedEntity {
    pub entity: EntityId,
    pub name: String,
    /// Every component on this entity, captured as JSON. The inspector
    /// renders this list directly — no per-component hard-coding.
    pub components_json: Vec<ComponentJson>,
}

/// A property edit to apply back to the ECS world.
///
/// Components are mutated through a single generic round-trip via
/// `ComponentRegistration::from_json`: the inspector walks each field of
/// the component's JSON shape, lets the user edit it, and ships the whole
/// patched `serde_json::Value` back through `SetComponentJson`. There is
/// no per-component editor variant — adding a new component costs zero
/// editor code.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub enum PropertyEdit {
    /// Rename the entity (`Name` component is special — it shows up in the
    /// inspector header rather than as a component card).
    SetName(EntityId, String),
    /// Replace the entire JSON value of one component on `entity`. The
    /// editor looks up the registration by `type_name` and calls
    /// `from_json` to commit.
    SetComponentJson {
        entity: EntityId,
        type_name: String,
        value: serde_json::Value,
    },
    /// Remove a component from `entity`. The editor looks up the
    /// registration by `type_name` and calls `remove` to commit.
    RemoveComponent {
        entity: EntityId,
        type_name: String,
    },
}

// ════════════════════════════════════════════════════════
//  Console / Status Bar types
// ════════════════════════════════════════════════════════

/// A captured log entry for the console panel.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub target: String,
}

/// Log severity matching `log::Level`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// Status bar data displayed at the bottom of the editor.
///
/// Populated each frame by `EditorApp::update`; the GPU-related fields are
/// pulled from `TelemetryService` when available, otherwise stay at 0.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub struct StatusBarData {
    pub fps: f32,
    pub frame_time_ms: f32,
    pub entity_count: usize,
    pub memory_used_mb: f32,
    /// GPU draw calls last frame (from `GpuReport`). 0 when telemetry has no
    /// reading yet.
    pub draw_calls: u32,
    /// GPU triangles rendered last frame.
    pub triangles: u64,
    /// Approximate VRAM use in MB. 0 if not reported.
    pub vram_mb: f32,
    /// CPU load (0.0–1.0) as reported by `HardwareReport`.
    pub cpu_load: f32,
    /// GPU load (0.0–1.0) as reported by `HardwareReport`.
    pub gpu_load: f32,
}

impl Default for StatusBarData {
    fn default() -> Self {
        Self {
            fps: 0.0,
            frame_time_ms: 0.0,
            entity_count: 0,
            memory_used_mb: 0.0,
            draw_calls: 0,
            triangles: 0,
            vram_mb: 0.0,
            cpu_load: 0.0,
            gpu_load: 0.0,
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
