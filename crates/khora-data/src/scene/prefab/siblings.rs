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

//! The order of an instance's siblings: where it sits among a parent's
//! children, put back after an expansion.

use std::collections::HashMap;

use khora_core::ecs::PersistentId;

use super::super::entity_refs::parent_in;
use super::super::record::Record;

/// Each parent's children, in the order `entities` lists them, by the
/// `Parent` their rows hold.
pub(super) fn children_of(
    entities: &[PersistentId],
    rows: &HashMap<PersistentId, Vec<(String, Record)>>,
) -> HashMap<PersistentId, Vec<PersistentId>> {
    let mut children: HashMap<PersistentId, Vec<PersistentId>> = HashMap::new();
    for id in entities {
        if let Some(parent) = rows.get(id).and_then(|components| parent_in(components)) {
            children.entry(parent).or_default().push(*id);
        }
    }
    children
}

/// Puts each parent's children named in `order` back in the order it names
/// them, within the places they already hold in `entities` — a hierarchy
/// rebuilds a parent's children in the order its record lists them, so that
/// is all a sibling order needs.
pub(super) fn restore_sibling_order(
    entities: &mut [PersistentId],
    rows: &HashMap<PersistentId, Vec<(String, Record)>>,
    order: &[PersistentId],
) {
    let mut by_parent: HashMap<PersistentId, Vec<PersistentId>> = HashMap::new();
    for id in order {
        if let Some(parent) = rows.get(id).and_then(|components| parent_in(components)) {
            by_parent.entry(parent).or_default().push(*id);
        }
    }
    let mut at: HashMap<PersistentId, usize> = entities
        .iter()
        .enumerate()
        .map(|(place, id)| (*id, place))
        .collect();
    for siblings in by_parent.into_values() {
        let present: Vec<PersistentId> = siblings
            .into_iter()
            .filter(|id| at.contains_key(id))
            .collect();
        let mut places: Vec<usize> = present.iter().map(|id| at[id]).collect();
        places.sort_unstable();
        for (place, id) in places.into_iter().zip(present) {
            entities[place] = id;
            at.insert(id, place);
        }
    }
}
