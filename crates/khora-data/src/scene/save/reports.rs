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

//! What a load reports of what it adapted on the way: only what it brings in.

use std::collections::{HashMap, HashSet};

use khora_core::ecs::PersistentId;

use super::components_of;
use crate::scene::record::{ReportEntry, ReportKind};
use crate::scene::scene_record::SceneRecord;

/// The entries of `entries` about what `record` — what is loaded — holds,
/// each once, in order.
///
/// A load assembles its record in steps — a prefab expanded, a scene read as
/// this build reads it, a save merged on top — and each step reports what it
/// adapted. What a later step then discarded — an entity the save destroyed,
/// a component it removed, a member an instance deleted — is not loaded, so
/// what was adapted in it is not news. An entity the scene lost is the
/// exception: [`ReportKind::RemovedFromScene`] says that its changes went. A
/// retired component is never loaded — that is what [`ReportKind::Retired`]
/// says — so it is reported where its entity loads.
pub fn about_what_loads(
    record: &SceneRecord,
    entries: impl IntoIterator<Item = ReportEntry>,
) -> Vec<ReportEntry> {
    let held: HashMap<PersistentId, HashSet<&str>> = components_of(record)
        .into_iter()
        .map(|(id, components)| (id, components.into_iter().map(|(name, _)| name).collect()))
        .collect();
    let listed: HashSet<PersistentId> = record.entities.iter().copied().collect();
    let loads = |entry: &ReportEntry| {
        if entry.kind == ReportKind::RemovedFromScene {
            return true;
        }
        let Some(entity) = entry.entity else {
            return true;
        };
        let entity_loads = listed.contains(&entity) || held.contains_key(&entity);
        if entry.kind == ReportKind::Retired {
            return entity_loads;
        }
        match &entry.component {
            None => entity_loads,
            Some(component) => held
                .get(&entity)
                .is_some_and(|components| components.contains(component.as_str())),
        }
    };
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .filter(|entry| loads(entry) && seen.insert(entry.clone()))
        .collect()
}
