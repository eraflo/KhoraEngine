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

//! Defines the `Mat3` and `Mat4` types and associated operations.

use super::{Quaternion, Vec2, Vec3, Vec4, EPSILON};

mod mat3;
mod mat4;

pub use mat3::Mat3;
pub use mat4::Mat4;

#[cfg(test)]
mod tests;
