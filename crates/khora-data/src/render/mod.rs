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

//! The render data: the [`RenderWorld`] View the render lanes draw from, the
//! overlays a host hands the engine, the frame graph, and the camera views
//! they are all built around.

mod editor_view;
mod frame_graph;
mod gizmo;
mod grid;
mod shining;
mod wireframe;
mod world;

pub use editor_view::EditorViewportOverride;
pub use frame_graph::{
    pass_order, registered_pass_order, submit_frame_graph, FrameGraph, GizmoPassSlot, GridPassSlot,
    PassContribution, PassDescriptor, PassOrderError, PassRegistration, ResourceId, ScenePassSlot,
    ShadowPassSlot, SharedFrameGraph, SkyboxPassSlot, TransparentPassSlot, UiPassSlot,
    WireframePassSlot,
};
pub use gizmo::{GizmoFrame, GizmoView};
pub use grid::{GridConfig, GridView};
pub use shining::{shining_lights, ShiningLight};
pub use wireframe::{WireframeConfig, WireframeView};
pub use world::{ExtractedLight, ExtractedMesh, ExtractedView, RenderWorld};

use khora_core::{
    lane::ColorTargetSize,
    math::Mat4,
    renderer::api::{frame::FrameContext, resource::ViewInfo},
    Runtime,
};

use crate::ecs::{Camera, CameraProblem, GlobalTransform, World};

/// Shadow result for a single light: view-projection matrix + atlas layer index.
pub type ShadowResult = (khora_core::math::Mat4, i32);

/// The aspect a camera is projected at when no colour target is known (a
/// GPU-free run has no image to take a shape from).
pub const FALLBACK_ASPECT: f32 = 1.0;

/// Returns the aspect of the image this frame renders into, or
/// [`FALLBACK_ASPECT`].
///
/// Read from the [`ColorTargetSize`] in the [`FrameContext`]. Every camera
/// projection takes its aspect from here, so a camera fills the window or the
/// editor's viewport without being stretched.
pub fn target_aspect(runtime: &Runtime) -> f32 {
    runtime
        .resources
        .get::<std::sync::Arc<FrameContext>>()
        .and_then(|fctx| fctx.get::<ColorTargetSize>())
        .map_or(FALLBACK_ASPECT, |size| size.aspect())
}

/// Returns the view matrix (world to view) of a camera placed at `global`.
///
/// The inverse of its rigid pose: the entity's scale is ignored, a camera's
/// zoom being its projection's job. Every scene camera view is built here, so
/// the renderer and the flows agree on it.
pub fn camera_view_matrix(global: &GlobalTransform) -> Mat4 {
    Mat4::from_quat(global.0.rotation().inverse()) * Mat4::from_translation(-global.0.translation())
}

/// Returns the first active camera's [`ViewInfo`], projected at `aspect`
/// (usually [`target_aspect`]).
///
/// The first active camera is chosen before it is checked: `None` when no
/// camera is active, `Some(Err(problem))` when its values make no image (see
/// [`Camera::problems`](crate::ecs::Camera::problems)), and then no other
/// camera is picked in its place.
pub fn extract_active_camera_view(
    world: &World,
    aspect: f32,
) -> Option<Result<ViewInfo, CameraProblem>> {
    world
        .query::<(&Camera, &GlobalTransform)>()
        .find(|(camera, _)| camera.is_active)
        .map(|(camera, global_transform)| {
            let projection = camera.projection_matrix(aspect)?;
            Ok(ViewInfo::new(
                camera_view_matrix(global_transform),
                projection,
                global_transform.0.translation(),
            ))
        })
}

/// Returns the frame's primary view: the first active camera's, else the
/// [`EditorViewportOverride`].
///
/// An active camera whose values make no image gives `None`; the override does
/// not stand in for it. [`RenderFlow`](crate::flow::RenderFlow) and
/// [`ShadowFlow`](crate::flow::ShadowFlow) both build around it, so the shadow
/// cascades are sliced for the camera the lit pass samples them from.
pub fn primary_view(world: &World, runtime: &Runtime) -> Option<ExtractedView> {
    match extract_active_camera_view(world, target_aspect(runtime)) {
        Some(Ok(view)) => {
            return Some(ExtractedView {
                view_proj: view.view_projection_matrix(),
                position: view.camera_position,
            })
        }
        Some(Err(_)) => return None,
        None => {}
    }
    runtime
        .resources
        .get::<EditorViewportOverride>()
        .and_then(|o| o.get())
}

/// Fingerprints the [`EditorViewportOverride`] bit for bit; 0 when there is none.
///
/// The editor camera moves without touching the ECS, so a flow caching on
/// [`primary_view`] folds this into its key.
pub(crate) fn editor_override_fingerprint(runtime: &Runtime) -> u64 {
    use std::hash::{Hash, Hasher};

    let Some(view) = runtime
        .resources
        .get::<EditorViewportOverride>()
        .and_then(|o| o.get())
    else {
        return 0;
    };

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Presence marker, so an override that happens to hash to 0 still
    // differs from "no override".
    1u8.hash(&mut hasher);
    for col in &view.view_proj.cols {
        col.x.to_bits().hash(&mut hasher);
        col.y.to_bits().hash(&mut hasher);
        col.z.to_bits().hash(&mut hasher);
        col.w.to_bits().hash(&mut hasher);
    }
    view.position.x.to_bits().hash(&mut hasher);
    view.position.y.to_bits().hash(&mut hasher);
    view.position.z.to_bits().hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests;
