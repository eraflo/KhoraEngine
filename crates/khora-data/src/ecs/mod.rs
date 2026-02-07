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

//! Implements Khora's custom **Chunked Relational Page ECS (CRPECS)**.
//!
//! This module contains the complete implementation of the engine's Entity-Component-System,
//! which is the heart of the **[D]ata** layer in the CLAD architecture.
//!
//! The CRPECS is designed from the ground up to enable the **Adaptive Game Data Flows (AGDF)**
//! concept from the SAA philosophy. Its key architectural feature is the dissociation
//! of an entity's identity from the physical storage of its component data, which allows
//! for extremely fast structural changes (adding/removing components).
//!
//! The primary entry point for interacting with the ECS is the [`World`] struct.

mod bitset;
mod bundle;
mod component;
mod components;
mod entity;
mod entity_store;
mod page;
mod planner;
mod query;
mod query_plan;
mod registry;
mod serialization;
mod storage;
mod world;

pub use bitset::DomainBitset;
pub use bundle::ComponentBundle;
pub use component::Component;
pub use components::*;
pub use entity::*;
pub use page::*;
pub use query::*;
pub use query_plan::{QueryMode, QueryPlan};
pub use registry::*;
pub use world::*;

#[cfg(test)]
mod tests;
