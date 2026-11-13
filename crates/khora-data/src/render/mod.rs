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

/// The aspect of the image this frame renders into: the
/// [`ColorTargetSize`] the engine publishes in the [`FrameContext`] once the
/// frame's target is acquired, or [`FALLBACK_ASPECT`] when there is none.
///
/// Every camera projection takes its aspect from here, so a camera fills the
/// window — or the editor's viewport — without being stretched, whatever its
/// shape and however often it is resized.
pub fn target_aspect(runtime: &Runtime) -> f32 {
    runtime
        .resources
        .get::<std::sync::Arc<FrameContext>>()
        .and_then(|fctx| fctx.get::<ColorTargetSize>())
        .map_or(FALLBACK_ASPECT, |size| size.aspect())
}

/// The view matrix (world → view) of a camera placed at `global`: the inverse
/// of its rigid pose (rotation and translation). The entity's scale is
/// ignored — a camera's zoom is its projection's job, never its transform's.
///
/// Every view of a scene camera is built here — [`extract_active_camera_view`]
/// and [`primary_view`] alike — so what the renderer is handed and what the
/// flows slice their data around are the same view.
pub fn camera_view_matrix(global: &GlobalTransform) -> Mat4 {
    Mat4::from_quat(global.0.rotation().inverse()) * Mat4::from_translation(-global.0.translation())
}

/// Extracts the first **active** camera as a [`ViewInfo`] suitable for
/// pushing to the renderer, projected for an image whose width over height
/// is `aspect` (usually [`target_aspect`]). Its view is the
/// [`camera_view_matrix`] of the camera's entity: scale is ignored.
///
/// The first active camera is chosen **before** it is checked:
/// - `None` — no entity has an active camera (editor edit mode, empty
///   scene, etc.); callers can fall back to a default `ViewInfo`.
/// - `Some(Err(problem))` — the active camera's values make no image (see
///   [`Camera::problems`](crate::ecs::Camera::problems)); it renders
///   nothing, and no other camera is picked in its place.
/// - `Some(Ok(view))` — the active camera's view.
///
/// Lives outside the [`RenderFlow`](crate::flow::RenderFlow) because it is
/// also used by the editor for non-render concerns (camera prepare, gizmo
/// space, etc.).
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

/// Returns the primary [`ExtractedView`] for the frame: the first active
/// scene `Camera`, projected at the [`target_aspect`], or — if none is
/// active — the editor's [`EditorViewportOverride`]. An active camera whose
/// values make no image gives `None`: it renders nothing, and the override
/// does not stand in for it.
///
/// Shared by [`RenderFlow`](crate::flow::RenderFlow) and
/// [`ShadowFlow`](crate::flow::ShadowFlow) so both flows agree on which
/// view their data is built around (CSM frustum slicing in particular
/// needs the same camera the lit pass will sample shadows from).
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

/// Bit-level fingerprint of the [`EditorViewportOverride`] as seen by
/// [`primary_view`]: distinguishes "no override" from "override present"
/// and any change in the override's value (editor camera moves do not
/// mutate the ECS, so flows that consult `primary_view` must fold this
/// into their cache key or they would serve stale views while the editor
/// camera flies).
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
