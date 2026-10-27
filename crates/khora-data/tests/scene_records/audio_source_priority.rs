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

//! An audio source's priority is authored: a scene keeps it, and a scene
//! saved before sources had one loads them at the default priority.

use khora_data::ecs::{AudioSource, World, DEFAULT_PRIORITY};
use khora_data::scene::record::{Record, ReportKind};

use super::*;

/// A world holding `source` on one authored entity, and that entity's
/// identity.
fn world_with(source: AudioSource) -> (World, PersistentId) {
    let mut world = World::new();
    let entity = world.spawn(source);
    world.mark_authored(entity).expect("authored");
    let id = world.persistent_id(entity).expect("id");
    (world, id)
}

/// A source's priority comes back from every encoding as it was saved.
#[test]
fn an_audio_source_priority_round_trips_through_every_encoding() {
    let (src, id) = world_with(AudioSource {
        priority: 200,
        ..AudioSource::default()
    });

    for (name, encoding) in every_encoding() {
        let record = capture_world(&src).expect("captures");
        let back = through(&record, name, encoding);
        let mut dst = World::new();
        let applied = apply(&mut dst, &back, Identity::Keep)
            .unwrap_or_else(|e| panic!("{name}: the load failed: {e}"));
        assert!(applied.report.is_clean(), "{name}: {:?}", applied.report);
        let twin = dst.entity_with_id(id).expect("loaded");
        assert_eq!(
            dst.get::<AudioSource>(twin).map(|s| s.priority),
            Some(200),
            "{name}"
        );
    }
}

/// A source saved before sources had a priority loads at the default one, and
/// the report says `priority` took its default.
#[test]
fn a_source_saved_without_a_priority_loads_at_the_default_and_says_so() {
    let (src, id) = world_with(AudioSource {
        priority: 7,
        ..AudioSource::default()
    });
    let mut record = capture_world(&src).expect("captures");
    match value_mut(&mut record, id, "AudioSource") {
        Record::Struct { fields, .. } => fields.retain(|(name, _)| name != "priority"),
        other => panic!("a struct component is a struct record: {other:?}"),
    }

    for (name, encoding) in every_encoding() {
        let back = through(&record, name, encoding);
        let mut dst = World::new();
        let applied = apply(&mut dst, &back, Identity::Keep)
            .unwrap_or_else(|e| panic!("{name}: a field the save predates is no error: {e}"));
        let twin = dst.entity_with_id(id).expect("loaded");
        assert_eq!(
            dst.get::<AudioSource>(twin).map(|s| s.priority),
            Some(DEFAULT_PRIORITY),
            "{name}"
        );
        assert!(
            applied
                .report
                .entries
                .iter()
                .any(|entry| entry.kind == ReportKind::Defaulted
                    && entry.entity == Some(id)
                    && entry.component.as_deref() == Some("AudioSource")
                    && entry.path == "priority"),
            "{name}: `AudioSource.priority` is not reported defaulted: {:?}",
            applied.report
        );
    }
}
