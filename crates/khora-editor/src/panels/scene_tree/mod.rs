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

//! The Scene Tree panel: the entity hierarchy, searchable, its rows foldable,
//! renamed in place and reparented by drag and drop.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

mod drop;
mod panel;
mod rows;

const ROW_HEIGHT: f32 = 26.0;

const HEADER_HEIGHT: f32 = 34.0;

const TOOLBAR_HEIGHT: f32 = 32.0;

const ROW_PAD_X: f32 = 8.0;

/// The Scene Tree: the hierarchy of the scene's entities.
pub struct SceneTreePanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
    /// How far the entity list is scrolled.
    scroll: khora_tool_ui::widgets::ScrollState,
    /// Entities whose subtree is folded away.
    ///
    /// The folded ones are stored, not the open ones, so a loaded scene and a
    /// spawned child show without opening anything.
    collapsed: std::collections::HashSet<khora_sdk::prelude::ecs::EntityId>,
    /// Whether the rename field took focus for the current rename: asking
    /// every frame would trap it.
    rename_focused: bool,
}

impl SceneTreePanel {
    /// Returns the Scene Tree over the shared editor state.
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
    /// Folds or unfolds this node's subtree.
    ToggleCollapse(khora_sdk::prelude::ecs::EntityId),
    Rename(khora_sdk::prelude::ecs::EntityId),
    Duplicate(khora_sdk::prelude::ecs::EntityId),
    Delete(khora_sdk::prelude::ecs::EntityId),
    Spawn(String),
    /// Reparents `child` under `new_parent`, or to the root with `None`.
    Reparent {
        child: khora_sdk::prelude::ecs::EntityId,
        new_parent: Option<khora_sdk::prelude::ecs::EntityId>,
    },
    /// Saves the entity and everything under it as a `.kprefab`, at a path
    /// the user picks.
    SaveAsPrefab(khora_sdk::prelude::ecs::EntityId),
    /// Saves the entity's inline material as a `.kmat` named by the string,
    /// and makes the entity reference it; nothing without an inline material.
    SaveAsMaterial(khora_sdk::prelude::ecs::EntityId, String),
    /// Drops the asset at `asset_entries[idx]` on the hierarchy, onto the
    /// `target` row if any.
    ///
    /// A mesh, prefab or scene is placed; a texture or material is assigned.
    DropAsset {
        idx: usize,
        target: Option<khora_sdk::prelude::ecs::EntityId>,
    },
}

#[cfg(test)]
mod tests;
