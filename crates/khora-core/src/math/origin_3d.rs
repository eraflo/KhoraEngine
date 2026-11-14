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

//! A three-dimensional origin, in whole units.

/// A three-dimensional origin, representing an (x, y, z) offset.
///
/// This is often used to specify the corner of a 3D volume or an offset
/// into a texture array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Origin3D {
    /// The x-coordinate of the origin.
    pub x: u32,
    /// The y-coordinate of the origin.
    pub y: u32,
    /// The z-coordinate or array layer of the origin.
    pub z: u32,
}
