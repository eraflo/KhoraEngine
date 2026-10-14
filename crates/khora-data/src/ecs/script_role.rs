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
/// A behavior is a component: adding `Guard` to an entity is what attaches
/// the behavior, and its state companion `Guard#state` is what the engine
/// writes of it while it runs. The registry records which is which, so the
/// script flow finds the behaviors a world holds — and the module whose
/// program runs each — from its components alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ScriptRole {
    /// The component a behavior is: its authored fields.
    Behavior {
        /// The module that declares the behavior, relative to the script
        /// root.
        module: String,
        /// Where the behavior sits among the behaviors and systems of its
        /// module, in source order.
        order: u32,
    },
    /// The component a system is: its fields, held by the system's one
    /// runtime entity.
    System {
        /// The module that declares the system, relative to the script root.
        module: String,
        /// Where the system sits among the behaviors and systems of its
        /// module, in source order.
        order: u32,
    },
    /// A behavior's state companion: what the engine keeps of an instance.
    BehaviorState {
        /// The behavior it belongs to, by name.
        behavior: String,
    },
}

impl ScriptRole {
    /// The module and the source order of a component a program runs — a
    /// behavior's or a system's; `None` for a state companion.
    pub fn runs(&self) -> Option<(&str, u32)> {
        match self {
            Self::Behavior { module, order } | Self::System { module, order } => {
                Some((module, *order))
            }
            Self::BehaviorState { .. } => None,
        }
    }
}
