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

//! Defines unlit materials for the rendering system.

use crate::{
    asset::{Asset, Material},
    math::LinearRgba,
};

/// A simple, unlit material.
///
/// This material does not react to lighting and simply renders with a solid
/// base color, optionally modulated by a texture. It's the most basic and
/// performant type of material.
#[derive(Debug, Clone)]
pub struct UnlitMaterial {
    /// The base color of the material.
    pub base_color: LinearRgba,
    // Future work:
    // pub base_color_texture: Option<AssetHandle<Texture>>,
}

// Mark `UnlitMaterial` as a valid asset.
impl Asset for UnlitMaterial {}

// Mark `UnlitMaterial` as a valid material.
impl Material for UnlitMaterial {}
