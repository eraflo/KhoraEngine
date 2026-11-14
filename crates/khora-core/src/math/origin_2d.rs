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

//! A two-dimensional origin, in whole units.

/// A two-dimensional origin, typically representing an (x, y) offset.
///
/// This is often used to specify the top-left corner of a rectangular region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Origin2D {
    /// The x-coordinate of the origin.
    pub x: u32,
    /// The y-coordinate of the origin.
    pub y: u32,
}
