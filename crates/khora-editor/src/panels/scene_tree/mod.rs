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

//! Scene Tree panel — the entity hierarchy: search, foldable rows with a
//! chevron and a type icon, and the branded gold selection bar.
//!
//! `EditorState::hidden_entities` still dims the rows it names, but nothing
//! populates it: the visibility eye was removed because it only greyed the row
//! while the object kept rendering. The set is left in place as the seam a
//! component-activation model would plug into.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

mod drop;
mod panel;
mod rows;

pub(crate) use drop::{payload_is_entity, unpack_entity};

const ROW_HEIGHT: f32 = 26.0;

const HEADER_HEIGHT: f32 = 34.0;

const TOOLBAR_HEIGHT: f32 = 32.0;

const ROW_PAD_X: f32 = 8.0;

pub struct SceneTreePanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
    /// How far the entity list is scrolled.
    scroll: khora_tool_ui::widgets::ScrollState,
    /// Entities whose subtree is folded away.
    ///
    /// Stored as the *exception* — collapsed rather than expanded — so a
    /// freshly loaded scene shows its whole hierarchy, and a newly spawned
    /// child appears without the user having to open anything.
    collapsed: std::collections::HashSet<khora_sdk::prelude::ecs::EntityId>,
    /// Whether the rename field already took focus for the current rename, so
    /// it is requested once rather than every frame (which would trap it).
    rename_focused: bool,
}

impl SceneTreePanel {
    pub fn new(state: Arc<Mutex<EditorState>>, theme: UiTheme) -> Self {
        Self {
            state,
            theme,
            scroll: khora_tool_ui::widgets::ScrollState::default(),
            collapsed: std::collections::HashSet::new(),
            rename_focused: false,
        }
    }
}

enum EditorAction {
    Select(khora_sdk::prelude::ecs::EntityId),
    /// Fold or unfold this node's subtree.
    ToggleCollapse(khora_sdk::prelude::ecs::EntityId),
    Rename(khora_sdk::prelude::ecs::EntityId),
    Duplicate(khora_sdk::prelude::ecs::EntityId),
    Delete(khora_sdk::prelude::ecs::EntityId),
    Spawn(String),
    /// Reparent `child` under `new_parent`, or detach it (root) when
    /// `new_parent` is `None`. Emitted by the scene tree drag-and-drop
    /// handler.
    Reparent {
        child: khora_sdk::prelude::ecs::EntityId,
        new_parent: Option<khora_sdk::prelude::ecs::EntityId>,
    },
    /// Save the entity's subtree (root + descendants) as a `.kprefab`
    /// asset. The path is picked through `rfd::FileDialog` in the
    /// command dispatcher.
    SaveAsPrefab(khora_sdk::prelude::ecs::EntityId),
    /// Save the entity's inline material to a `.kmat` asset and convert
    /// the entity to reference it. Carries the entity plus the chosen
    /// material name (file stem). No-op in the dispatcher when the entity
    /// has no inline material.
    SaveAsMaterial(khora_sdk::prelude::ecs::EntityId, String),
    /// An asset tile dropped onto the hierarchy: `idx` is the index into
    /// `EditorState::asset_entries`, `target` the entity row it landed on (if
    /// any). Dispatched by asset type — mesh/prefab/scene instantiate into the
    /// scene, texture/material assign to `target`.
    DropAsset {
        idx: usize,
        target: Option<khora_sdk::prelude::ecs::EntityId>,
    },
}
