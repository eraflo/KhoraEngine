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

//! The drag payload that carries an asset tile to the scene tree or the viewport.

/// High 32 bits of a `u64` drag payload that identifies an asset-browser
/// tile — the low 32 bits hold the asset index in `EditorState::asset_entries`.
/// Every tile is a drag source; the drop sink (viewport / folder tree)
/// dispatches by the asset's type, so the tag is type-agnostic. Picked so
/// it can't collide with the `EntityId`-packed payload used by the
/// scene-tree reparent flow (entity generations are u32 and start at 1, so
/// any value with `0xKHPF` in the top half can only mean "asset tile").
pub(crate) const ASSET_DRAG_TAG: u64 = 0x4B48_5046_0000_0000; // "KHPF" in ASCII

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
    payload & 0xFFFF_FFFF_0000_0000 == ASSET_DRAG_TAG
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
