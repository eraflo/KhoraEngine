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

//! The asset kinds the engine declares.

crate::register_asset_kind!(
    /// A mesh, `Mesh` in Ergon.
    pub const MESH = "mesh", ergon = "Mesh"
);

crate::register_asset_kind!(
    /// A material, `Material` in Ergon.
    pub const MATERIAL = "material", ergon = "Material"
);

crate::register_asset_kind!(
    /// A texture, `Texture` in Ergon.
    pub const TEXTURE = "texture", ergon = "Texture"
);

crate::register_asset_kind!(
    /// A sound, `Sound` in Ergon.
    pub const SOUND = "sound", ergon = "Sound"
);

crate::register_asset_kind!(
    /// A font, `Font` in Ergon.
    pub const FONT = "font", ergon = "Font"
);

crate::register_asset_kind!(
    /// A prefab, `Prefab` in Ergon.
    pub const PREFAB = "prefab", ergon = "Prefab"
);

crate::register_asset_kind!(
    /// A scene, a document poured into the world; `Scene` in Ergon.
    pub const SCENE = "scene", ergon = "Scene"
);

crate::register_asset_kind!(
    /// A shader source, which no script holds.
    pub const SHADER = "shader"
);

crate::register_asset_kind!(
    /// An Ergon module, which no script holds as a value.
    pub const SCRIPT = "script"
);
