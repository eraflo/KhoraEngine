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

//! Which winding order makes a triangle front-facing.

/// Defines which vertex winding order considers a triangle to be "front-facing".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrontFace {
    /// Counter-clockwise winding order is the front face (e.g., OpenGL default).
    Ccw,
    /// Clockwise winding order is the front face (e.g., DirectX default).
    Cw,
}
