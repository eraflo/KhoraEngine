// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! `AssetTypeHandler` — extensibility seam for the Asset Browser.
//!
//! Each handler owns one `asset_type_name` (the lower-case identifier
//! produced by the index builder) and decides:
//!
//!   - which tile-kind / icon / category to show in the browser;
//!   - what happens when the user double-clicks it.
//!
//! Built-in handlers are registered through `inventory::submit!` from
//! [`builtins`]. Plugins can register their own by submitting to
//! [`AssetTypeHandlerRegistration`] from any crate that depends on
//! `khora-editor` (or via a future plugin loader).

use khora_sdk::editor_ui::Icon;

use crate::widgets::tile::AssetTileKind;

/// Action to take after the user activates (double-clicks) an asset.
#[derive(Debug, Clone)]
pub enum ActivationKind {
    /// Load this scene into the world. The dispatcher in
    /// `commands::load_scene_dispatch` routes it through the project
    /// VFS when the path is internal.
    LoadScene { abs_path: String },
    /// Open externally with the OS-default app.
    OpenExternal { abs_path: String },
}

/// Pluggable handler for one asset type.
///
/// `icon` and `category_label` are reserved for Phase 4 (folder-tree
/// sidebar driven entirely by handlers); built-in implementations
/// already supply them so the rewrite is a drop-in.
#[allow(dead_code)]
pub trait AssetTypeHandler: Send + Sync + 'static {
    /// Lower-case `asset_type_name` (matches `AssetMetadata::asset_type_name`).
    fn matches_type_name(&self, type_name: &str) -> bool;
    fn tile_kind(&self) -> AssetTileKind;
    fn icon(&self) -> Icon;
    fn category_label(&self) -> &'static str;
    /// Default activation: open externally.
    fn activate(&self, abs_path: String) -> ActivationKind {
        ActivationKind::OpenExternal { abs_path }
    }
}

/// Inventory entry — registered through `inventory::submit!`.
pub struct AssetTypeHandlerRegistration {
    pub handler: &'static dyn AssetTypeHandler,
}

inventory::collect!(AssetTypeHandlerRegistration);

/// Lookup the handler for a given `asset_type_name`. Returns `None` only
/// if no built-in or plugin handler matched (caller should fall back to
/// the unknown-type tile + open-external behaviour).
pub fn handler_for(type_name: &str) -> Option<&'static dyn AssetTypeHandler> {
    inventory::iter::<AssetTypeHandlerRegistration>
        .into_iter()
        .map(|r| r.handler)
        .find(|h| h.matches_type_name(type_name))
}

/// Tile kind by type name — convenience for the panel grid.
pub fn tile_kind_for(type_name: &str) -> AssetTileKind {
    handler_for(type_name)
        .map(|h| h.tile_kind())
        .unwrap_or(AssetTileKind::Unknown)
}

// ─── Built-in handlers ────────────────────────────────────────────────

pub mod builtins {
    use super::*;

    pub struct SceneHandler;
    impl AssetTypeHandler for SceneHandler {
        fn matches_type_name(&self, n: &str) -> bool {
            n == "scene"
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Scene
        }
        fn icon(&self) -> Icon {
            Icon::Film
        }
        fn category_label(&self) -> &'static str {
            "Scenes"
        }
        fn activate(&self, abs_path: String) -> ActivationKind {
            ActivationKind::LoadScene { abs_path }
        }
    }

    pub struct MeshHandler;
    impl AssetTypeHandler for MeshHandler {
        fn matches_type_name(&self, n: &str) -> bool {
            n == "mesh"
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Mesh
        }
        fn icon(&self) -> Icon {
            Icon::Cube
        }
        fn category_label(&self) -> &'static str {
            "Meshes"
        }
    }

    pub struct TextureHandler;
    impl AssetTypeHandler for TextureHandler {
        fn matches_type_name(&self, n: &str) -> bool {
            n == "texture"
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Texture
        }
        fn icon(&self) -> Icon {
            Icon::Image
        }
        fn category_label(&self) -> &'static str {
            "Textures"
        }
    }

    pub struct AudioHandler;
    impl AssetTypeHandler for AudioHandler {
        fn matches_type_name(&self, n: &str) -> bool {
            n == "audio"
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Audio
        }
        fn icon(&self) -> Icon {
            Icon::Music
        }
        fn category_label(&self) -> &'static str {
            "Audio"
        }
    }

    pub struct ShaderHandler;
    impl AssetTypeHandler for ShaderHandler {
        fn matches_type_name(&self, n: &str) -> bool {
            n == "shader"
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Shader
        }
        fn icon(&self) -> Icon {
            Icon::Zap
        }
        fn category_label(&self) -> &'static str {
            "Shaders"
        }
    }

    pub struct ScriptHandler;
    impl AssetTypeHandler for ScriptHandler {
        fn matches_type_name(&self, n: &str) -> bool {
            n == "script"
        }
        fn tile_kind(&self) -> AssetTileKind {
            AssetTileKind::Script
        }
        fn icon(&self) -> Icon {
            Icon::Code
        }
        fn category_label(&self) -> &'static str {
            "Scripts"
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
}
