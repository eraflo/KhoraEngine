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

/// A 1D extent (e.g., width or depth).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Extent1D {
    pub width: u32,
}

/// A 2D extent (e.g., width and height).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Extent2D {
    pub width: u32,
    pub height: u32,
}

/// A 3D extent (e.g., width, height, and depth/array layers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Extent3D {
    pub width: u32,
    pub height: u32,
    pub depth_or_array_layers: u32,
}

/// A 2D origin (e.g., x and y offset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Origin2D {
    pub x: u32,
    pub y: u32,
}

/// A 3D origin (e.g., x, y, and z/array layer offset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Origin3D {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}
