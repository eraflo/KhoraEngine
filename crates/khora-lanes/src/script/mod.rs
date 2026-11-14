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

//! The script family: its lane, which runs the behaviours of the frame's
//! entities within their fuel, the scripting runtime it locks, and its registration.

mod advance;
mod arrival;
mod attached;
mod carry;
mod delivery;
mod departure;
mod family;
mod fixed_step;
mod frame;
mod fuel;
mod fuel_demand;
mod hooks;
pub mod intake;
mod invocation;
mod lane;
mod levels;
mod mail;
mod outcome;
mod park;
pub mod persistence;
mod problems;
mod progress;
mod rate;
mod registration;
mod reload;
pub mod report;
pub mod resumption;
pub mod runtime;
mod shares;
mod state_update;
mod turn;
pub mod watchdog;

pub use family::{ScriptFamily, ScriptOutputs};
pub use frame::run_behaviors;
pub use intake::take_in;
pub use lane::*;
pub use report::{Resumed, ScriptRunReport, Slow};
pub use runtime::{Body, FuelDemand, Instance, Pending, ReloadReport, ScriptRuntime};
pub use watchdog::{WATCHDOG_FAULT, WATCHDOG_WARN};

#[cfg(test)]
mod tests;
