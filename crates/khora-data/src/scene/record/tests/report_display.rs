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

//! How a report entry reads in a log.

use khora_core::ecs::PersistentId;

use super::*;

/// An entry names what happened, where, and on what — so a log line alone
/// tells an author which value to look at.
#[test]
fn an_entry_reads_as_a_sentence_naming_its_place() {
    let entry = ReportEntry {
        entity: Some(PersistentId::authored(0x2a)),
        component: Some("Transform".to_owned()),
        path: "scale".to_owned(),
        kind: ReportKind::Renamed {
            from: "size".to_owned(),
        },
    };
    let line = entry.to_string();
    assert!(line.contains("`scale`"), "{line}");
    assert!(line.contains("`size`"), "{line}");
    assert!(line.contains("Transform"), "{line}");
    assert!(
        line.contains(&format!("{:#x}", PersistentId::authored(0x2a).to_bits())),
        "{line}"
    );
}

/// Without a component or an entity, the entry says only what happened.
#[test]
fn an_entry_without_a_place_names_no_owner() {
    let entry = ReportEntry {
        entity: None,
        component: None,
        path: "x".to_owned(),
        kind: ReportKind::Widened,
    };
    assert_eq!(
        entry.to_string(),
        "`x` was read at another number type, without loss"
    );
}
