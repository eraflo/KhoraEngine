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

//! Per-frame shadow output slot for the [`OutputDeck`](crate::lane::OutputDeck).
//!
//! Every shadow strategy writes a [`ShadowFrame`] into the deck at the
//! end of its `execute()`; lit consumer lanes drain or peek the same
//! slot to look up shadow data. This is the **only** channel through
//! which shadow data flows between agents — no `FrameContext` hoist,
//! no `LaneContext` insertion, no shared `Resource`.

use super::{ShadowEntries, ShadowGpuBindings};

/// Per-frame shadow output published by the shadow lane into the deck.
///
/// Holds:
/// - The opaque [`ShadowGpuBindings`] bundle lit consumer lanes use to
///   build their group-3 bind group.
/// - The per-light [`ShadowEntries`] map lit consumer lanes look up to
///   embed shadow matrices / atlas indices in their per-light uniforms.
///
/// `bindings` may be `None` for a frame where the strategy had nothing
/// to bind (e.g. atlases still initialising); consumers fall back to no
/// shadows in that case. The `entries` field is always present (it may
/// be empty).
#[derive(Debug, Clone, Default)]
pub struct ShadowFrame {
    /// GPU resources to bind for the lighting bind group (slot 1/2/3).
    pub bindings: Option<ShadowGpuBindings>,
    /// Per-light shadow data, keyed by light index.
    pub entries: ShadowEntries,
}

impl ShadowFrame {
    /// Returns the bindings if present, falling back to `None` so the
    /// caller can early-return.
    pub fn bindings(&self) -> Option<&ShadowGpuBindings> {
        self.bindings.as_ref()
    }

    /// Convenience accessor for an entry by light index.
    pub fn entry(&self, light_index: usize) -> Option<&super::ShadowEntry> {
        self.entries.get(light_index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lane::OutputDeck;
    use crate::math::Mat4;

    #[test]
    fn shadow_frame_deck_round_trip() {
        let mut deck = OutputDeck::new();
        {
            let frame = deck.slot::<ShadowFrame>();
            frame.entries.insert(
                7,
                super::super::ShadowEntry::Atlas2D {
                    view_proj: Mat4::IDENTITY,
                    atlas_index: 0,
                },
            );
        }
        let taken: ShadowFrame = deck.take();
        assert_eq!(taken.entries.len(), 1);
        assert!(taken.entry(7).is_some());
    }
}
