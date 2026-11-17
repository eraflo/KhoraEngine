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

//! The asset type handlers, found by the `AssetType` they own.

use khora_sdk::khora_core::asset::AssetType;

use super::super::handlers::{
    builtins, handler_for, tile_kind_for, ActivationKind, AssetTypeHandler,
};
use crate::widgets::tile::AssetTileKind;

/// Each built-in handler is the one found for the type it owns.
#[test]
fn each_builtin_handler_is_found_by_the_type_it_owns() {
    let handlers: [&dyn AssetTypeHandler; 8] = [
        &builtins::SceneHandler,
        &builtins::MeshHandler,
        &builtins::TextureHandler,
        &builtins::AudioHandler,
        &builtins::ShaderHandler,
        &builtins::PrefabHandler,
        &builtins::MaterialHandler,
        &builtins::ScriptHandler,
    ];
    for handler in handlers {
        let owned = handler.asset_type();
        let found = handler_for(&owned).expect("a handler is registered for its type");
        assert_eq!(found.asset_type(), owned);
        assert_eq!(found.tile_kind(), handler.tile_kind());
        assert_eq!(tile_kind_for(&owned), handler.tile_kind());
    }
}

/// A prefab is drawn as a scene.
#[test]
fn a_prefab_shows_the_scene_tile() {
    assert_eq!(tile_kind_for(&AssetType::Prefab), AssetTileKind::Scene);
}

/// A scene's handler loads the scene activated, at its path.
#[test]
fn activating_a_scene_loads_it() {
    let handler = handler_for(&AssetType::Scene).expect("scenes have a handler");
    let activation = handler.activate("project/assets/main.kscene".to_owned());
    assert!(
        matches!(
            &activation,
            ActivationKind::LoadScene { abs_path } if abs_path == "project/assets/main.kscene"
        ),
        "{activation:?}"
    );
}

/// A type no handler owns finds none and shows the unknown tile.
#[test]
fn an_unknown_type_gets_the_unknown_tile() {
    let unknown = AssetType::Other("xyz".to_owned());
    assert!(handler_for(&unknown).is_none());
    assert_eq!(tile_kind_for(&unknown), AssetTileKind::Unknown);
}
