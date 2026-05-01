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

//! Render-data layer — the per-frame extracted scene representation and the
//! free functions that populate it from the ECS.
//!
//! Per CLAD this module is a **data** responsibility, not a service: there
//! is no `pub struct SceneExtractor`.  Extraction is a free function called
//! by the engine's hot loop before any agent runs.  Agents access the
//! resulting `RenderWorld` through the shared `RenderWorldStore` container.

mod extract;
mod frame_graph;
mod store;
mod world;

pub use extract::{extract_active_camera_view, extract_scene};
pub use frame_graph::{
    submit_frame_graph, FrameGraph, PassDescriptor, ResourceId, SharedFrameGraph,
};
pub use store::RenderWorldStore;
pub use world::{ExtractedLight, ExtractedMesh, ExtractedView, RenderWorld};

/// Shadow result for a single light: view-projection matrix + atlas layer index.
pub type ShadowResult = (khora_core::math::Mat4, i32);
