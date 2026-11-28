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

//! The Rust type of each asset kind's values, as a whole engine binds them.

use std::any::TypeId;
use std::collections::BTreeMap;

use khora_core::asset::font::Font;
use khora_core::asset::{kinds, AssetKind, Material};
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::resource::CpuTexture;
use khora_data::assets::{AssetValueRegistration, SoundData};
use khora_data::scene::PrefabAsset;

/// Each kind a value of which an asset reference names is bound, once, to its
/// value type; a scene (poured into the world) and a script (an Ergon module)
/// have no value bound.
#[test]
fn each_value_type_names_its_kind() {
    let mut bindings: BTreeMap<AssetKind, Vec<TypeId>> = BTreeMap::new();
    for registration in inventory::iter::<AssetValueRegistration> {
        bindings
            .entry(registration.kind)
            .or_default()
            .push((registration.value)());
    }

    for (kind, value) in [
        (kinds::MESH, TypeId::of::<Mesh>()),
        (kinds::MATERIAL, TypeId::of::<Box<dyn Material>>()),
        (kinds::TEXTURE, TypeId::of::<CpuTexture>()),
        (kinds::SOUND, TypeId::of::<SoundData>()),
        (kinds::FONT, TypeId::of::<Font>()),
        (kinds::PREFAB, TypeId::of::<PrefabAsset>()),
    ] {
        assert_eq!(
            bindings.get(&kind).map(Vec::as_slice),
            Some([value].as_slice()),
            "`{}` binds its value type once",
            kind.name()
        );
    }
    for kind in [kinds::SCENE, kinds::SCRIPT] {
        assert_eq!(
            bindings.get(&kind),
            None,
            "`{}` has no value bound",
            kind.name()
        );
    }
}
