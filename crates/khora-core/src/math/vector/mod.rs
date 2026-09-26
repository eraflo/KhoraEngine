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

//! Provides 2D, 3D, and 4D vector types and their associated operations.

use super::EPSILON;

mod vec2;
mod vec3;
mod vec4;

pub use vec2::Vec2;
pub use vec3::Vec3;
pub use vec4::Vec4;

#[cfg(test)]
mod tests;
