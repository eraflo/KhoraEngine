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

//! Per-frame gizmo output slot for the
//! [`OutputDeck`](khora_core::lane::OutputDeck).
//!
//! Producers (typically the editor application) push line instances
//! into `GizmoFrame.lines`; the engine-side `GizmoLane` consumes the
//! slot and renders the overlay. Empty by default — a frame with no
//! gizmos triggers no GPU work.
//!
//! Symmetric to the [`ShadowFrame`](khora_core::renderer::api::shadow::ShadowFrame)
//! pattern: only contract types live in `khora-data`, no GPU code.

use khora_core::ui::editor::GizmoLineInstance;

/// Per-frame gizmo lines published by a host application (editor,
/// debug tooling) and consumed by the engine's `GizmoLane`.
#[derive(Debug, Default, Clone)]
pub struct GizmoFrame {
    /// Line segments to draw this frame.
    pub lines: Vec<GizmoLineInstance>,
}

impl GizmoFrame {
    /// Returns true when no producer published any lines this frame.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Returns the number of line instances queued this frame.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Clears all queued lines.
    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::lane::OutputDeck;
    use khora_core::math::Vec3;

    #[test]
    fn gizmo_frame_deck_round_trip() {
        let mut deck = OutputDeck::new();
        {
            let frame = deck.slot::<GizmoFrame>();
            frame.lines.push(GizmoLineInstance::new(
                Vec3::ZERO,
                Vec3::new(1.0, 0.0, 0.0),
                [1.0, 0.0, 0.0, 1.0],
            ));
        }
        let taken: GizmoFrame = deck.take();
        assert_eq!(taken.len(), 1);
    }

    #[test]
    fn empty_by_default() {
        let frame = GizmoFrame::default();
        assert!(frame.is_empty());
    }
}
