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

//! The overlay families: the editor overlays drawn over the scene, each its
//! own family with its own pass, composited in order — the ground grid, the
//! wireframe, then the gizmos.

mod gizmo;
mod gizmo_family;
mod grid;
mod grid_family;
mod wireframe;
mod wireframe_family;

pub use gizmo::{GizmoInputs, GizmoLane};
pub use gizmo_family::*;
pub use grid::{GridInputs, GridLane};
pub use grid_family::*;
pub use wireframe::{WireframeInputs, WireframeLane};
pub use wireframe_family::*;
