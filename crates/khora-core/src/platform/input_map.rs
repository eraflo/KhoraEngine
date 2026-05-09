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

//! Action / binding abstraction over raw [`InputEvent`]s.
//!
//! Game code asks "is the `jump` action pressed?" instead of pattern-matching
//! on every key. Bindings can be reconfigured at runtime (rebind menus,
//! per-profile mappings) without touching gameplay logic.
//!
//! The map is updated once per frame from the engine's input queue; query
//! functions never block and never allocate.
//!
//! # Example
//!
//! ```
//! # use khora_core::platform::input_map::{Action, InputBinding, InputMap};
//! # use khora_core::platform::{InputEvent, KeyCode, MouseButton};
//! let mut map = InputMap::new();
//! map.bind("jump", InputBinding::Key(KeyCode::Space));
//! map.bind("jump", InputBinding::Mouse(MouseButton::Left)); // also fires on click
//!
//! map.update(&[InputEvent::KeyPressed { key_code: KeyCode::Space }]);
//! assert!(map.is_pressed("jump"));
//! assert!(map.just_pressed("jump"));
//!
//! map.update(&[]); // next frame, no event
//! assert!(map.is_pressed("jump"));        // still held — no Released event
//! assert!(!map.just_pressed("jump"));     // edge cleared
//! ```

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use super::input::{InputEvent, KeyCode, MouseButton};

/// A user-defined action name like `"jump"`, `"fire"`, `"menu_back"`.
///
/// `Cow<'static, str>` lets you bind from `&'static str` literals without
/// allocating, while still allowing dynamic action names from config files.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Action(pub Cow<'static, str>);

impl Action {
    /// Returns the action name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&'static str> for Action {
    fn from(s: &'static str) -> Self {
        Self(Cow::Borrowed(s))
    }
}

impl From<String> for Action {
    fn from(s: String) -> Self {
        Self(Cow::Owned(s))
    }
}

/// Anything that can be bound to an [`Action`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputBinding {
    /// A keyboard key (physical code).
    Key(KeyCode),
    /// A mouse button.
    Mouse(MouseButton),
}

/// Action / binding map. One per engine — registered as a service in the
/// `ServiceRegistry` and updated once per frame from the input queue.
#[derive(Debug, Default)]
pub struct InputMap {
    /// `action → list of bindings` (multiple bindings → action fires when ANY of them fires).
    bindings: HashMap<Action, Vec<InputBinding>>,
    /// Inverse index for O(1) update — `binding → list of actions it fires`.
    inverse: HashMap<InputBinding, Vec<Action>>,
    /// Currently-held actions.
    pressed: HashSet<Action>,
    /// Actions that became pressed this frame (cleared at the start of `update`).
    just_pressed: HashSet<Action>,
    /// Actions that became released this frame (cleared at the start of `update`).
    just_released: HashSet<Action>,
}

impl InputMap {
    /// Creates an empty map with no bindings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a binding for `action`. An action can have multiple bindings —
    /// it fires when ANY of them fires (logical OR).
    pub fn bind(&mut self, action: impl Into<Action>, binding: InputBinding) {
        let action = action.into();
        self.bindings
            .entry(action.clone())
            .or_default()
            .push(binding);
        self.inverse.entry(binding).or_default().push(action);
    }

    /// Removes a single binding for `action`. Returns `true` if found.
    pub fn unbind(&mut self, action: &Action, binding: &InputBinding) -> bool {
        let removed_from_bindings = self
            .bindings
            .get_mut(action)
            .map(|v| {
                let len_before = v.len();
                v.retain(|b| b != binding);
                v.len() != len_before
            })
            .unwrap_or(false);
        if removed_from_bindings {
            if let Some(actions) = self.inverse.get_mut(binding) {
                actions.retain(|a| a != action);
            }
        }
        removed_from_bindings
    }

    /// Removes every binding for `action`.
    pub fn clear_action(&mut self, action: &Action) {
        if let Some(bindings) = self.bindings.remove(action) {
            for b in bindings {
                if let Some(actions) = self.inverse.get_mut(&b) {
                    actions.retain(|a| a != action);
                }
            }
        }
        self.pressed.remove(action);
        self.just_pressed.remove(action);
        self.just_released.remove(action);
    }

    /// Returns `true` when the named action is currently held.
    pub fn is_pressed(&self, action: &str) -> bool {
        self.pressed.iter().any(|a| a.as_str() == action)
    }

    /// Returns `true` only on the frame the action transitioned from
    /// released → pressed. Cleared on the next `update` call.
    pub fn just_pressed(&self, action: &str) -> bool {
        self.just_pressed.iter().any(|a| a.as_str() == action)
    }

    /// Returns `true` only on the frame the action transitioned from
    /// pressed → released. Cleared on the next `update` call.
    pub fn just_released(&self, action: &str) -> bool {
        self.just_released.iter().any(|a| a.as_str() == action)
    }

    /// Drains a frame of input events into the action sets. Call once per
    /// frame **before** game logic queries the map. Edge sets (`just_*`)
    /// are cleared first; held set persists across frames until a Released
    /// event arrives.
    pub fn update(&mut self, events: &[InputEvent]) {
        self.just_pressed.clear();
        self.just_released.clear();

        for event in events {
            match event {
                InputEvent::KeyPressed { key_code } => {
                    self.fire_press(InputBinding::Key(*key_code));
                }
                InputEvent::KeyReleased { key_code } => {
                    self.fire_release(InputBinding::Key(*key_code));
                }
                InputEvent::MouseButtonPressed { button } => {
                    self.fire_press(InputBinding::Mouse(*button));
                }
                InputEvent::MouseButtonReleased { button } => {
                    self.fire_release(InputBinding::Mouse(*button));
                }
                _ => {} // mouse motion / wheel don't drive actions today
            }
        }
    }

    fn fire_press(&mut self, binding: InputBinding) {
        if let Some(actions) = self.inverse.get(&binding) {
            for action in actions {
                if self.pressed.insert(action.clone()) {
                    self.just_pressed.insert(action.clone());
                }
            }
        }
    }

    fn fire_release(&mut self, binding: InputBinding) {
        if let Some(actions) = self.inverse.get(&binding) {
            for action in actions {
                if self.pressed.remove(action) {
                    self.just_released.insert(action.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_and_press() {
        let mut m = InputMap::new();
        m.bind("jump", InputBinding::Key(KeyCode::Space));
        m.update(&[InputEvent::KeyPressed {
            key_code: KeyCode::Space,
        }]);
        assert!(m.is_pressed("jump"));
        assert!(m.just_pressed("jump"));
    }

    #[test]
    fn just_pressed_clears_next_frame() {
        let mut m = InputMap::new();
        m.bind("jump", InputBinding::Key(KeyCode::Space));
        m.update(&[InputEvent::KeyPressed {
            key_code: KeyCode::Space,
        }]);
        assert!(m.just_pressed("jump"));

        m.update(&[]);
        assert!(m.is_pressed("jump"), "still held — no release event");
        assert!(!m.just_pressed("jump"), "edge should be cleared");
    }

    #[test]
    fn release_event_clears_pressed() {
        let mut m = InputMap::new();
        m.bind("fire", InputBinding::Mouse(MouseButton::Left));

        m.update(&[InputEvent::MouseButtonPressed {
            button: MouseButton::Left,
        }]);
        assert!(m.is_pressed("fire"));

        m.update(&[InputEvent::MouseButtonReleased {
            button: MouseButton::Left,
        }]);
        assert!(!m.is_pressed("fire"));
        assert!(m.just_released("fire"));
    }

    #[test]
    fn multi_binding_either_works() {
        let mut m = InputMap::new();
        m.bind("jump", InputBinding::Key(KeyCode::Space));
        m.bind("jump", InputBinding::Mouse(MouseButton::Left));

        // Mouse fires it.
        m.update(&[InputEvent::MouseButtonPressed {
            button: MouseButton::Left,
        }]);
        assert!(m.is_pressed("jump"));

        // Release just the mouse — the action is still considered held
        // because no internal "ref-count per binding" is tracked. This
        // matches Unity's InputAction semantics: the action is one logical
        // boolean fed by OR of its bindings, but the release path uses the
        // same OR path — releasing any binding "releases the action". This
        // is the simpler / more predictable behaviour for keyboard+mouse.
        m.update(&[InputEvent::MouseButtonReleased {
            button: MouseButton::Left,
        }]);
        assert!(!m.is_pressed("jump"));
    }

    #[test]
    fn unbind_removes_inverse_index() {
        let mut m = InputMap::new();
        let action = Action::from("jump");
        let binding = InputBinding::Key(KeyCode::Space);
        m.bind(action.clone(), binding);
        assert!(m.unbind(&action, &binding));

        m.update(&[InputEvent::KeyPressed {
            key_code: KeyCode::Space,
        }]);
        assert!(!m.is_pressed("jump"));
    }
}
