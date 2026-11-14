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

//! File names: sanitising, keeping an extension, naming a saved entity.

/// Returns `entity`'s display name as the scene tree shows it, without
/// reaching the `World`.
pub(super) fn entity_display_name(
    roots: &[khora_sdk::editor_ui::SceneNode],
    entity: khora_sdk::prelude::ecs::EntityId,
) -> Option<String> {
    fn walk(
        nodes: &[khora_sdk::editor_ui::SceneNode],
        target: khora_sdk::prelude::ecs::EntityId,
    ) -> Option<String> {
        for node in nodes {
            if node.entity == target {
                return Some(node.name.clone());
            }
            if let Some(found) = walk(&node.children, target) {
                return Some(found);
            }
        }
        None
    }
    walk(roots, entity)
}

/// Returns `name` safe as a file name on every platform, or `"prefab"`.
///
/// Each run of unsafe characters becomes one underscore.
pub(super) fn sanitize_for_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_was_replacement = false;
    for ch in name.chars() {
        let safe = ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | ' ');
        if safe {
            out.push(ch);
            last_was_replacement = false;
        } else if !last_was_replacement {
            out.push('_');
            last_was_replacement = true;
        }
    }
    let trimmed = out.trim_matches(&[' ', '.', '_'][..]).to_string();
    if trimmed.is_empty() {
        "prefab".to_string()
    } else {
        trimmed
    }
}

/// Returns `new_name` with `old_name`'s extension when it has none of its own:
/// `crate.png` renamed `box` gives `box.png`, renamed `box.jpg` stays so.
pub(super) fn ensure_extension(new_name: &str, old_name: &str) -> String {
    if new_name.contains('.') {
        return new_name.to_string();
    }
    match old_name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => format!("{new_name}.{ext}"),
        _ => new_name.to_string(),
    }
}
