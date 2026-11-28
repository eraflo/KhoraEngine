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

//! The asset kind handlers: per asset kind, the tile it shows and what a
//! double-click does. Any crate registers one with
//! [`AssetKindHandlerRegistration`]; the built-in ones are in [`builtins`].

use khora_sdk::khora_core::asset::{kinds, AssetKind};

use crate::widgets::tile::AssetTileKind;

/// Action to take after the user activates (double-clicks) an asset.
#[derive(Debug, Clone)]
pub enum ActivationKind {
    /// Loads this scene into the world, through the project VFS when the
    /// path is inside the project.
    LoadScene { abs_path: String },
    /// Places an instance of the prefab in the scene, on the next frame
    /// (`EditorRequest::SpawnPrefab`).
    SpawnPrefab,
    /// Opens the file with the OS-default application.
    OpenExternal { abs_path: String },
}

/// Pluggable handler for one asset kind.
pub trait AssetKindHandler: Send + Sync + 'static {
    /// Returns the asset kind this handler owns.
    fn kind(&self) -> AssetKind;
    /// Returns the tile this kind shows in the grid.
    fn tile_kind(&self) -> AssetTileKind;
    /// Returns what a double-click does; by default, opens the file externally.
    fn activate(&self, abs_path: String) -> ActivationKind {
        ActivationKind::OpenExternal { abs_path }
    }
}

/// An asset kind handler, registered through `inventory::submit!`.
pub struct AssetKindHandlerRegistration {
    /// The handler.
    pub handler: &'static dyn AssetKindHandler,
}

inventory::collect!(AssetKindHandlerRegistration);

/// Returns the handler registered for `kind`, if any.
///
/// With none, the asset shows the unknown tile and opens externally.
pub fn handler_for(kind: AssetKind) -> Option<&'static dyn AssetKindHandler> {
    inventory::iter::<AssetKindHandlerRegistration>
        .into_iter()
        .map(|r| r.handler)
        .find(|h| h.kind() == kind)
}

/// Returns the tile `kind` shows, the unknown tile without a handler.
pub fn tile_kind_for(kind: AssetKind) -> AssetTileKind {
    handler_for(kind)
        .map(|h| h.tile_kind())
        .unwrap_or(AssetTileKind::Unknown)
}

/// The handlers of the engine's own asset kinds.
pub mod builtins {
    use super::*;

    /// The handler of scenes: a double-click loads one.
    pub struct SceneHandler;
    impl AssetKindHandler for SceneHandler {
        fn kind(&self) -> AssetKind {
            kinds::SCENE
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
    impl AssetKindHandler for MeshHandler {
        fn kind(&self) -> AssetKind {
            kinds::MESH
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Mesh
        }
    }

    /// The handler of textures.
    pub struct TextureHandler;
    impl AssetKindHandler for TextureHandler {
        fn kind(&self) -> AssetKind {
            kinds::TEXTURE
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Texture
        }
    }

    /// The handler of sounds.
    pub struct SoundHandler;
    impl AssetKindHandler for SoundHandler {
        fn kind(&self) -> AssetKind {
            kinds::SOUND
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Sound
        }
    }

    /// The handler of shaders.
    pub struct ShaderHandler;
    impl AssetKindHandler for ShaderHandler {
        fn kind(&self) -> AssetKind {
            kinds::SHADER
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Shader
        }
    }

    /// The handler of prefabs: a double-click places an instance.
    pub struct PrefabHandler;
    impl AssetKindHandler for PrefabHandler {
        fn kind(&self) -> AssetKind {
            kinds::PREFAB
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
    impl AssetKindHandler for MaterialHandler {
        fn kind(&self) -> AssetKind {
            kinds::MATERIAL
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Material
        }
    }

    /// The handler of Ergon scripts.
    pub struct ScriptHandler;
    impl AssetKindHandler for ScriptHandler {
        fn kind(&self) -> AssetKind {
            kinds::SCRIPT
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Script
        }
    }

    inventory::submit! {
        super::AssetKindHandlerRegistration { handler: &SceneHandler }
    }
    inventory::submit! {
        super::AssetKindHandlerRegistration { handler: &MeshHandler }
    }
    inventory::submit! {
        super::AssetKindHandlerRegistration { handler: &TextureHandler }
    }
    inventory::submit! {
        super::AssetKindHandlerRegistration { handler: &SoundHandler }
    }
    inventory::submit! {
        super::AssetKindHandlerRegistration { handler: &ShaderHandler }
    }
    inventory::submit! {
        super::AssetKindHandlerRegistration { handler: &ScriptHandler }
    }
    inventory::submit! {
        super::AssetKindHandlerRegistration { handler: &PrefabHandler }
    }
    inventory::submit! {
        super::AssetKindHandlerRegistration { handler: &MaterialHandler }
    }
}
