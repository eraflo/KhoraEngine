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

//! A command of the editor's menus, command palette or shortcuts.

/// A command of the editor's menus, command palette or shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuAction {
    /// Starts an empty scene.
    NewScene,
    /// Opens a scene file picked through a dialog.
    Open,
    /// Opens a project folder picked through a dialog.
    OpenProject,
    /// Saves the scene to its file, or through a dialog when it has none.
    Save,
    /// Saves the scene to a file picked through a dialog.
    SaveAs,
    /// Quits the editor.
    Quit,
    /// Despawns the selection.
    Delete,
    /// Spawns an empty entity.
    SpawnEmpty,
    /// Builds the project as a game.
    BuildGame,
    /// Opens the documentation in the browser.
    Documentation,
    /// Logs the engine's version.
    About,
}
