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

//! `EditorViewportOverride` — fallback view consumed by [`RenderFlow`] when
//! no active scene `Camera` exists.
//!
//! Tools that drive their own camera outside the ECS (the editor in
//! Editing mode, headless screenshot tools, …) write the desired
//! [`ExtractedView`] here once per frame **before** the scheduler runs.
//! `RenderFlow` reads it during `project()` and appends it to
//! `RenderWorld.views` if the world produced none of its own.
//!
//! A tool that sets its whole camera with [`set_camera`] also hands
//! [`CameraFlow`] the [`ViewInfo`] the renderer is prepared with when no scene
//! camera is active.
//!
//! The type is intentionally generic — it lives in the data layer so that
//! [`RenderFlow`] does not need to know about any editor crate.
//!
//! [`RenderFlow`]: crate::flow::RenderFlow
//! [`CameraFlow`]: crate::flow::CameraFlow
//! [`set_camera`]: EditorViewportOverride::set_camera

use std::sync::{Arc, RwLock};

use khora_core::renderer::api::resource::ViewInfo;

use super::ExtractedView;

/// What the host set: the view the flows fall back to, and the camera it was
/// built from when the host gave one.
#[derive(Clone, Default)]
struct Override {
    view: Option<ExtractedView>,
    camera: Option<ViewInfo>,
}

/// Shared, mutable per-frame view override.
///
/// Cloning shares the inner state — every `Arc` clone observes the same
/// override.
#[derive(Clone, Default)]
pub struct EditorViewportOverride(Arc<RwLock<Override>>);

impl EditorViewportOverride {
    /// Creates a new, empty override.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the current override view (or clears it with `None`). A view
    /// set alone carries no camera: [`camera`](Self::camera) returns `None`.
    pub fn set(&self, view: Option<ExtractedView>) {
        if let Ok(mut slot) = self.0.write() {
            *slot = Override { view, camera: None };
        }
    }

    /// Replaces the override with a whole camera (or clears it with `None`):
    /// the flows fall back to its view, and the renderer is prepared with it.
    pub fn set_camera(&self, camera: Option<ViewInfo>) {
        if let Ok(mut slot) = self.0.write() {
            *slot = Override {
                view: camera.as_ref().map(|camera| ExtractedView {
                    view_proj: camera.view_projection_matrix(),
                    position: camera.camera_position,
                }),
                camera,
            };
        }
    }

    /// Returns the current override view, if any.
    pub fn get(&self) -> Option<ExtractedView> {
        self.0.read().ok().and_then(|slot| slot.view.clone())
    }

    /// Returns the camera set with [`set_camera`](Self::set_camera), if any.
    pub fn camera(&self) -> Option<ViewInfo> {
        self.0.read().ok().and_then(|slot| slot.camera.clone())
    }

    /// Clears the current override.
    pub fn clear(&self) {
        self.set(None);
    }
}
