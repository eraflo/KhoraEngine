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

//! Drag and drop for the egui UI builder: a `u64` payload published by the
//! dragged item and read by the item under the pointer, through egui's own
//! drag-and-drop store.

use super::ui_builder::EguiUiBuilder;

impl EguiUiBuilder<'_> {
    /// Publishes `payload` while the last item is being dragged.
    pub(super) fn attach_drag_payload(&mut self, payload: u64) {
        // Attach to the response left by the last `interact_rect` call.
        // That response was created with `Sense::click_and_drag()`, so
        // egui already detects drags — we just need to publish the payload.
        if let Some(response) = self.last_response.as_ref() {
            if response.dragged() {
                response.dnd_set_drag_payload::<u64>(payload);
            }
        }
    }

    /// The payload hovering the last item, if a drag carries one.
    pub(super) fn hovering_payload(&self) -> Option<u64> {
        self.last_response
            .as_ref()
            .and_then(|r| r.dnd_hover_payload::<u64>())
            .map(|payload| *payload)
    }

    /// The payload released on the last item this frame, if any.
    pub(super) fn released_payload(&mut self) -> Option<u64> {
        self.last_response
            .as_ref()
            .and_then(|r| r.dnd_release_payload::<u64>())
            .map(|payload| *payload)
    }

    /// Whether the last item is being dragged.
    pub(super) fn last_item_dragged(&self) -> bool {
        self.last_response
            .as_ref()
            .map(|r| r.dragged())
            .unwrap_or(false)
    }
}
