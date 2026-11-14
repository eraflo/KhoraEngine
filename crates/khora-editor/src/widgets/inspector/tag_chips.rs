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

//! The inspector's renderer for the `Tag` component: a chip per tag with a
//! remove button, and a row to add one.

use khora_sdk::editor_ui::UiBuilder;
use serde_json::Value;

use std::cell::RefCell;
use std::collections::HashMap;

use khora_sdk::prelude::ecs::EntityId;

thread_local! {
    /// Each entity's "add tag" text input, kept across frames.
    static ADD_BUFFERS: RefCell<HashMap<EntityId, String>> = RefCell::new(HashMap::new());
}

/// Renders a `Tag` value's chips and add row; returns whether the set
/// changed, for the caller to queue a `SetComponentJson` edit.
pub fn render_tag_chips(ui: &mut dyn UiBuilder, entity: EntityId, value: &mut Value) -> bool {
    let arr = match value {
        Value::Array(a) => a,
        _ => {
            ui.label("(unexpected Tag JSON shape — skipping)");
            return false;
        }
    };

    let mut changed = false;
    let mut to_remove: Option<usize> = None;

    // ── Chip strip ──
    ui.horizontal(&mut |row| {
        for (i, tag_value) in arr.iter().enumerate() {
            let tag = match tag_value.as_str() {
                Some(s) => s,
                None => continue,
            };
            row.label(tag);
            if row.small_button("x") {
                to_remove = Some(i);
            }
        }
        if arr.is_empty() {
            row.label("(no tags)");
        }
    });

    if let Some(i) = to_remove {
        arr.remove(i);
        changed = true;
    }

    // ── Add row: text input + "+" ──
    ADD_BUFFERS.with(|cell| {
        let mut buffers = cell.borrow_mut();
        let buf = buffers.entry(entity).or_default();

        let mut commit = false;
        ui.horizontal(&mut |row| {
            row.text_edit_singleline(buf);
            if row.small_button("+") {
                commit = true;
            }
        });

        if commit {
            let trimmed = buf.trim();
            if !trimmed.is_empty() {
                let already = arr.iter().any(|v| v.as_str() == Some(trimmed));
                if !already {
                    arr.push(Value::String(trimmed.to_owned()));
                    changed = true;
                }
            }
            buf.clear();
        }
    });

    changed
}
