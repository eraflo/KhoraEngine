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

//! What the editor's panels ask of the editor app, queued on its state.

use super::{PrefabApplyScope, SpawnShape};
use crate::asset::AssetUUID;
use crate::ecs::entity::EntityId;
use crate::problems::Problem;

/// A request a panel queues on `EditorState::requests` for the app to carry out.
///
/// Each kind has one consumer, which takes its own kinds and leaves the
/// others queued, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum EditorRequest {
    /// A command of the menus, the command palette or a shortcut.
    Menu(MenuAction),
    /// Play, Pause or Stop.
    Transport(Transport),
    /// Shows or hides the scene's meshes as edge lines.
    ToggleWireframe,
    /// The user's answer about trusting a project.
    Trust(TrustAnswer),
    /// Opens the confirmation of trusting the open project.
    ConfirmTrust,
    /// Spawns a new entity.
    Spawn(SpawnShape),
    /// Despawns one entity.
    DeleteEntity {
        /// The entity to despawn.
        entity: EntityId,
    },
    /// Duplicates one entity and its children.
    DuplicateEntity {
        /// The entity to copy.
        entity: EntityId,
    },
    /// Moves an entity under another parent.
    Reparent {
        /// The entity to move.
        child: EntityId,
        /// Its new parent, `None` for the scene's root.
        parent: Option<EntityId>,
    },
    /// Adds a component to an entity, by its registered name.
    AddComponent {
        /// The entity to add it to.
        entity: EntityId,
        /// The component's registered type name, such as `"Camera"`.
        type_name: String,
    },
    /// Brings an entity's whole prefab instance back to its prefab's values.
    RevertPrefab {
        /// Any member of the instance.
        entity: EntityId,
    },
    /// Writes an override of the inspected entity into its prefab.
    ApplyToPrefab {
        /// The entity whose override is applied.
        entity: EntityId,
        /// What of it is applied.
        scope: PrefabApplyScope,
    },
    /// Saves an entity as a `.kprefab`, asking for the file through a dialog.
    SaveAsPrefab {
        /// The entity to save.
        entity: EntityId,
    },
    /// Saves an entity's inline material as `assets/materials/<name>.kmat`.
    SaveAsMaterial {
        /// The entity whose material is saved; it then references the file.
        entity: EntityId,
        /// The file's stem.
        name: String,
    },
    /// Reports what the last scene load adapted, as the `Load` problems.
    ///
    /// An empty list clears them (a new scene).
    ReportLoadProblems {
        /// The problems the load found.
        problems: Vec<Problem>,
    },
    /// Opens a scene file, prepared off the frame.
    OpenScene {
        /// The scene file's absolute path.
        path: String,
    },
    /// Spawns a prefab as a linked instance.
    ///
    /// A `.kprefab`, a glTF scene sub-asset, or a glTF file standing for its
    /// default scene.
    SpawnPrefab {
        /// The prefab's identity.
        prefab: AssetUUID,
        /// The entity to parent the instance's root under, `None` for the scene's root.
        parent: Option<EntityId>,
    },
    /// Saves an entity as a `.kprefab` at a path chosen beforehand, with no dialog.
    SaveAsPrefabAt {
        /// The entity whose subtree is saved.
        entity: EntityId,
        /// The file's forward-slash path under `<project>/assets/`.
        ///
        /// A path ending in `/` takes the entity's name as its file stem.
        path: String,
    },
    /// Spawns an entity drawing a mesh asset: a mesh file or a mesh sub-asset.
    SpawnMesh {
        /// The mesh's identity.
        mesh: AssetUUID,
        /// Where it is spawned, in world space.
        point: [f32; 3],
        /// The entity to parent it under, `None` for the scene's root.
        parent: Option<EntityId>,
    },
    /// Makes every selected entity reference a material by its identity.
    ///
    /// A `.kmat`, or a material inside another file, which a path cannot name.
    AssignMaterial {
        /// The material's identity.
        material: AssetUUID,
    },
    /// Gives an entity a texture or a material dropped on it.
    ///
    /// A material is referenced; a texture becomes the base colour of an
    /// inline standard material.
    AssignDropped {
        /// The texture's or material's identity.
        asset: AssetUUID,
        /// The entity it was dropped on.
        entity: EntityId,
    },
    /// Shows an asset in the asset browser: its folder opened, its tile selected and inspected.
    RevealAsset {
        /// The asset's forward-slash path under `assets/`.
        path: String,
    },
    /// Releases a quarantined asset, so that it is read again.
    ReleaseAsset {
        /// The asset's forward-slash path under `assets/`.
        path: String,
    },
    /// A file operation on the project's assets.
    AssetFile(AssetFileOp),
}

/// A file operation the asset browser asks for, on paths under the project's `assets/`.
///
/// Every path is forward-slash and relative to `assets/`. Each goes through
/// the project's identity registry, so references survive renames and moves.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AssetFileOp {
    /// Creates an empty folder.
    CreateFolder {
        /// The folder's path.
        path: String,
    },
    /// Creates an Ergon script, never over an existing file, then reveals it.
    CreateScript {
        /// The script's path.
        path: String,
        /// Its source.
        source: String,
    },
    /// Renames an asset, which keeps its identity.
    Rename {
        /// Its current path.
        from: String,
        /// Its new path.
        to: String,
    },
    /// Moves an asset into a folder, which keeps its identity.
    Move {
        /// The asset's path.
        path: String,
        /// The folder's path, `""` for the root.
        into: String,
    },
    /// Sends an asset to the system's recycle bin.
    Trash {
        /// The asset's path.
        path: String,
    },
    /// Copies an asset next to itself.
    Duplicate {
        /// The asset's path.
        path: String,
    },
}

/// A command of the editor's menus, command palette or shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuAction {
    /// Starts an empty scene.
    NewScene,
    /// Opens a scene file picked through a dialog.
    Open,
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

/// A command of the play transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Transport {
    /// Starts or resumes playing.
    Play,
    /// Pauses playing.
    Pause,
    /// Stops playing and restores the scene as Play found it.
    Stop,
}

/// The user's answer about trusting a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrustAnswer {
    /// Trusts the project waiting for an answer, and opens it.
    TrustOpen,
    /// Opens the project waiting for an answer restricted.
    RestrictOpen,
    /// Opens nothing of the project waiting for an answer.
    CancelOpen,
    /// Trusts the project open.
    TrustProject,
}
