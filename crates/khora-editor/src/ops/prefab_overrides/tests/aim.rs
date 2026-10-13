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

//! `Aim`, an authored component of these tests: a reference to another
//! entity beside a plain value — what a turret's aim holds.
//!
//! Its registration is written out by hand, as `#[derive(Component)]` writes
//! it: the derive spells its output against the deriving crate's
//! `crate::ecs` / `crate::scene`, and the editor has neither.

use std::sync::OnceLock;

use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_data::ecs::{
    Component, ComponentDomainRegistration, ComponentProvenance, SemanticDomain, World,
};
use khora_sdk::khora_data::scene::{
    positional, record, schema, ComponentRegistration, ComponentShape, FieldSchema, Staged,
    StagedValue,
};
use serde::{Deserialize, Serialize};

/// Where a turret aims, and how far it reaches.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(super) struct Aim {
    /// The entity aimed at.
    pub(super) target: EntityId,
    /// How far the turret reaches.
    pub(super) range: f32,
}

impl Component for Aim {}

/// `Aim`'s schema, traced once.
static SCHEMA: OnceLock<(u64, bool)> = OnceLock::new();

inventory::submit! {
    ComponentDomainRegistration {
        register: |world: &mut World| {
            world.register_component::<Aim>(SemanticDomain::Spatial);
        }
    }
}

inventory::submit! {
    ComponentRegistration {
        type_id: std::any::TypeId::of::<Aim>(),
        type_name: "Aim",
        shape: ComponentShape::Fields(&[
            FieldSchema {
                name: "target",
                ty: "EntityId",
            },
            FieldSchema {
                name: "range",
                ty: "f32",
            },
        ]),
        provenance: ComponentProvenance::Authored,
        formerly: &[],
        resumable: false,
        column_to_record: |column, row, references| {
            record::to_record(&<Aim as Component>::clone_from_column(column, row), references)
        },
        stage: |saved, references| {
            let (aim, report) = record::resolve::<Aim>(saved, references)?;
            Ok(Staged {
                component: Box::new(StagedValue(aim)),
                report,
            })
        },
        schema: || schema::traced_once::<Aim>(&SCHEMA, "Aim").0,
        schema_complete: || schema::traced_once::<Aim>(&SCHEMA, "Aim").1,
        column_to_snapshot: |column, row, out, references| {
            positional::to_positional(
                &<Aim as Component>::clone_from_column(column, row),
                out,
                references,
            )
        },
        stage_snapshot: |bytes, references| {
            let aim = positional::from_positional::<Aim>(bytes, references)?;
            Ok(Staged {
                component: Box::new(StagedValue(aim)),
                report: Vec::new(),
            })
        },
        create_default: |_, _| Err("an aim has no default target".to_owned()),
        to_json: |world, entity| {
            world
                .clone_component::<Aim>(entity)
                .and_then(|aim| serde_json::to_value(aim).ok())
        },
        from_json: |world, entity, value| {
            let aim: Aim = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            if !world.set_component(entity, aim) {
                world
                    .add_component(entity, aim)
                    .map_err(|e| format!("{e:?}"))?;
            }
            Ok(())
        },
        remove: |world, entity| {
            world
                .remove_component::<Aim>(entity)
                .map(|_| ())
                .map_err(|e| format!("{e:?}"))
        },
        script_type: <Aim as Component>::script_type,
        script_default: || None,
    }
}
