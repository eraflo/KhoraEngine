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

//! The `u64` drag-and-drop payloads the editor's panels exchange: asset
//! tiles, scene-tree entities, and the dock's own tab tag. One module, so a
//! receiver classifies every kind the same way.

use khora_tool_ui::widgets::DOCK_DRAG_TAG;

/// The high half of a payload, where the kind tags live.
const TAG_MASK: u64 = 0xFFFF_FFFF_0000_0000;

/// High 32 bits of a `u64` drag payload that identifies an asset-browser
/// tile — the low 32 bits hold the asset index in `EditorState::asset_entries`.
/// Every tile is a drag source; the drop sink (viewport / folder tree)
/// dispatches by the asset's type, so the tag is type-agnostic. Picked so
/// it can't collide with the `EntityId`-packed payload used by the
/// scene-tree reparent flow (entity generations are u32 and start at 1, so
/// any value with `0xKHPF` in the top half can only mean "asset tile").
const ASSET_DRAG_TAG: u64 = 0x4B48_5046_0000_0000; // "KHPF" in ASCII

/// Number of low bits of `EditorState::asset_epoch` stamped into a drag
/// payload. Eight is plenty: the stamp only has to survive one drag, and the
/// epoch would have to advance 256 times mid-gesture to alias.
const DRAG_EPOCH_BITS: u32 = 8;

const DRAG_INDEX_MASK: u64 = (1 << (32 - DRAG_EPOCH_BITS)) - 1;

/// Packs an asset-list index plus a stamp of the epoch it was read at.
///
/// The index alone is not enough: it points into `EditorState::asset_entries`,
/// which `hot_reload::pump` rebuilds whenever a file appears, disappears or is
/// renamed on disk. Without the stamp, a rescan mid-drag silently retargets the
/// drop at whatever now occupies that slot.
pub(crate) fn pack_asset_drag(index: u32, epoch: u64) -> u64 {
    let stamp = (epoch & ((1 << DRAG_EPOCH_BITS) - 1)) << (32 - DRAG_EPOCH_BITS);
    ASSET_DRAG_TAG | stamp | (index as u64 & DRAG_INDEX_MASK)
}

/// Whether `payload` carries the asset tag, regardless of how stale it is.
///
/// Kept separate from [`unpack_asset_drag`] because the two answer different
/// questions: this one classifies the payload, the other resolves it. Routing
/// classification through the epoch check would make a stale asset drag look
/// like a packed `EntityId` and get handled as a reparent.
pub(crate) fn is_asset_drag(payload: u64) -> bool {
    payload & TAG_MASK == ASSET_DRAG_TAG
}

/// Unpacks an asset drag payload, rejecting it when the asset list changed
/// since the drag started.
pub(crate) fn unpack_asset_drag(payload: u64, current_epoch: u64) -> Option<u32> {
    if !is_asset_drag(payload) {
        return None;
    }
    let stamp = (payload >> (32 - DRAG_EPOCH_BITS)) & ((1 << DRAG_EPOCH_BITS) - 1);
    if stamp != current_epoch & ((1 << DRAG_EPOCH_BITS) - 1) {
        log::debug!("Asset drop ignored: the asset list changed during the drag");
        return None;
    }
    Some((payload & DRAG_INDEX_MASK) as u32)
}

/// Packs an `EntityId` into a `u64` for drag-and-drop payloads. Layout:
/// high 32 = generation, low 32 = index. Also used by the asset
/// browser's drop handler to ingest entity drags from the scene tree
/// (drop = create prefab in the current folder).
pub(crate) fn pack_entity(e: khora_sdk::prelude::ecs::EntityId) -> u64 {
    ((e.generation as u64) << 32) | (e.index as u64)
}

/// Inverse of [`pack_entity`].
pub(crate) fn unpack_entity(payload: u64) -> khora_sdk::prelude::ecs::EntityId {
    khora_sdk::prelude::ecs::EntityId {
        index: payload as u32,
        generation: (payload >> 32) as u32,
    }
}

/// `true` when `payload` carries none of the drag tags: neither an asset
/// tile nor a dock tab. Lets receivers tell scene-tree entity payloads apart
/// from the others on the same `dnd_take_drop_payload` channel.
///
/// The check is conservative: any payload whose top 32 bits don't
/// match a known tag is assumed to be a packed `EntityId`. Entity
/// generations are tiny u32s (start at 1) so this can't collide with
/// the ASCII-encoded tags.
pub(crate) fn payload_is_entity(payload: u64) -> bool {
    !is_asset_drag(payload) && payload & TAG_MASK != DOCK_DRAG_TAG & TAG_MASK
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_sdk::prelude::ecs::EntityId;

    const INDICES: [u32; 4] = [0, 1, 42, DRAG_INDEX_MASK as u32];
    const EPOCHS: [u64; 5] = [0, 1, 255, 256, 1_000_003];

    #[test]
    fn an_asset_tile_payload_round_trips() {
        for index in INDICES {
            for epoch in EPOCHS {
                let payload = pack_asset_drag(index, epoch);
                assert!(is_asset_drag(payload), "{payload:#x}");
                assert_eq!(unpack_asset_drag(payload, epoch), Some(index));
            }
        }
    }

    /// A drag that outlives a rescan of the asset list resolves to nothing
    /// rather than to whatever now sits at its index.
    #[test]
    fn an_asset_tile_payload_from_an_older_asset_list_is_refused() {
        let payload = pack_asset_drag(3, 7);
        assert_eq!(unpack_asset_drag(payload, 8), None);
        assert!(is_asset_drag(payload), "still classified as an asset drag");
    }

    #[test]
    fn an_asset_tile_payload_is_never_taken_for_an_entity() {
        for index in INDICES {
            for epoch in EPOCHS {
                assert!(!payload_is_entity(pack_asset_drag(index, epoch)));
            }
        }
    }

    #[test]
    fn a_dock_tab_payload_is_never_taken_for_an_asset_tile() {
        assert!(!is_asset_drag(DOCK_DRAG_TAG));
        for epoch in EPOCHS {
            assert_eq!(unpack_asset_drag(DOCK_DRAG_TAG, epoch), None);
        }
        assert_ne!(
            ASSET_DRAG_TAG & 0xFFFF_FFFF_0000_0000,
            DOCK_DRAG_TAG & 0xFFFF_FFFF_0000_0000,
            "the two tags share their high half"
        );
    }

    /// `(index, generation)` pairs: the first entity, a recycled slot, and the
    /// extremes of the index range.
    const ENTITIES: [(u32, u32); 5] = [(0, 1), (1, 1), (7, 3), (12_345, 0xFFFF), (u32::MAX, 1)];

    #[test]
    fn an_entity_payload_round_trips() {
        for (index, generation) in ENTITIES {
            let entity = EntityId { index, generation };
            let payload = pack_entity(entity);
            assert!(payload_is_entity(payload), "{payload:#x}");
            assert_eq!(unpack_entity(payload), entity);
        }
    }

    #[test]
    fn an_entity_payload_is_never_taken_for_an_asset_tile() {
        for (index, generation) in ENTITIES {
            let payload = pack_entity(EntityId { index, generation });
            assert!(!is_asset_drag(payload), "{payload:#x}");
            assert_eq!(unpack_asset_drag(payload, 0), None);
        }
    }

    /// A dock tab dropped on the tree or the asset browser is not a reparent
    /// or a prefab request: its payload names no entity.
    #[test]
    fn a_dock_tab_payload_is_never_taken_for_an_entity() {
        assert!(
            !payload_is_entity(khora_tool_ui::widgets::DOCK_DRAG_TAG),
            "the dock-tab tag {:#x} is classified as a packed entity",
            khora_tool_ui::widgets::DOCK_DRAG_TAG
        );
    }
}
