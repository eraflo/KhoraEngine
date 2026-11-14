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

//! A three-dimensional extent, in whole units.

/// A three-dimensional extent, representing width, height, and depth.
///
/// This is used for 3D textures, texture arrays, or cubemaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Extent3D {
    /// The width component of the extent.
    pub width: u32,
    /// The height component of the extent.
    pub height: u32,
    /// The depth or number of array layers.
    pub depth_or_array_layers: u32,
}
