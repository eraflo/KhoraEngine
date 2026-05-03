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

//! Render-data layer — the per-frame extracted scene representation.
//!
//! Per CLAD this module is a **data** responsibility. The per-frame
//! projection of the scene is now owned by
//! [`RenderFlow`](crate::flow::RenderFlow), which publishes a
//! [`RenderWorld`] into the
//! [`LaneBus`](khora_core::lane::LaneBus) during the Substrate Pass.
//!
//! Render lanes consume that view from the bus; they no longer query the
//! ECS World directly.

mod editor_view;
mod frame_graph;
mod shadow_outputs;
mod world;

pub use editor_view::EditorViewportOverride;
pub use frame_graph::{
    submit_frame_graph, FrameGraph, PassDescriptor, ResourceId, SharedFrameGraph,
};
pub use shadow_outputs::{ShadowEntries, ShadowEntry};
pub use world::{ExtractedLight, ExtractedMesh, ExtractedView, RenderWorld};

use khora_core::{
    math::{Mat4, Vec3},
    renderer::api::resource::ViewInfo,
    ServiceRegistry,
};

use crate::ecs::{Camera, GlobalTransform, World};

/// Shadow result for a single light: view-projection matrix + atlas layer index.
pub type ShadowResult = (khora_core::math::Mat4, i32);

/// Extracts the first **active** camera as a [`ViewInfo`] suitable for
/// pushing to the renderer.
///
/// Returns `None` if no entity has an active camera (editor edit mode,
/// empty scene, etc.) — callers can fall back to a default `ViewInfo`.
///
/// Lives outside the [`RenderFlow`](crate::flow::RenderFlow) because it is
/// also used by the editor for non-render concerns (camera prepare, gizmo
/// space, etc.).
pub fn extract_active_camera_view(world: &World) -> Option<ViewInfo> {
    for (camera, global_transform) in world.query::<(&Camera, &GlobalTransform)>() {
        if !camera.is_active {
            continue;
        }

        let world_matrix = global_transform.to_matrix();
        let camera_position = Vec3::new(
            world_matrix.cols[3][0],
            world_matrix.cols[3][1],
            world_matrix.cols[3][2],
        );
        let view_matrix = world_matrix.inverse().unwrap_or(Mat4::IDENTITY);
        let projection_matrix = camera.projection_matrix();

        return Some(ViewInfo::new(
            view_matrix,
            projection_matrix,
            camera_position,
        ));
    }
    None
}

/// Returns the primary [`ExtractedView`] for the frame: the first active
/// scene `Camera`, or — if none — the editor's
/// [`EditorViewportOverride`].
///
/// Shared by [`RenderFlow`](crate::flow::RenderFlow) and
/// [`ShadowFlow`](crate::flow::ShadowFlow) so both flows agree on which
/// view their data is built around (CSM frustum slicing in particular
/// needs the same camera the lit pass will sample shadows from).
pub fn primary_view(world: &World, services: &ServiceRegistry) -> Option<ExtractedView> {
    for (camera, global_transform) in world.query::<(&Camera, &GlobalTransform)>() {
        if !camera.is_active {
            continue;
        }
        let position = global_transform.0.translation();
        let rotation = global_transform.0.rotation();
        let view_matrix = Mat4::from_quat(rotation.inverse()) * Mat4::from_translation(-position);
        let view_proj = camera.projection_matrix() * view_matrix;
        return Some(ExtractedView {
            view_proj,
            position,
        });
    }
    services
        .get::<EditorViewportOverride>()
        .and_then(|o| o.get())
}
