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

//! Acts as the **[A]gent** for the physics subsystem, fulfilling the role of an ISA.
//!
//! This module provides the high-level, tactical logic for the physics simulation.
//! It is responsible for configuring and dispatching the various `physics_lanes`
//! (e.g., broadphase, solver, integration) that perform the actual calculations.
//!
//! As an **Intelligent Subsystem Agent (ISA)**, its responsibilities will include:
//! - Reporting the simulation's performance and resource cost to the DCC.
//! - Managing multiple strategies, such as switching between a high-precision but
//!   slow physics solver and a faster, less precise one.
//! - Adapting simulation parameters (e.g., timestep frequency) based on the budget
//!   allocated by GORNA to maintain stability or performance.

mod agent;

pub use agent::*;
