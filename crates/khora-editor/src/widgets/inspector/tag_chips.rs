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

//! Custom inspector renderer for the `Tag` component.
//!
//! `SerializableTag(BTreeSet<String>)` serializes as a flat array of
//! strings. The generic JSON walker would render it as
//! `[0] = "alpha", [1] = "beta", …` rows, which is correct but ugly. This
//! renderer shows each tag as a chip with an `x` button to remove it,
//! plus a row at the bottom with a text input + `+` button to add new tags.
//!
//! On any mutation we mark the parent value `changed = true`; the inspector
//! then queues a `PropertyEdit::SetComponentJson` against the entity, which
//! re-runs the macro-generated `from_json` to rebuild a `Tag` from the
//! updated JSON array.

use khora_sdk::editor_ui::UiBuilder;
use serde_json::Value;

use std::cell::RefCell;
use std::collections::HashMap;

use khora_sdk::prelude::ecs::EntityId;

thread_local! {
    /// Per-entity scratch buffer for the "add tag" text input. Wiping the
    /// buffer after a successful add means the next chip starts empty.
    static ADD_BUFFERS: RefCell<HashMap<EntityId, String>> = RefCell::new(HashMap::new());
}

/// Renders the chip strip + add-row for a `Tag` component value. Returns
/// `true` if the tag set was mutated and the caller should queue a
/// `SetComponentJson` edit.
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
