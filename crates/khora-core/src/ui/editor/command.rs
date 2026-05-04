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
}
