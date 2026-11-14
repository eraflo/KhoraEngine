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

//! The asset type handlers: per asset type, the tile it shows and what a
//! double-click does. Any crate registers one with
//! [`AssetTypeHandlerRegistration`]; the built-in ones are in [`builtins`].

use crate::widgets::tile::AssetTileKind;

/// Action to take after the user activates (double-clicks) an asset.
#[derive(Debug, Clone)]
pub enum ActivationKind {
    /// Loads this scene into the world, through the project VFS when the
    /// path is inside the project.
    LoadScene { abs_path: String },
    /// Places an instance of the prefab in the scene, on the next frame
    /// (`EditorState::pending_prefab_spawn`).
    SpawnPrefab,
    /// Opens the file with the OS-default application.
    OpenExternal { abs_path: String },
}

/// Pluggable handler for one asset type.
pub trait AssetTypeHandler: Send + Sync + 'static {
    /// Whether this handler owns `type_name`, the lower-case
    /// `AssetMetadata::asset_type_name`.
    fn matches_type_name(&self, type_name: &str) -> bool;
    /// Returns the tile this type shows in the grid.
    fn tile_kind(&self) -> AssetTileKind;
    /// Returns what a double-click does; by default, opens the file externally.
    fn activate(&self, abs_path: String) -> ActivationKind {
        ActivationKind::OpenExternal { abs_path }
    }
}

/// An asset type handler, registered through `inventory::submit!`.
pub struct AssetTypeHandlerRegistration {
    /// The handler.
    pub handler: &'static dyn AssetTypeHandler,
}

inventory::collect!(AssetTypeHandlerRegistration);

/// Returns the handler registered for `type_name`, if any.
///
/// With none, the asset shows the unknown tile and opens externally.
pub fn handler_for(type_name: &str) -> Option<&'static dyn AssetTypeHandler> {
    inventory::iter::<AssetTypeHandlerRegistration>
        .into_iter()
        .map(|r| r.handler)
        .find(|h| h.matches_type_name(type_name))
}

/// Returns the tile `type_name` shows, the unknown tile without a handler.
pub fn tile_kind_for(type_name: &str) -> AssetTileKind {
    handler_for(type_name)
        .map(|h| h.tile_kind())
        .unwrap_or(AssetTileKind::Unknown)
}

/// The handlers of the engine's own asset types.
pub mod builtins {
    use super::*;

    /// The handler of scenes: a double-click loads one.
    pub struct SceneHandler;
    impl AssetTypeHandler for SceneHandler {
        fn matches_type_name(&self, n: &str) -> bool {
            n == "scene"
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Scene
        }
        fn activate(&self, abs_path: String) -> ActivationKind {
            ActivationKind::LoadScene { abs_path }
        }
    }

    /// The handler of meshes.
    pub struct MeshHandler;
    impl AssetTypeHandler for MeshHandler {
        fn matches_type_name(&self, n: &str) -> bool {
            n == "mesh"
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Mesh
        }
    }

    /// The handler of textures.
    pub struct TextureHandler;
    impl AssetTypeHandler for TextureHandler {
        fn matches_type_name(&self, n: &str) -> bool {
            n == "texture"
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Texture
        }
    }

    /// The handler of audio clips.
    pub struct AudioHandler;
    impl AssetTypeHandler for AudioHandler {
        fn matches_type_name(&self, n: &str) -> bool {
            n == "audio"
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Audio
        }
    }

    /// The handler of shaders.
    pub struct ShaderHandler;
    impl AssetTypeHandler for ShaderHandler {
        fn matches_type_name(&self, n: &str) -> bool {
            n == "shader"
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Shader
        }
    }

    /// The handler of prefabs: a double-click places an instance.
    pub struct PrefabHandler;
    impl AssetTypeHandler for PrefabHandler {
        fn matches_type_name(&self, n: &str) -> bool {
            n == "prefab"
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Scene
        }
        fn activate(&self, _abs_path: String) -> ActivationKind {
            ActivationKind::SpawnPrefab
        }
    }

    /// The handler of materials.
    pub struct MaterialHandler;
    impl AssetTypeHandler for MaterialHandler {
        fn matches_type_name(&self, n: &str) -> bool {
            n == "material"
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Material
        }
    }

    /// The handler of Ergon scripts.
    pub struct ScriptHandler;
    impl AssetTypeHandler for ScriptHandler {
        fn matches_type_name(&self, n: &str) -> bool {
            n == "script"
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Script
        }
    }

    inventory::submit! {
        super::AssetTypeHandlerRegistration { handler: &SceneHandler }
    }
    inventory::submit! {
        super::AssetTypeHandlerRegistration { handler: &MeshHandler }
    }
    inventory::submit! {
        super::AssetTypeHandlerRegistration { handler: &TextureHandler }
    }
    inventory::submit! {
        super::AssetTypeHandlerRegistration { handler: &AudioHandler }
    }
    inventory::submit! {
        super::AssetTypeHandlerRegistration { handler: &ShaderHandler }
    }
    inventory::submit! {
        super::AssetTypeHandlerRegistration { handler: &ScriptHandler }
    }
    inventory::submit! {
        super::AssetTypeHandlerRegistration { handler: &PrefabHandler }
    }
    inventory::submit! {
        super::AssetTypeHandlerRegistration { handler: &MaterialHandler }
    }
}
