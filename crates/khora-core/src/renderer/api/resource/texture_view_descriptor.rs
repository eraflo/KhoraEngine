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

//! The descriptor of a texture view: which format, dimension, aspect, mips and layers it reads.

use crate::renderer::api::resource::TextureFormat;
use std::borrow::Cow;

/// A descriptor used to create a [`TextureViewId`](super::TextureViewId).
/// A view describes a specific way to access a texture's data.
#[derive(Debug, Clone)]
pub struct TextureViewDescriptor<'a> {
    /// An optional debug label.
    pub label: Option<Cow<'a, str>>,
    /// The format of the texture view.
    pub format: Option<TextureFormat>,
    /// The dimensionality of the texture view.
    pub dimension: Option<TextureViewDimension>,
    /// The aspects of the texture to be accessed.
    pub aspect: ImageAspect,
    /// The first mipmap level to be accessed by the view.
    pub base_mip_level: u32,
    /// The number of mipmap levels to include in the view.
    pub mip_level_count: Option<u32>,
    /// The first array layer to be accessed by the view.
    pub base_array_layer: u32,
    /// The number of array layers to include in the view.
    pub array_layer_count: Option<u32>,
}

/// The dimensionality of a texture view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureViewDimension {
    /// A view of a 1D texture.
    D1,
    /// A view of a 2D texture.
    D2,
    /// A view of a 2D texture array.
    D2Array,
    /// A view of a cubemap texture (6 faces of a 2D texture).
    Cube,
    /// A view of a cubemap texture array.
    CubeArray,
    /// A view of a 3D texture.
    D3,
}

/// Defines which aspects of a texture are accessed by a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageAspect {
    /// Access all aspects (color, depth, and stencil).
    All,
    /// Access only the stencil component of a depth/stencil texture.
    StencilOnly,
    /// Access only the depth component of a depth/stencil texture.
    DepthOnly,
}
