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

//! `CameraFlow` — projects the active scene camera into a [`CameraView`] the
//! engine prepares the frame from.
//!
//! The view is the first **active** camera, projected at the aspect of the
//! frame's colour target ([`target_aspect`](crate::render::target_aspect)),
//! exactly as [`extract_active_camera_view`](crate::render::extract_active_camera_view)
//! builds it. With no active camera, it is the camera a tool set with
//! [`EditorViewportOverride::set_camera`] — the editor's own, while editing.
//! The SDK reads it once per frame, after the Flows run, and hands it to
//! `RenderSystem::prepare_frame`: no agent reaches the `World` for it.

use khora_core::renderer::api::resource::ViewInfo;
use khora_core::Runtime;

use crate::ecs::{CameraProblem, SemanticDomain, World};
use crate::flow::{Flow, Selection};
use crate::register_flow;
use crate::render::{extract_active_camera_view, target_aspect, EditorViewportOverride};

/// The active scene camera of this frame, as the renderer is prepared with it.
#[derive(Debug, Clone)]
pub struct CameraView {
    /// - `None` — no entity has an active camera, and no tool set a camera of
    ///   its own.
    /// - `Some(Err(problem))` — the active camera's values make no image; it
    ///   prepares nothing, and no other camera is picked in its place.
    /// - `Some(Ok(view))` — the active camera's view, projected at the frame
    ///   target's aspect; or, with none active, the tool's camera.
    pub view: Option<Result<ViewInfo, CameraProblem>>,
}

/// Projects the active scene camera into the frame's [`CameraView`].
#[derive(Default)]
pub struct CameraFlow;

impl Flow for CameraFlow {
    type View = CameraView;

    const DOMAIN: SemanticDomain = SemanticDomain::Render;
    const NAME: &'static str = "camera";

    fn project(&self, world: &World, _sel: &Selection, runtime: &Runtime) -> Self::View {
        let view = extract_active_camera_view(world, target_aspect(runtime)).or_else(|| {
            runtime
                .resources
                .get::<EditorViewportOverride>()
                .and_then(EditorViewportOverride::camera)
                .map(Ok)
        });
        CameraView { view }
    }
}

register_flow!(CameraFlow);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::{Camera, GlobalTransform};
    use crate::flow::FlowRegistration;
    use crate::render::{extract_active_camera_view, FALLBACK_ASPECT};
    use khora_core::math::Vec3;
    use std::f32::consts::FRAC_PI_4;

    /// The view `CameraFlow` publishes for `world` in a GPU-free run.
    fn camera_view(world: &World) -> CameraView {
        CameraFlow.project(world, &Selection::new(), &Runtime::default())
    }

    /// **The camera view is the active camera's, or the reason it has none.**
    /// A usable active camera gives the same `ViewInfo` the renderer was
    /// prepared with before (`extract_active_camera_view` at the target's
    /// aspect, here the fallback: a GPU-free run has no target); an active
    /// camera whose near plane sits on the eye gives its `CameraProblem`, not
    /// a view; a world without an active camera gives nothing.
    #[test]
    fn camera_view_carries_the_active_camera_and_its_problem() {
        let position = Vec3::new(1.0, 2.0, 5.0);
        let mut world = World::new();
        let camera = world.spawn((
            GlobalTransform::at_position(position),
            Camera::new_perspective(FRAC_PI_4, 0.1, 100.0),
        ));

        let expected = extract_active_camera_view(&world, FALLBACK_ASPECT)
            .expect("an active camera")
            .expect("a usable camera");
        match camera_view(&world).view {
            Some(Ok(view)) => {
                assert_eq!(view.view_matrix, expected.view_matrix, "view matrix");
                assert_eq!(
                    view.projection_matrix, expected.projection_matrix,
                    "projection at the target aspect"
                );
                assert_eq!(view.camera_position, position, "camera position");
            }
            other => panic!("a usable active camera gives its view, got {other:?}"),
        }

        let mut unusable = *world.get::<Camera>(camera).expect("the camera");
        unusable.z_near = 0.0;
        world.set_component(camera, unusable);
        assert_eq!(
            camera_view(&world).view.map(|view| view.err()),
            Some(Some(CameraProblem::NearNotPositive { near: 0.0 })),
            "an unusable active camera gives its problem"
        );

        let mut inactive = unusable;
        inactive.is_active = false;
        world.set_component(camera, inactive);
        assert!(
            camera_view(&world).view.is_none(),
            "no active camera, no view"
        );
    }

    /// The flow runs every frame like any other: it is registered, under its
    /// name, for the render domain.
    #[test]
    fn the_camera_flow_is_registered() {
        let registered = inventory::iter::<FlowRegistration>
            .into_iter()
            .find(|registration| registration.name == CameraFlow::NAME);
        match registered {
            Some(registration) => assert_eq!(registration.domain, SemanticDomain::Render),
            None => panic!(
                "`{}` is not registered with `register_flow!`",
                CameraFlow::NAME
            ),
        }
    }
}
