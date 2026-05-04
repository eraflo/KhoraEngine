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
//! The type is intentionally generic — it lives in the data layer so that
//! [`RenderFlow`] does not need to know about any editor crate.
//!
//! [`RenderFlow`]: crate::flow::RenderFlow

use std::sync::{Arc, RwLock};

use super::ExtractedView;

/// Shared, mutable per-frame view override.
///
/// Cloning shares the inner state — every `Arc` clone observes the same
/// override.
#[derive(Clone, Default)]
pub struct EditorViewportOverride(Arc<RwLock<Option<ExtractedView>>>);

impl EditorViewportOverride {
    /// Creates a new, empty override.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the current override view (or clears it with `None`).
    pub fn set(&self, view: Option<ExtractedView>) {
        if let Ok(mut slot) = self.0.write() {
            *slot = view;
        }
    }

    /// Returns the current override view, if any.
    pub fn get(&self) -> Option<ExtractedView> {
        self.0.read().ok().and_then(|slot| slot.clone())
    }

    /// Clears the current override.
    pub fn clear(&self) {
        self.set(None);
    }
}
