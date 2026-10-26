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

//! A level: one set of parameters a behaviour can run with.
//!
//! A lane offers its levels to GORNA as menu rows. Each row is named by a
//! [`StrategyId`], unique within its family.

use super::cost::PerDevice;
use crate::agent::gorna::StrategyId;

/// How much a level changes what the family's readers see.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Impact {
    /// The reference result.
    None,
    /// A difference a player has to look for.
    Minor,
    /// A difference a player sees.
    Major,
}

/// One set of parameters a lane can run with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Level {
    /// The menu row's id, unique within the family.
    pub id: StrategyId,
    /// The level's name, for tools and logs.
    pub name: &'static str,
    /// How much the level changes the result.
    pub impact: Impact,
    /// What the level holds on each device, in bytes.
    pub memory: PerDevice<u64>,
}
