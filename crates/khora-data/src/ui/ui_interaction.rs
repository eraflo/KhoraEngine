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

//! How a UI element is interacted with, and its current state.

use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// Track the interaction capabilities and current state of a UI element.
#[derive(Debug, Clone, Default, Component)]
#[component(domain = Ui)]
pub struct UiInteraction {
    /// The current interaction state, updated continuously by the input system
    pub state: UiInteractionState,

    /// If true, this element can receive focus (e.g., text inputs)
    pub focusable: bool,

    /// If true, this element blocks clicks/hovers from passing through to elements below it
    pub blocks_input: bool,
}

/// Represents the interaction state of a UI element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum UiInteractionState {
    #[default]
    /// The element is not being interacted with.
    Normal,
    /// The element is being hovered over.
    Hovered,
    /// The element is being pressed.
    Pressed,
    /// The element is focused.
    Focused,
}
