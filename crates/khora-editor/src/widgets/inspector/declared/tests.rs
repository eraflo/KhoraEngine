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

//! What a declared card says and writes, without drawing it.

use std::collections::HashMap;

use khora_sdk::editor_ui::{
    ComponentOverride, DeclaredCard, DeclaredCardKind, EntityIcon, InspectedPrefab, PropertyEdit,
    SceneNode,
};
use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::khora_core::math::Vec3;
use khora_sdk::khora_core::script::{ErgonType, ScriptValue};
use khora_sdk::khora_data::ecs::SemanticDomain;
use khora_sdk::prelude::ecs::EntityId;

use super::fields::{neutral, value_text};
use super::{kind_tag, provenance_line, restored, reverted, scene_names};

/// Edits as they print — `PropertyEdit` is compared by what it says.
fn said(edits: &[PropertyEdit]) -> Vec<String> {
    edits.iter().map(|edit| format!("{edit:?}")).collect()
}

fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 1,
    }
}

fn card(module: Option<&str>, kind: DeclaredCardKind) -> DeclaredCard {
    DeclaredCard {
        name: "Lantern".to_owned(),
        domain: SemanticDomain::Render,
        module: module.map(str::to_owned),
        kind,
        read_only: false,
        orphaned: module.is_none(),
        fields: Vec::new(),
    }
}

fn node(index: u32, name: &str, children: Vec<SceneNode>) -> SceneNode {
    SceneNode {
        entity: entity(index),
        name: name.to_owned(),
        icon: EntityIcon::Empty,
        children,
        tag_count: 0,
    }
}

/// An instance member whose `Lantern` overrides `glow` against a prefab
/// lighting it at `glow` 2, `label` "post"; `added` when the instance added
/// it.
fn lantern_prefab(added: bool) -> InspectedPrefab {
    InspectedPrefab {
        root: entity(1),
        prefab: AssetUUID::new(),
        prefab_path: None,
        is_root: true,
        components: vec![ComponentOverride {
            type_name: "Lantern".to_owned(),
            added,
            fields: if added {
                Vec::new()
            } else {
                vec![vec!["glow".to_owned()]]
            },
        }],
        removed: Vec::new(),
        prefab_json: Vec::new(),
        prefab_declared: if added {
            Vec::new()
        } else {
            vec![(
                "Lantern".to_owned(),
                vec![
                    ("glow".to_owned(), ScriptValue::Float(2.0)),
                    ("label".to_owned(), ScriptValue::Str("post".to_owned())),
                ],
            )]
        },
    }
}

#[test]
fn a_duration_reads_in_seconds_and_an_angle_in_degrees() {
    let names = HashMap::new();
    assert_eq!(
        value_text(&ScriptValue::Float(1.5), &ErgonType::Duration, &names),
        "1.5 s"
    );
    assert_eq!(
        value_text(
            &ScriptValue::Float(std::f32::consts::FRAC_PI_2),
            &ErgonType::Angle,
            &names
        ),
        "90 deg"
    );
    assert_eq!(
        value_text(&ScriptValue::Float(2.0), &ErgonType::Float, &names),
        "2"
    );
}

#[test]
fn an_optional_reads_none_or_its_value() {
    let names = HashMap::new();
    let optional = ErgonType::Optional(Box::new(ErgonType::Duration));
    assert_eq!(value_text(&ScriptValue::Null, &optional, &names), "none");
    assert_eq!(
        value_text(&ScriptValue::Float(3.0), &optional, &names),
        "3 s"
    );
}

#[test]
fn an_entity_reads_by_its_name_or_its_index() {
    let names = HashMap::from([(entity(4), "Gate".to_owned())]);
    assert_eq!(
        value_text(&ScriptValue::Entity(entity(4)), &ErgonType::Entity, &names),
        "Gate"
    );
    assert_eq!(
        value_text(&ScriptValue::Entity(entity(9)), &ErgonType::Entity, &names),
        "Entity 9"
    );
}

#[test]
fn a_new_value_is_its_types_neutral_one() {
    assert_eq!(neutral(&ErgonType::Int), ScriptValue::Int(0));
    assert_eq!(neutral(&ErgonType::Angle), ScriptValue::Float(0.0));
    assert_eq!(
        neutral(&ErgonType::Engine("Vec3")),
        ScriptValue::Vec3(Vec3::ZERO)
    );
    assert_eq!(
        neutral(&ErgonType::Array(Box::new(ErgonType::Int))),
        ScriptValue::Array(Vec::new())
    );
    assert_eq!(
        neutral(&ErgonType::Struct {
            name: "Loot".to_owned(),
            fields: Some(vec![
                ("gold".to_owned(), ErgonType::Int),
                ("owner".to_owned(), ErgonType::Entity),
            ]),
        }),
        ScriptValue::Struct(vec![
            ("gold".to_owned(), ScriptValue::Int(0)),
            ("owner".to_owned(), ScriptValue::Null),
        ])
    );
}

#[test]
fn a_card_is_tagged_by_the_part_its_component_plays() {
    assert_eq!(kind_tag(DeclaredCardKind::Component), "script");
    assert_eq!(kind_tag(DeclaredCardKind::Behavior), "behavior");
    assert_eq!(kind_tag(DeclaredCardKind::BehaviorState), "state");
    assert_eq!(kind_tag(DeclaredCardKind::System), "system");
}

#[test]
fn a_card_names_its_module_or_says_none_declares_it() {
    assert_eq!(
        provenance_line(&card(
            Some("props/lantern.erg"),
            DeclaredCardKind::Component
        )),
        "props/lantern.erg"
    );
    assert!(
        provenance_line(&card(None, DeclaredCardKind::Component)).contains("No script declares"),
        "an orphan says so"
    );
    assert!(
        provenance_line(&card(Some("ai/guard.erg"), DeclaredCardKind::BehaviorState))
            .contains("engine"),
        "a state companion is the engine's"
    );
}

#[test]
fn every_entity_of_the_scene_tree_is_named() {
    let roots = vec![node(1, "Post", vec![node(2, "Wick", Vec::new())])];
    let names = scene_names(&roots);
    assert_eq!(names.get(&entity(1)).map(String::as_str), Some("Post"));
    assert_eq!(names.get(&entity(2)).map(String::as_str), Some("Wick"));
}

#[test]
fn reverting_a_card_sets_back_only_its_overridden_fields() {
    let edits = reverted(entity(1), "Lantern", &lantern_prefab(false));
    assert_eq!(
        said(&edits),
        said(&[PropertyEdit::SetDeclaredField {
            entity: entity(1),
            component: "Lantern".to_owned(),
            field: "glow".to_owned(),
            value: ScriptValue::Float(2.0),
        }])
    );
}

#[test]
fn reverting_an_added_card_takes_it_off() {
    let edits = reverted(entity(1), "Lantern", &lantern_prefab(true));
    assert_eq!(
        said(&edits),
        said(&[PropertyEdit::RemoveDeclared {
            entity: entity(1),
            component: "Lantern".to_owned(),
        }])
    );
}

#[test]
fn restoring_a_removed_card_adds_it_with_the_prefabs_fields() {
    let edits = said(&restored(entity(1), "Lantern", &lantern_prefab(false)));
    assert_eq!(edits.len(), 3, "{edits:?}");
    assert_eq!(
        edits[0],
        said(&[PropertyEdit::AddDeclared {
            entity: entity(1),
            component: "Lantern".to_owned(),
        }])[0]
    );
    let label = said(&[PropertyEdit::SetDeclaredField {
        entity: entity(1),
        component: "Lantern".to_owned(),
        field: "label".to_owned(),
        value: ScriptValue::Str("post".to_owned()),
    }]);
    assert!(edits.contains(&label[0]), "{edits:?}");
}
