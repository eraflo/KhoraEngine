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

//! How the editor shows a kind of asset: its icon, and the word a sentence
//! names it by. One place, so a renamed kind changes here only.

use khora_sdk::editor_ui::Icon;
use khora_sdk::khora_core::asset::AssetKind;

/// The icon an asset of `kind` is shown with.
pub fn asset_kind_icon(kind: AssetKind) -> Icon {
    match kind {
        AssetKind::Prefab => Icon::Prefab,
        AssetKind::Sound => Icon::Music,
        AssetKind::Material => Icon::Circle,
        AssetKind::Mesh => Icon::Cube,
        AssetKind::Texture => Icon::Image,
        AssetKind::Scene => Icon::Film,
    }
}

/// The word a sentence names an asset of `kind` by: `"prefab"`, `"sound"`, …
pub fn asset_kind_noun(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Prefab => "prefab",
        AssetKind::Sound => "sound",
        AssetKind::Material => "material",
        AssetKind::Mesh => "mesh",
        AssetKind::Texture => "texture",
        AssetKind::Scene => "scene",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Each kind has its own icon** — a sound its note, a scene its film.
    #[test]
    fn each_asset_kind_is_shown_with_its_icon() {
        let shown: Vec<Icon> = AssetKind::ALL.into_iter().map(asset_kind_icon).collect();
        assert_eq!(
            shown,
            [
                Icon::Prefab,
                Icon::Music,
                Icon::Circle,
                Icon::Cube,
                Icon::Image,
                Icon::Film,
            ]
        );
    }

    /// **Each kind is named by its own word** — a sound is a `sound`, not the
    /// index's `audio`.
    #[test]
    fn each_asset_kind_is_named_by_its_noun() {
        let named: Vec<&str> = AssetKind::ALL.into_iter().map(asset_kind_noun).collect();
        assert_eq!(
            named,
            ["prefab", "sound", "material", "mesh", "texture", "scene"]
        );
    }
}
