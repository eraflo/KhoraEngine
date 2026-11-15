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

//! What a declared component is to scripting.

/// The part a component plays for scripts, when it plays one.
///
/// Adding the behavior `Guard` to an entity attaches it; its state companion
/// `Guard#state` is what the engine writes of it while it runs. Recorded so
/// the script flow finds a world's behaviors from its components alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ScriptRole {
    /// The component a behavior is: its authored fields.
    Behavior {
        /// The module that declares the behavior, relative to the script root.
        module: String,
        /// Its place in source order among its module's behaviors and systems.
        order: u32,
    },
    /// The component a system is: its fields, held by the system's one runtime entity.
    System {
        /// The module that declares the system, relative to the script root.
        module: String,
        /// Its place in source order among its module's behaviors and systems.
        order: u32,
    },
    /// A behavior's state companion: what the engine keeps of an instance.
    BehaviorState {
        /// The behavior it belongs to, by name.
        behavior: String,
    },
}

impl ScriptRole {
    /// Returns the module and source order of a behavior or a system, `None` for a state companion.
    pub fn runs(&self) -> Option<(&str, u32)> {
        match self {
            Self::Behavior { module, order } | Self::System { module, order } => {
                Some((module, *order))
            }
            Self::BehaviorState { .. } => None,
        }
    }
}
