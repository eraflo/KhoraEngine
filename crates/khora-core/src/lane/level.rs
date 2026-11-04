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

use serde::{Deserialize, Serialize};

use super::cost::PerDevice;
use crate::agent::gorna::StrategyId;

/// How much a level changes what the family's readers see.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Impact {
    /// The reference result.
    None,
    /// A difference a player has to look for.
    Minor,
    /// A difference a player sees.
    Major,
}

impl Impact {
    /// What a result of this impact is worth, from `0.0` to `1.0`: the
    /// reference is whole, a difference to look for costs a little, one a
    /// player sees costs more.
    pub fn quality(self) -> f32 {
        match self {
            Impact::None => 1.0,
            Impact::Minor => 0.7,
            Impact::Major => 0.4,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::Impact;

    /// **A row's quality comes from its impact**: the reference result is
    /// worth 1.0, a difference a player has to look for 0.7, one a player sees
    /// 0.4.
    #[test]
    fn impact_maps_to_quality() {
        assert_eq!(Impact::None.quality(), 1.0);
        assert_eq!(Impact::Minor.quality(), 0.7);
        assert_eq!(Impact::Major.quality(), 0.4);
    }

    /// The less a level changes the result, the more it is worth: the order
    /// of `Impact` and the order of quality run opposite.
    #[test]
    fn a_lesser_impact_is_worth_more() {
        let qualities = [Impact::None, Impact::Minor, Impact::Major].map(Impact::quality);
        assert!(
            qualities.windows(2).all(|pair| pair[0] > pair[1]),
            "quality falls as the impact grows: {qualities:?}"
        );
    }
}
