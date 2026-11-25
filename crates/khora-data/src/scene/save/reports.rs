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

/// Returns the entries of `entries` about what `record` loads, each once, in order.
///
/// An adaptation in something a later step discarded (an entity the save
/// destroyed, a component it removed, a member an instance deleted) is not news
/// and is dropped. [`ReportKind::RemovedFromScene`] is always kept, and
/// [`ReportKind::Retired`] wherever its entity loads, as is a whole component
/// [`ReportKind::Dropped`] (a value only a session holds, read from a file).
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
        // A component skipped whole is in no record: it is reported wherever
        // its entity loads.
        if entry.kind == ReportKind::Retired || refused_whole(entry) {
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

/// Returns whether `entry` says a whole component was refused: `Dropped`, at no path.
fn refused_whole(entry: &ReportEntry) -> bool {
    entry.kind == ReportKind::Dropped && entry.path.is_empty() && entry.component.is_some()
}

#[cfg(test)]
mod tests {
    use khora_core::ecs::PersistentId;
    use khora_core::math::Vec3;

    use super::about_what_loads;
    use crate::ecs::{Transform, World};
    use crate::scene::capture_world;
    use crate::scene::record::{ReportEntry, ReportKind};

    fn entry(
        entity: Option<PersistentId>,
        component: Option<&str>,
        kind: ReportKind,
    ) -> ReportEntry {
        ReportEntry {
            entity,
            component: component.map(str::to_owned),
            path: String::new(),
            kind,
        }
    }

    #[test]
    fn only_entries_about_what_the_record_loads_are_kept_each_once() {
        let mut src = World::new();
        let spawned = src.spawn(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
        src.mark_authored(spawned).expect("authored");
        let record = capture_world(&src).expect("captures");
        let loaded = record.entities[0];
        let gone = PersistentId::from_bits(loaded.to_bits() ^ 1);

        let held = entry(Some(loaded), Some("Transform"), ReportKind::Defaulted);
        let not_held = entry(Some(loaded), Some("Camera"), ReportKind::Defaulted);
        let removed = entry(Some(gone), Some("Transform"), ReportKind::RemovedFromScene);
        let retired_here = entry(Some(loaded), Some("Old"), ReportKind::Retired);
        let retired_elsewhere = entry(Some(gone), Some("Old"), ReportKind::Retired);
        let about_nothing = entry(None, None, ReportKind::Widened);

        let kept = about_what_loads(
            &record,
            [
                held.clone(),
                not_held,
                removed.clone(),
                retired_here.clone(),
                retired_elsewhere,
                about_nothing.clone(),
                held.clone(),
            ],
        );

        assert_eq!(kept, vec![held, removed, retired_here, about_nothing]);
    }
}
