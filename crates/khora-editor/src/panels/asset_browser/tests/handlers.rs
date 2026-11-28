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

//! The asset kind handlers, found by the `AssetKind` they own.

use khora_sdk::khora_core::asset::{kinds, AssetKind};

use super::super::handlers::{
    builtins, handler_for, tile_kind_for, ActivationKind, AssetKindHandler,
};
use crate::widgets::tile::AssetTileKind;

/// Each built-in handler is the one found for the kind it owns.
#[test]
fn each_builtin_handler_is_found_by_the_kind_it_owns() {
    let handlers: [&dyn AssetKindHandler; 8] = [
        &builtins::SceneHandler,
        &builtins::MeshHandler,
        &builtins::TextureHandler,
        &builtins::SoundHandler,
        &builtins::ShaderHandler,
        &builtins::PrefabHandler,
        &builtins::MaterialHandler,
        &builtins::ScriptHandler,
    ];
    for handler in handlers {
        let owned = handler.kind();
        let found = handler_for(owned).expect("a handler is registered for its kind");
        assert_eq!(found.kind(), owned);
        assert_eq!(found.tile_kind(), handler.tile_kind());
        assert_eq!(tile_kind_for(owned), handler.tile_kind());
    }
}

/// A prefab is drawn as a scene.
#[test]
fn a_prefab_shows_the_scene_tile() {
    assert_eq!(tile_kind_for(kinds::PREFAB), AssetTileKind::Scene);
}

/// A scene's handler loads the scene activated, at its path.
#[test]
fn activating_a_scene_loads_it() {
    let handler = handler_for(kinds::SCENE).expect("scenes have a handler");
    let activation = handler.activate("project/assets/main.kscene".to_owned());
    assert!(
        matches!(
            &activation,
            ActivationKind::LoadScene { abs_path } if abs_path == "project/assets/main.kscene"
        ),
        "{activation:?}"
    );
}

/// A kind no handler owns finds none and shows the unknown tile.
#[test]
fn a_kind_no_handler_owns_gets_the_unknown_tile() {
    let unhandled = AssetKind::__declare("xyz");
    assert!(handler_for(unhandled).is_none());
    assert_eq!(tile_kind_for(unhandled), AssetTileKind::Unknown);
    assert!(
        handler_for(kinds::FONT).is_none(),
        "a font has no tile of its own"
    );
}
