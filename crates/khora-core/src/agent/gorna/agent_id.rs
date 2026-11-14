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

//! An agent's identity: the name its family declares.

use serde::{Deserialize, Serialize};

/// An agent's identity: its name, which its family declares
/// ([`LaneFamily::AGENT`](crate::lane::family::LaneFamily::AGENT)).
///
/// The core knows no agent: a family names itself where it lives, and boot
/// refuses two families with one name. Agents compare, hash and order by
/// their name, so a decision journal reads the same from one build to the
/// next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AgentId(&'static str);

impl AgentId {
    /// The agent named `name`.
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// The agent's name.
    pub fn name(&self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl Serialize for AgentId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for AgentId {
    /// An agent read back, as a journal records it. The name outlives the
    /// read, so it is kept for the program's life: a reader should read each
    /// agent's name once (a journal's table of names), not once per entry.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Ok(Self(Box::leak(name.into_boxed_str())))
    }
}
