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

//! CRPECS, the Chunked Relational Page ECS: the engine's entity-component storage.
//!
//! The entry point is the [`World`]. See the book: concepts/ecs.md, "Why a custom ECS".

mod any_vec;
mod bitset;
mod bundle;
mod check;
mod column_ops;
mod column_ticks;
pub mod component;
mod component_domain_registration;
mod component_key;
mod component_registry;
mod components;
mod entity;
mod entity_store;
pub mod layout;
pub mod maintenance;
mod mut_guard;
mod observer;
pub(crate) mod packed;
mod page;
mod page_index;
mod planner;
mod query;
mod query_plan;
mod role;
mod script_role;
pub mod soa;
mod storage;
pub mod system;
pub mod systems;
mod tick;
mod world;
mod world_ext;

pub use bitset::DomainBitset;
pub use bundle::ComponentBundle;
pub use check::{check_all, FieldProblem};
pub use column_ops::{ColumnOps, ColumnSnapshot, FieldWriteError};
pub use component::Component;
pub use component_key::{ColumnMap, ComponentKey, KeyHasher};
pub use component_registry::*;
pub use components::*;
pub use entity::*;
pub use maintenance::EcsMaintenance;
pub use mut_guard::Mut;
pub use packed::{FieldError, FieldKind, LayoutError, PackedField, PackedLayout};
pub use page::*;
pub use query::*;
pub use query_plan::{QueryMode, QueryPlan};
pub use role::{check_roles, registered_roles, Role, ServesUnknownFamily};
pub use script_role::ScriptRole;
pub use soa::{FieldSoaColumn, SoaLayout};
pub use system::{DataSystemRegistration, SystemContext, TickPhase};
pub use tick::{ColumnTicks, Observer, Tick, QUIET_ROW_TICKS};
pub use world::*;
pub use world_ext::{entities_with_all_tags, entities_with_any_tag, entities_with_tag};

#[cfg(test)]
mod tests;
