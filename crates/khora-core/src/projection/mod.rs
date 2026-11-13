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

//! What the readers of a projection want projected.
//!
//! A `Flow` (khora-data) projects the World into a View; some Flows project only what
//! somebody asked for — the script flow, only the components a program
//! reads. The ones asking (a DataSystem, the hot-reload pump, a tool) cannot
//! reach the Flow, and the Flow alone reads the World. An [`Interest`] is
//! where they meet: each owner announces the items it wants, the Flow
//! projects their union.
//!
//! The rule of time: what is announced before a Flow runs is in that frame's
//! projection; what is announced after is in the next one. A Flow whose
//! projection follows a demand folds its [`Interest::generation`] into its
//! cache key.

mod demand;
mod interest;

pub use demand::Demand;
pub use interest::Interest;

#[cfg(test)]
mod tests;
