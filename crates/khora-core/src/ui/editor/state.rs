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

use crate::asset::AssetUUID;
use crate::ecs::entity::EntityId;
use std::collections::HashSet;

pub use super::inspected::{
    ComponentJson, ComponentOverride, InspectedEntity, InspectedPrefab, PrefabApplyScope,
    PropertyEdit,
};

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
/// Two workspaces ship today: `Scene` (the default 3D dock) and `ControlPlane`
/// (the DCC / agents inspector). Future authoring workspaces (2D canvas, node
/// graph, animation, shader graph) will be added as they are built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EditorMode {
    /// Default 3D viewport workspace.
    #[default]
    Scene,
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
    /// Number of tags carried by the entity (`Tag` component). The scene
    /// tree paints a small tag glyph next to the row when this is non-zero.
    pub tag_count: usize,
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

    // ── Properties Inspector ───────────────────────────
    /// Snapshot of the single-selected entity's components (if any).
    pub inspected: Option<InspectedEntity>,
    /// Pending property edits to apply back to the ECS world.
    pub pending_edits: Vec<PropertyEdit>,

    // ── Console / Status Bar ───────────────────────────
    /// Log entries captured by the editor log sink.
    pub log_entries: Vec<LogEntry>,
    /// What is wrong with the open project's scripts — compile, registration
    /// and running problems — as the engine's `Problems` lists them.
    pub problems: Vec<crate::problems::Problem>,
    /// The `Problems` generation `problems` was copied at: a
    /// frame where it has not moved copies nothing.
    pub problems_generation: u64,
    /// What the scene load just done adapted, as problems, waiting for the
    /// app to report them as the `Load` source; `Some(empty)` clears it (a
    /// new scene).
    pub pending_load_problems: Option<Vec<crate::problems::Problem>>,
    /// The last load's problems as they were handed over — kept so a Stop,
    /// which respawns every entity, can point them at the restored ones.
    pub load_problems: Vec<crate::problems::Problem>,
    /// The problems the components' own checks find in the world as it is
    /// now (the `Data` source): what the inspector shows under a field. A
    /// load's report says what *was* loaded, and may be fixed since.
    pub data_problems: Vec<crate::problems::Problem>,
    /// Every name the open project's scripts declare — components,
    /// behaviors, systems — sorted, exactly as Ergon spells them: the names a
    /// new declaration would be refused, as the engine's `ScriptCatalog`
    /// lists them.
    pub declared_names: Vec<String>,
    /// The `ScriptCatalog` declared generation `declared_names` was copied
    /// at: a frame where it has not moved copies nothing.
    pub declared_names_generation: u64,
    /// Status bar data (FPS, entity count, memory).
    pub status: StatusBarData,
    /// Asset entries for the asset browser (populated from the VFS).
    pub asset_entries: Vec<AssetEntry>,

    // ── Viewport interaction + Gizmo ───────────────────
    /// Whether the 3D viewport is currently hovered (for camera controls).
    /// Updated each frame from `is_last_item_hovered()` after the viewport
    /// image is laid out — only meaningful for the current paint pass; UI
    /// code that reads input handlers should prefer `viewport_screen_rect`
    /// + the live cursor position.
    pub viewport_hovered: bool,
    /// Screen-space rect of the 3D viewport image (`[x, y, w, h]` in physical
    /// pixels, the unit of the platform cursor) for the current frame; the
    /// viewport render target is sized from it. `None` when the editor isn't in Scene mode or
    /// the viewport hasn't been laid out yet. Used by the input pipeline to
    /// decide if a mouse event lives over the viewport — checked against
    /// the live cursor position so it doesn't suffer from the
    /// frame-of-latency `viewport_hovered` had.
    pub viewport_screen_rect: Option<[f32; 4]>,
    /// The active gizmo tool.
    pub gizmo_mode: GizmoMode,
    /// Index of the currently selected asset in the asset browser (if any).
    pub selected_asset: Option<usize>,
    /// Forward-slash relative path (under `<project>/assets/`) of the
    /// asset selected in the browser, or `None`. When set, the
    /// Inspector switches to asset-metadata mode. Cleared
    /// when an entity selection is made.
    pub inspected_asset_path: Option<String>,
    /// The asset an inspector slot asked to reveal, by its forward-slash
    /// path under `assets/`: the asset browser opens its folder, selects its
    /// tile and inspects it, as a click on the tile does.
    pub pending_reveal_asset: Option<String>,
    /// What the open asset picker filters its entries by — one for every
    /// slot, since one picker is open at a time.
    pub asset_picker_query: String,
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
    /// The game's version, read from `project.json::version` — `0.1.0` for
    /// a project written before projects had one.
    pub project_version: Option<String>,
    /// Whether the asset browser should open a folder picker next frame.
    pub pending_browse_project_folder: bool,

    // ── Play Mode ──────────────────────────────────────
    /// Current play mode (Editing / Playing / Paused).
    pub play_mode: PlayMode,
    /// The active camera that renders nothing while playing — its entity and
    /// what is wrong with it — for the viewport to say so over the image;
    /// `None` when the game's camera renders or no game runs.
    pub broken_play_camera: Option<(EntityId, String)>,
    /// The scene's snapshot taken at Play, restored at Stop.
    pub scene_snapshot: Option<Vec<u8>>,
    /// The restore Stop filed, being prepared off the frame: Play waits for it.
    pub restoring: Option<crate::scene::LoadTicket>,
    /// The last load's problems as Stop found them, by the persistent id of
    /// the entity each names: mapped onto the restored entities.
    pub restore_problems: Vec<(Option<crate::ecs::PersistentId>, crate::problems::Problem)>,
    /// The scenes opening off the frame, in order: ticket and path.
    pub opening: Vec<(crate::scene::LoadTicket, String)>,
    /// Moves at each scene chosen (new, opened): what waited for one before is void.
    pub scene_choices: u64,

    // ── Project trust ──────────────────────────────────
    /// Why the last action asked was refused — Play or Build Game in a
    /// restricted project — for the interface to say so; `None` when nothing
    /// was.
    pub refusal: Option<String>,
    /// The project whose open waits for the user's trust decision: it was
    /// never decided of, and nothing of it is opened until it is.
    pub trust_prompt: Option<std::path::PathBuf>,
    /// Whether the project open is restricted: the app mirrors the engine's
    /// `ProjectTrustState` into it each frame.
    pub restricted: bool,
    /// The confirmation of trusting the project open, while it is open, with
    /// what trusting changes. The restricted bar asks for it through the
    /// `confirm_trust` menu action; the app fills it.
    pub trust_confirm: Option<TrustSummary>,
    /// The files of the project quarantined, mirrored from the run's
    /// refusals when the problems' generation moves, their paths relative to
    /// the assets root.
    pub quarantined: Vec<QuarantinedAsset>,
    /// A quarantined asset to release, by its forward-slash path under
    /// `assets/`. Drained by the app.
    pub pending_release: Option<String>,

    // ── Scene file path ──────────────────────────────
    /// Path to the currently open scene file (for Save).
    pub current_scene_path: Option<String>,
    /// Pending scene load path (set by asset browser double-click, consumed by update).
    pub pending_scene_load: Option<String>,

    // ── Component addition ─────────────────────────────
    /// Pending component addition (set by properties panel or scene tree context menu, consumed by update).
    /// The String is the component type name (e.g., "Camera", "RigidBody").
    pub pending_add_component: Option<(EntityId, String)>,

    /// What "+ Add Component" offers the inspected entity — Rust components
    /// and script-declared ones it does not hold, each with its domain and
    /// what declares it. Filled by `extract_inspected` each frame from the
    /// live `World`, so the menu never queries the world itself.
    pub addable_components: Vec<super::declared::AddableComponent>,

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

    // ── Prefab workflow ────────────────────────────────
    /// Set when the user picks "Save as Prefab" in the scene tree
    /// context menu. The editor consumes this next frame, opens a
    /// file dialog, calls `serialize_subtree`, and writes the resulting
    /// `.kprefab` file.
    pub pending_save_as_prefab: Option<EntityId>,
    /// Same payload as `pending_save_as_prefab`, but with a pre-chosen
    /// destination forward-slash relative path under `<project>/assets/`
    /// — set by drag-and-drop (e.g. dragging an entity onto the asset
    /// browser's current folder). The dispatcher writes directly with
    /// no file dialog and reuses the entity name as the file stem when
    /// the path ends in `/`.
    pub pending_save_as_prefab_at: Option<(EntityId, String)>,
    /// Set when the user drops a prefab onto the viewport / hierarchy (or
    /// activates one from the asset browser): a `.kprefab`, a glTF scene
    /// sub-asset, or a glTF file standing for its default scene. Holds the
    /// prefab's identity plus an optional parent entity (the hierarchy row it
    /// was dropped on) so the instantiated root is parented under it. Consumed
    /// next frame to instantiate the prefab as a linked instance.
    pub pending_prefab_spawn: Option<(AssetUUID, Option<EntityId>)>,
    /// Set when the user applies an override of the inspected entity to its
    /// prefab: the entity and what of it is applied. Consumed next frame:
    /// the prefab's file is rewritten through the VFS and reindexed.
    pub pending_prefab_apply: Option<(EntityId, PrefabApplyScope)>,
    /// An entity whose whole prefab instance is to take its prefab's values
    /// back — every value override of every member, every component added to
    /// or removed from one. Drained by the editor's update.
    pub pending_prefab_revert: Option<EntityId>,

    // ── Material authoring workflow ─────────────────────
    /// Set when the user picks "Save Material as .kmat" on an entity that
    /// carries a `MaterialRef::Inline`. Holds the entity plus the chosen
    /// material name (file stem). The editor consumes this next frame:
    /// serializes the inline material to RON under
    /// `assets/materials/<name>.kmat`, reindexes, and rewrites the entity's
    /// component to `MaterialRef::Asset(uuid)` so it now references the
    /// shared, reloadable asset.
    pub pending_save_as_material: Option<(EntityId, String)>,
    /// Set when the user assigns a `.kmat` from the asset browser to the
    /// current selection. Holds the material's identity — a `.kmat`, or a
    /// material inside another file, which a path cannot name. Consumed next
    /// frame: each selected entity's `MaterialRef` is set to `Asset(uuid)`.
    pub pending_assign_material: Option<AssetUUID>,

    // ── Asset explorer: file operations & scene drop ───
    /// Every directory under `assets/` (forward-slash, relative), empty ones
    /// too, refreshed with `asset_entries`.
    pub asset_dirs: Vec<String>,
    /// Bumped whenever `asset_entries`/`asset_dirs` change: the asset browser
    /// rescans its cache then, a file modified in place included.
    pub asset_epoch: u64,
    /// The reindex the hot-reload pump filed, not landed yet: the entries,
    /// dirs and epoch are refreshed once its outcome is read.
    pub reindexing: Option<crate::asset::IndexTicket>,
    /// Create an empty folder at this forward-slash relative path under
    /// `assets/`. Consumed next frame.
    pub pending_create_folder: Option<String>,
    /// Rename/move an asset: `(old_rel, new_rel)`, both forward-slash under
    /// `assets/`. The identity registry freezes the UUID so references survive.
    pub pending_rename_asset: Option<(String, String)>,
    /// Move an asset into a folder: `(src_rel, dest_dir)` (dest_dir `""` = root).
    pub pending_move_asset: Option<(String, String)>,
    /// Send an asset to the OS recycle bin: forward-slash relative path.
    pub pending_delete_asset: Option<String>,
    /// Duplicate an asset next to itself: forward-slash relative path.
    pub pending_duplicate_asset: Option<String>,
    /// Create an Ergon script: `(rel_path, source)`, forward-slash under
    /// `assets/`. Never overwrites a file. Consumed next frame.
    pub pending_create_script: Option<(String, String)>,
    /// Spawn a mesh asset into the scene: `(mesh identity, [x,y,z] world
    /// point, optional parent entity)` — a mesh file or a mesh sub-asset. Set
    /// by dragging a mesh tile onto the viewport (parent `None`) or a hierarchy
    /// row (parent = that entity); drained into a `MeshRef::Asset` entity
    /// spawn, parented under the target when present.
    pub pending_spawn_mesh_asset: Option<(AssetUUID, [f32; 3], Option<EntityId>)>,
    /// Assign a texture/material asset to a specific entity (drag onto an entity
    /// in the viewport): `(asset identity, entity)`. The asset's index type
    /// decides: a material is referenced, a texture becomes the base colour of
    /// an inline standard material.
    pub pending_assign_texture: Option<(AssetUUID, EntityId)>,
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

    /// Drops every piece of state that names an [`EntityId`], for use whenever
    /// the world is replaced wholesale — new scene, scene load, or `Stop`
    /// restoring the pre-play snapshot.
    ///
    /// Those paths rebuild the world with **fresh** ids, so anything still
    /// holding an old one is not merely stale but actively dangerous: entity
    /// slots are recycled, and an index+generation pair can come back attached
    /// to a different entity. A surviving selection then makes `Delete` act on
    /// something the user never selected.
    ///
    /// Card state is keyed by entity too, so it is cleared here rather than
    /// growing without bound across scene changes.
    pub fn clear_entity_references(&mut self) {
        self.selection.clear();
        self.inspected = None;
        self.hidden_entities.clear();
        self.renaming_entity = None;
        self.rename_buffer.clear();
        self.inspector_card_open.clear();
        self.inspector_card_enabled.clear();
        self.scene_roots.clear();
        self.entity_count = 0;
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
//  Console / Status Bar types
// ════════════════════════════════════════════════════════

/// A captured log entry for the console panel.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub target: String,
    /// Wall-clock time the entry was captured, as `HH:MM:SS`.
    ///
    /// Formatted at capture rather than stored as an instant: the console is
    /// the only consumer, it always renders it, and doing it once at capture
    /// keeps the paint path free of formatting work.
    pub time: String,
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
    /// GPU draw calls last frame. 0 until the render family reports them
    /// (the frame graph is not timed yet: issue #292).
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
    /// The asset's identity, as the index resolved it through the project's
    /// identity registry — what a reference to it stores.
    pub uuid: AssetUUID,
    /// The file holding this asset when it is a sub-asset (a glTF's mesh,
    /// material, texture or scene): its `name` is then its label inside the
    /// file and its `source_path` the file's. `None` for a file of its own.
    pub container: Option<AssetUUID>,
}

// ════════════════════════════════════════════════════════
//  Project trust types
// ════════════════════════════════════════════════════════

/// What trusting the project open changes, for its confirmation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustSummary {
    /// How many scripts it runs: the `.erg` files under `assets/scripts`.
    pub scripts: usize,
    /// The shader modules its overlays replace, by their logical names
    /// (`khora::lighting`): the `.wgsl` files under `assets/shaders`.
    pub overlays: Vec<String>,
    /// Whether Build Game compiles its code: it has a `Cargo.toml`.
    pub builds_code: bool,
    /// How many of its files were refused at the restricted limits for their
    /// size, and are read again once it is trusted.
    pub refused_for_size: usize,
}

/// A quarantined asset, as the asset browser and its details pane show it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuarantinedAsset {
    /// Its forward-slash path under `assets/`.
    pub rel_path: String,
    /// Why it was refused.
    pub reason: String,
    /// What to do about it.
    pub remedy: String,
    /// What refused it: `file guard`, `type check`, `decoder` or `pack`.
    pub refused_by: String,
    /// The trust it was read at: `trusted project`, `restricted project`,
    /// `downloaded file`, `engine` or `save`.
    pub read_as: String,
    /// What it is held by: `blake3 xxxx…xxxx` for its content, `size and
    /// date` for its stamp.
    pub content: String,
}
