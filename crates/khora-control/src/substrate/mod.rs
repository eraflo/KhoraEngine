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

//! Substrate Pass — orchestrates the Data layer's self-maintenance work.
//!
//! Per CLAD's enriched doctrine, the Substrate Pass runs *around* the
//! command path (`Control → Agent → Lane → Data`), not inside it. It is
//! invoked by the Scheduler at well-defined points in the tick:
//!
//! - [`khora_data::ecs::TickPhase::PreSimulation`] before any agent simulates,
//! - [`khora_data::ecs::TickPhase::PostSimulation`] before extraction,
//! - [`khora_data::ecs::TickPhase::PreExtract`] right before `Flow`s project,
//! - [`khora_data::ecs::TickPhase::Maintenance`] at the end of the tick.
//!
//! The pass discovers `DataSystemRegistration` entries via [`inventory`],
//! orders them by `runs_after` (topological sort) with `order_hint` as a
//! tie-breaker, and invokes them sequentially.

pub mod flow_runner;

pub use flow_runner::run_flows;

mod data_system_runner;

pub use data_system_runner::run_data_systems;
