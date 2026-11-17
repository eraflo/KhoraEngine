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

//! Editor state shared between the editor app and its UI panels.
//!
//! The app refreshes [`EditorState`] every frame with a snapshot of the ECS
//! world; panels read it through a shared `Arc<Mutex<EditorState>>`.

use super::EditorRequest;
use crate::asset::{AssetType, AssetUUID};
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
/// Extracted from the ECS world each frame; it borrows no ECS data.
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
    /// Number of tags carried by the entity (`Tag` component).
    ///
    /// The scene tree paints a tag glyph next to the row when it is non-zero.
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

/// Shared editor state, refreshed by the editor app each frame.
///
/// Panels read it to display the scene tree, selection and inspector; it
/// owns all its data and borrows nothing from the ECS.
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
    /// What the panels asked of the app, oldest first.
    ///
    /// Each consumer takes its own kinds with [`take_requests`](Self::take_requests).
    pub requests: Vec<EditorRequest>,
    /// Entity currently being renamed (inline text editing).
    pub renaming_entity: Option<EntityId>,
    /// Buffer for the rename text input.
    pub rename_buffer: String,

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
    /// The `Problems` generation `problems` was copied at.
    ///
    /// A frame where it has not moved copies nothing.
    pub problems_generation: u64,
    /// The last load's problems as they were handed over.
    ///
    /// Kept so a Stop, which respawns every entity, can point them at the restored ones.
    pub load_problems: Vec<crate::problems::Problem>,
    /// The problems the components' own checks find in the current world (the `Data` source).
    ///
    /// What the inspector shows under a field. A load's report says what *was*
    /// loaded, and may be fixed since.
    pub data_problems: Vec<crate::problems::Problem>,
    /// Every name the open project's scripts declare, sorted, as Ergon spells them.
    ///
    /// Components, behaviors and systems, as the engine's `ScriptCatalog` lists
    /// them: the names a new declaration would be refused.
    pub declared_names: Vec<String>,
    /// The `ScriptCatalog` declared generation `declared_names` was copied at.
    ///
    /// A frame where it has not moved copies nothing.
    pub declared_names_generation: u64,
    /// Status bar data (FPS, entity count, memory).
    pub status: StatusBarData,
    /// Asset entries for the asset browser (populated from the VFS).
    pub asset_entries: Vec<AssetEntry>,

    // ── Viewport interaction + Gizmo ───────────────────
    /// Whether the 3D viewport is hovered in the current paint pass.
    ///
    /// Set once the viewport image is laid out. Input handling should test
    /// `viewport_screen_rect` against the live cursor instead.
    pub viewport_hovered: bool,
    /// Screen-space rect of the 3D viewport image this frame, `[x, y, w, h]` in physical pixels.
    ///
    /// Pixels are the platform cursor's unit; the viewport render target is
    /// sized from it, and input tests the live cursor against it. `None`
    /// outside Scene mode or before the viewport is laid out.
    pub viewport_screen_rect: Option<[f32; 4]>,
    /// The active gizmo tool.
    pub gizmo_mode: GizmoMode,
    /// Index of the currently selected asset in the asset browser (if any).
    pub selected_asset: Option<usize>,
    /// Forward-slash path under `<project>/assets/` of the asset selected in the browser.
    ///
    /// When set, the inspector shows the asset's metadata; an entity selection
    /// clears it.
    pub inspected_asset_path: Option<String>,
    /// What the open asset picker filters its entries by, shared by every slot.
    ///
    /// One picker is open at a time.
    pub asset_picker_query: String,
    /// Currently set project folder (used for asset scanning).
    pub project_folder: Option<String>,
    /// Human-readable project name (read from `project.json`).
    pub project_name: Option<String>,
    /// The engine version the project targets, from `project.json::engine_version`.
    ///
    /// Set by the hub at project creation; the status bar and command palette
    /// show it rather than the editor binary's own version.
    pub project_engine_version: Option<String>,
    /// The game's version, read from `project.json::version`.
    ///
    /// `0.1.0` for a project whose file has none.
    pub project_version: Option<String>,
    /// Whether the asset browser should open a folder picker next frame.
    pub pending_browse_project_folder: bool,

    // ── Play Mode ──────────────────────────────────────
    /// Current play mode (Editing / Playing / Paused).
    pub play_mode: PlayMode,
    /// The active camera that renders nothing while playing, and what is wrong with it.
    ///
    /// The viewport says so over the image. `None` when the game's camera
    /// renders or no game runs.
    pub broken_play_camera: Option<(EntityId, String)>,
    /// The scene's snapshot taken at Play, restored at Stop.
    pub scene_snapshot: Option<Vec<u8>>,
    /// The restore Stop filed, being prepared off the frame: Play waits for it.
    pub restoring: Option<crate::scene::LoadTicket>,
    /// The last load's problems as Stop found them, keyed by their entity's persistent id.
    ///
    /// Mapped onto the restored entities.
    pub restore_problems: Vec<(Option<crate::ecs::PersistentId>, crate::problems::Problem)>,
    /// The scenes opening off the frame, in order: ticket and path.
    pub opening: Vec<(crate::scene::LoadTicket, String)>,
    /// Moves at each scene chosen (new, opened): what waited for one before is void.
    pub scene_choices: u64,

    // ── Project trust ──────────────────────────────────
    /// Why the last asked action was refused, for the interface to say so.
    ///
    /// Play or Build Game in a restricted project; `None` when nothing was.
    pub refusal: Option<String>,
    /// The project whose opening waits for the user's trust decision.
    ///
    /// It was never decided on, and nothing of it is opened until it is.
    pub trust_prompt: Option<std::path::PathBuf>,
    /// Whether the open project is restricted.
    ///
    /// The app mirrors the engine's `ProjectTrustState` into it each frame.
    pub restricted: bool,
    /// The open confirmation of trusting the project, with what trusting changes.
    ///
    /// The restricted bar asks for it through [`EditorRequest::ConfirmTrust`];
    /// the app fills it.
    pub trust_confirm: Option<TrustSummary>,
    /// The project's quarantined files, paths relative to the assets root.
    ///
    /// Mirrored from the run's refusals when the problems' generation moves.
    pub quarantined: Vec<QuarantinedAsset>,

    // ── Scene file path ──────────────────────────────
    /// Path to the currently open scene file (for Save).
    pub current_scene_path: Option<String>,

    // ── Component addition ─────────────────────────────
    /// What "+ Add Component" offers the inspected entity.
    ///
    /// Rust and script-declared components it does not hold, each with its
    /// domain and what declares it. Filled each frame by `extract_inspected`,
    /// so the menu never queries the world itself.
    pub addable_components: Vec<super::declared::AddableComponent>,

    // ── Editor workspace ───────────────────────────────
    /// Currently active editing workspace (scene / control plane / …).
    pub active_mode: EditorMode,
    /// Whether the command palette modal is open.
    pub command_palette_open: bool,
    /// Inspector card expand/collapse state, keyed by stable card id.
    ///
    /// The id is typically `"<entity_index>::<title>"`, so cards of two
    /// entities never share a state.
    pub inspector_card_open: std::collections::HashMap<String, bool>,
    /// Current git branch name, read from the project folder's `.git/HEAD`.
    ///
    /// `None` if the project is not a git repository or the file is unreadable.
    pub current_git_branch: Option<String>,
    /// Entities the user hid with the scene tree's eye icon, kept by the UI only.
    pub hidden_entities: HashSet<EntityId>,

    // ── Asset explorer ─────────────────────────────────
    /// Every directory under `assets/` (forward-slash, relative), empty ones
    /// too, refreshed with `asset_entries`.
    pub asset_dirs: Vec<String>,
    /// Bumped whenever `asset_entries`/`asset_dirs` change: the asset browser
    /// rescans its cache then, a file modified in place included.
    pub asset_epoch: u64,
    /// The reindex the hot-reload pump filed, not landed yet.
    ///
    /// The entries, dirs and epoch are refreshed once its outcome is read.
    pub reindexing: Option<crate::asset::IndexTicket>,
}

impl EditorState {
    /// Returns `true` if the entity is currently selected.
    pub fn is_selected(&self, entity: EntityId) -> bool {
        self.selection.contains(&entity)
    }

    /// Selects `entity` alone, clearing the previous selection.
    pub fn select(&mut self, entity: EntityId) {
        self.selection.clear();
        self.selection.insert(entity);
    }

    /// Toggles `entity` in the selection (Ctrl+click).
    pub fn toggle_select(&mut self, entity: EntityId) {
        if !self.selection.remove(&entity) {
            self.selection.insert(entity);
        }
    }

    /// Clears the selection.
    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// Drops every piece of state that names an [`EntityId`].
    ///
    /// Call it whenever the world is replaced wholesale (new scene, scene load,
    /// Stop). Entity slots are recycled, so an old id can name another entity:
    /// a surviving selection would make `Delete` act on it. Card state, keyed
    /// by entity, is cleared too.
    pub fn clear_entity_references(&mut self) {
        self.selection.clear();
        self.inspected = None;
        self.hidden_entities.clear();
        self.renaming_entity = None;
        self.rename_buffer.clear();
        self.inspector_card_open.clear();
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

    /// Queues a property edit to apply back to the ECS world next frame.
    pub fn push_edit(&mut self, edit: PropertyEdit) {
        self.pending_edits.push(edit);
    }

    /// Takes all pending edits, for the app to apply.
    pub fn drain_edits(&mut self) -> Vec<PropertyEdit> {
        std::mem::take(&mut self.pending_edits)
    }

    /// Queues `request` after those already asked.
    pub fn push_request(&mut self, request: EditorRequest) {
        self.requests.push(request);
    }

    /// Takes the requests `take` accepts, oldest first, and leaves the others queued in order.
    ///
    /// `take` returns `Ok` with what it keeps of a request it handles, and
    /// gives any other back as `Err`.
    pub fn take_requests<T>(
        &mut self,
        mut take: impl FnMut(EditorRequest) -> Result<T, EditorRequest>,
    ) -> Vec<T> {
        let mut taken = Vec::new();
        if self.requests.is_empty() {
            return taken;
        }
        let queued = std::mem::take(&mut self.requests);
        for request in queued {
            match take(request) {
                Ok(kept) => taken.push(kept),
                Err(left) => self.requests.push(left),
            }
        }
        taken
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
    /// Formatted once at capture, keeping the console's paint path free of formatting.
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
/// Refreshed each frame by the editor app; a field nothing reports stays at 0.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub struct StatusBarData {
    pub fps: f32,
    pub frame_time_ms: f32,
    pub entity_count: usize,
    pub memory_used_mb: f32,
    /// GPU draw calls last frame.
    ///
    /// 0 until the render family reports them (the frame graph is not timed: issue #292).
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
    /// What kind of asset it is.
    pub asset_type: AssetType,
    /// Source path (for display purposes).
    pub source_path: String,
    /// The asset's identity, as the index resolved it through the project's
    /// identity registry — what a reference to it stores.
    pub uuid: AssetUUID,
    /// The file holding this asset when it is a sub-asset, such as a glTF's mesh.
    ///
    /// Its `name` is then its label inside the file and its `source_path` the
    /// file's. `None` for a file of its own.
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

#[cfg(test)]
mod tests {
    use super::super::{EditorRequest, MenuAction, SpawnShape, Transport};
    use super::EditorState;

    fn menu(request: EditorRequest) -> Result<MenuAction, EditorRequest> {
        match request {
            EditorRequest::Menu(action) => Ok(action),
            other => Err(other),
        }
    }

    /// Two requests asked in one frame are both kept: neither overwrites the other.
    #[test]
    fn two_requests_in_one_frame_are_both_kept() {
        let mut state = EditorState::default();
        state.push_request(EditorRequest::Menu(MenuAction::SpawnEmpty));
        state.push_request(EditorRequest::Menu(MenuAction::Delete));

        assert_eq!(
            state.take_requests(menu),
            [MenuAction::SpawnEmpty, MenuAction::Delete]
        );
        assert!(state.requests.is_empty());
    }

    /// A consumer takes its own kinds, oldest first, and leaves the others in their order.
    #[test]
    fn taking_one_kind_leaves_the_others_in_order() {
        let mut state = EditorState::default();
        state.push_request(EditorRequest::Transport(Transport::Play));
        state.push_request(EditorRequest::Menu(MenuAction::Save));
        state.push_request(EditorRequest::Spawn(SpawnShape::Cube));
        state.push_request(EditorRequest::Menu(MenuAction::About));
        state.push_request(EditorRequest::ToggleWireframe);

        assert_eq!(
            state.take_requests(menu),
            [MenuAction::Save, MenuAction::About]
        );
        assert_eq!(
            state.requests,
            [
                EditorRequest::Transport(Transport::Play),
                EditorRequest::Spawn(SpawnShape::Cube),
                EditorRequest::ToggleWireframe,
            ]
        );
    }

    /// Nothing queued, nothing taken.
    #[test]
    fn an_empty_queue_gives_nothing() {
        let mut state = EditorState::default();
        assert!(state.take_requests(menu).is_empty());
        assert!(state.requests.is_empty());
    }
}
