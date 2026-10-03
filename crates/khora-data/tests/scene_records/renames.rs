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

//! Renamed components and fields, read through `#[component(formerly)]`.

use khora_data::ecs::World;
use khora_data::scene::record::{Record, ReportKind};
use khora_macros::Component;

use super::*;

/// A component whose type was renamed: saves wrote it as `LegacyBeacon`.
#[derive(Debug, Clone, PartialEq, Default, Component)]
#[component(domain = Spatial, formerly = "LegacyBeacon")]
pub struct Beacon {
    pub range: f32,
}

/// A component one of whose fields was renamed: saves wrote `reserve` as
/// `points`.
#[derive(Debug, Clone, PartialEq, Default, Component)]
#[component(domain = Spatial)]
pub struct Stamina {
    #[component(formerly = "points")]
    pub reserve: f32,
    pub regen: f32,
}

/// The record `world` captures, as written by today's code.
fn captured(world: &World) -> SceneRecord {
    capture_world(world).expect("captures")
}

/// A save is older than a rename. The type's former name, declared on the
/// type, still finds it; the value loads; the report says what was renamed.
#[test]
fn a_renamed_component_type_loads_through_formerly() {
    let mut src = World::new();
    let entity = src.spawn(Beacon { range: 42.5 });
    src.mark_authored(entity).expect("authored");
    let id = src.persistent_id(entity).expect("id");

    let mut record = captured(&src);
    assert_eq!(
        rename_component(&mut record, "Beacon", "LegacyBeacon"),
        1,
        "the beacon is recorded in one page"
    );

    for (name, encoding) in every_encoding() {
        let back = through(&record, name, encoding);
        let mut dst = World::new();
        let applied = apply(&mut dst, &back, Identity::Keep)
            .unwrap_or_else(|e| panic!("{name}: a former name is no error: {e}"));
        let twin = dst.entity_with_id(id).expect("loaded");
        assert_eq!(
            dst.get::<Beacon>(twin),
            Some(&Beacon { range: 42.5 }),
            "{name}"
        );

        let renamed = applied
            .report
            .entries
            .iter()
            .find(|entry| {
                entry.kind
                    == ReportKind::Renamed {
                        from: "LegacyBeacon".into(),
                    }
            })
            .unwrap_or_else(|| panic!("{name}: no rename reported: {:?}", applied.report));
        assert_eq!(renamed.entity, Some(id), "{name}");
        assert_eq!(renamed.component.as_deref(), Some("Beacon"), "{name}");
    }
}

/// A field renamed in code keeps reading from saves that used its former
/// name, declared on the field; the other fields are untouched and the report
/// names the rename.
#[test]
fn a_renamed_field_loads_through_formerly() {
    let mut src = World::new();
    let entity = src.spawn(Stamina {
        reserve: 7.5,
        regen: 0.25,
    });
    src.mark_authored(entity).expect("authored");
    let id = src.persistent_id(entity).expect("id");

    let mut record = captured(&src);
    let value = value_mut(&mut record, id, "Stamina");
    let Record::Struct { fields, .. } = value else {
        panic!("a struct component is a struct record: {value:?}");
    };
    let field = fields
        .iter_mut()
        .find(|(name, _)| name == "reserve")
        .expect("the field is recorded by name");
    field.0 = "points".into();

    for (name, encoding) in every_encoding() {
        let back = through(&record, name, encoding);
        let mut dst = World::new();
        let applied = apply(&mut dst, &back, Identity::Keep)
            .unwrap_or_else(|e| panic!("{name}: a former field name is no error: {e}"));
        let twin = dst.entity_with_id(id).expect("loaded");
        assert_eq!(
            dst.get::<Stamina>(twin),
            Some(&Stamina {
                reserve: 7.5,
                regen: 0.25
            }),
            "{name}"
        );

        let renamed = applied
            .report
            .entries
            .iter()
            .find(|entry| {
                entry.kind
                    == ReportKind::Renamed {
                        from: "points".into(),
                    }
            })
            .unwrap_or_else(|| panic!("{name}: no rename reported: {:?}", applied.report));
        assert_eq!(renamed.entity, Some(id), "{name}");
        assert_eq!(renamed.component.as_deref(), Some("Stamina"), "{name}");
        assert!(renamed.path.contains("reserve"), "{name}: {renamed:?}");
    }
}
