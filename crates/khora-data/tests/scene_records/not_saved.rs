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

//! A save holding a component the engine rebuilds rather than reads.

use khora_data::ecs::{Name, Parent, Transform};
use khora_data::scene::record::ReportKind;

use super::*;

/// `Children` is derived from `Parent`. A save that holds it anyway — written
/// by a tool, or before the component stopped being saved — loads: the
/// recorded copy is skipped and reported, and the list is rebuilt from the
/// children's `Parent`, whatever the copy said.
#[test]
fn a_recorded_derived_component_is_skipped_reported_and_rebuilt() {
    let mut src = World::new();
    let parent = src.spawn((Transform::identity(), Name::new("P")));
    let child = src.spawn((Transform::identity(), Name::new("C")));
    assert!(src.set_parent(child, Some(parent)));
    let parent_id = src.mark_authored(parent).expect("authored");
    let child_id = src.mark_authored(child).expect("authored");

    let mut record = capture_world(&src).expect("captures");
    let (page, _, _) = slot_of(&record, parent_id, "Name").expect("P has a row");
    // A list naming nothing — out of step with the child's `Parent`.
    add_column(&mut record, page, "Children", Record::Seq(Vec::new()));

    let mut dst = World::new();
    let applied = apply(&mut dst, &record, Identity::Keep).expect("loads");

    let (p, c) = (
        dst.entity_with_id(parent_id).expect("P loaded"),
        dst.entity_with_id(child_id).expect("C loaded"),
    );
    assert_eq!(dst.get::<Parent>(c).map(|x| x.0), Some(p));
    assert_eq!(dst.get::<Children>(p).map(|x| x.0.clone()), Some(vec![c]));
    assert!(
        applied
            .report
            .entries
            .iter()
            .any(|entry| entry.kind == ReportKind::NotSaved
                && entry.component.as_deref() == Some("Children")),
        "the skipped copy is reported: {:?}",
        applied.report
    );
}
