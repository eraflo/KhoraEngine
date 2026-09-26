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

//! The rendering strategies the render agent negotiates between.

/// Rendering strategy selection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderingStrategy {
    /// Simple unlit rendering (vertex colors only).
    #[default]
    Unlit,
    /// Standard forward rendering with lighting (Blinn-Phong).
    LitForward,
    /// Full PBR (Cook-Torrance) forward rendering with shadows.
    StandardPbr,
    /// Forward+ (tiled forward) rendering with compute-based light culling.
    ForwardPlus,
    /// Automatic selection based on scene complexity (light count).
    Auto,
}
