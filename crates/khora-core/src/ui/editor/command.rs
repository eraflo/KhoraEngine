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

//! Undo/redo command history for editor operations.
//!
//! Each user action (property edit, spawn, delete) is wrapped in an
//! [`EditorCommand`] and pushed onto the [`CommandHistory`] stack.

use super::state::PropertyEdit;

/// A reversible editor operation.
#[derive(Debug, Clone)]
pub struct EditorCommand {
    /// Human-readable label (e.g. "Set Transform", "Rename Entity").
    pub description: String,
    /// The forward edit.
    pub forward: PropertyEdit,
    /// The reverse edit to undo `forward`.
    pub reverse: PropertyEdit,
}

/// Fixed-capacity undo/redo stack.
///
/// New commands push onto `undo_stack` and clear `redo_stack`.
/// Undo pops from `undo_stack`, applies `reverse`, pushes onto `redo_stack`.
/// Redo pops from `redo_stack`, applies `forward`, pushes onto `undo_stack`.
#[derive(Debug, Clone)]
pub struct CommandHistory {
    undo_stack: Vec<EditorCommand>,
    redo_stack: Vec<EditorCommand>,
    max_size: usize,
}

impl Default for CommandHistory {
    fn default() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_size: 256,
        }
    }
}

impl CommandHistory {
    /// Creates a new history with the given maximum stack depth.
    pub fn new(max_size: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_size,
        }
    }

    /// Push a command after executing its forward edit.
    pub fn push(&mut self, cmd: EditorCommand) {
        self.redo_stack.clear();
        if self.undo_stack.len() >= self.max_size {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(cmd);
    }

    /// Undo the last command. Returns the reverse `PropertyEdit` to apply.
    pub fn undo(&mut self) -> Option<PropertyEdit> {
        let cmd = self.undo_stack.pop()?;
        let reverse = cmd.reverse.clone();
        self.redo_stack.push(cmd);
        Some(reverse)
    }

    /// Redo the last undone command. Returns the forward `PropertyEdit` to apply.
    pub fn redo(&mut self) -> Option<PropertyEdit> {
        let cmd = self.redo_stack.pop()?;
        let forward = cmd.forward.clone();
        self.undo_stack.push(cmd);
        Some(forward)
    }

    /// Whether there is anything to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Whether there is anything to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Description of the next undoable command (for UI display).
    pub fn undo_description(&self) -> Option<&str> {
        self.undo_stack.last().map(|c| c.description.as_str())
    }

    /// Description of the next redoable command (for UI display).
    pub fn redo_description(&self) -> Option<&str> {
        self.redo_stack.last().map(|c| c.description.as_str())
    }

    /// Number of commands currently on the undo stack. Bounded by the
    /// configured maximum depth.
    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::entity::EntityId;

    fn cmd(label: &str) -> EditorCommand {
        let e = EntityId {
            index: 0,
            generation: 0,
        };
        EditorCommand {
            description: label.to_owned(),
            forward: PropertyEdit::SetName(e, format!("{label}-fwd")),
            reverse: PropertyEdit::SetName(e, format!("{label}-rev")),
        }
    }

    #[test]
    fn push_is_bounded_and_evicts_oldest() {
        let mut history = CommandHistory::new(4);
        for i in 0..10 {
            history.push(cmd(&format!("cmd{i}")));
        }
        // The stack never grows past its cap, and the oldest entries are the
        // ones dropped — the newest command is still on top.
        assert_eq!(history.undo_depth(), 4);
        assert_eq!(history.undo_description(), Some("cmd9"));
    }

    #[test]
    fn push_clears_the_redo_stack() {
        let mut history = CommandHistory::new(8);
        history.push(cmd("a"));
        history.undo();
        assert!(history.can_redo());
        // A fresh edit after an undo discards the redo branch.
        history.push(cmd("b"));
        assert!(!history.can_redo());
    }

    #[test]
    fn undo_then_redo_roundtrips() {
        let mut history = CommandHistory::new(8);
        history.push(cmd("a"));
        assert!(history.can_undo() && !history.can_redo());

        let reverse = history.undo().expect("a command to undo");
        assert!(matches!(reverse, PropertyEdit::SetName(_, ref s) if s == "a-rev"));
        assert!(!history.can_undo() && history.can_redo());

        let forward = history.redo().expect("a command to redo");
        assert!(matches!(forward, PropertyEdit::SetName(_, ref s) if s == "a-fwd"));
        assert!(history.can_undo() && !history.can_redo());
    }

    #[test]
    fn undo_redo_on_empty_history_is_none() {
        let mut history = CommandHistory::new(4);
        assert!(history.undo().is_none());
        assert!(history.redo().is_none());
    }
}
