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

use super::*;

/// **The bug the generation exists to stop.** A backend that recycles slots
/// hands the same index to a new occupant; a handle that carried only the
/// index would address that new occupant instead of failing, and would do
/// so silently — a stale handle moving somebody else's body.
#[test]
fn two_occupants_of_one_slot_are_different_handles() {
    let first = ColliderHandle(
        SlotId {
            index: 7,
            generation: 0,
        }
        .pack(),
    );
    let recycled = ColliderHandle(
        SlotId {
            index: 7,
            generation: 1,
        }
        .pack(),
    );

    assert_ne!(first, recycled, "same slot, different occupant");
}

#[test]
fn a_handle_round_trips_its_slot() {
    let slot = SlotId {
        index: u32::MAX,
        generation: 3,
    };

    assert_eq!(RigidBodyHandle(slot.pack()).slot(), slot);
    assert_eq!(ColliderHandle(slot.pack()).slot(), slot);
}

/// The index occupies the low half, so a generation of zero packs to the
/// index itself — which is what every handle written before this change
/// held, and why they keep meaning what they meant.
#[test]
fn a_first_generation_handle_is_its_index() {
    let slot = SlotId {
        index: 42,
        generation: 0,
    };

    assert_eq!(slot.pack(), 42);
}
