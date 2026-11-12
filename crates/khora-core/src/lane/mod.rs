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

//! # Lanes
//!
//! A **lane** is one way of doing a family's work: one behaviour among the
//! alternatives its agent chooses from (`SimpleUnlit`, `LitForward`,
//! `ForwardPlus` for rendering). A lane implements [`Lane`]: the family it
//! belongs to, the levels it offers, what a level costs, and how it records
//! a frame's work.
//!
//! A lane never reaches the `World`. It reads what it declares through its
//! typed inputs (`#[derive(LaneInputs)]`, see [`io`]) — Views from the
//! [`LaneBus`], resources, locks — and writes its typed outputs
//! (`#[derive(LaneOutputs)]`) to the [`OutputDeck`]. The scheduler runs it
//! as a layer at its family's place; its agent only chooses which lane runs,
//! at which level.

pub mod contract;
pub mod plumbing;

pub use contract::{compile_check, cost, erased, family, guard, io, level};
pub use plumbing::{bus, context_keys, deck, host_views, lease, reserve, staging};

pub use bus::LaneBus;
pub use context_keys::*;
pub use contract::{Lane, LaneError, LaneReport};
pub use deck::OutputDeck;
pub use host_views::HostViews;
pub use lease::{Lease, Leases};
pub use reserve::{Reserve, Staged};
pub use staging::Staging;
