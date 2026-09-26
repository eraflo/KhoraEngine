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

/// Walks a `SceneNode` forest to find `entity`'s display name. Used by
/// the asset browser's drop receiver to compose a default `.kprefab`
/// filename without a round-trip through the live `World`.
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

/// Strips characters that aren't safe in cross-platform file names
/// (path separators, shell-meta, control chars). Falls back to an
/// underscore for runs of stripped chars so consecutive replacements
/// don't collapse into nothing.
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

/// Re-applies the original file's extension to a user-edited name when the
/// user omitted one (so renaming `crate.png` to `box` yields `box.png`, but
/// `box.jpg` is honoured verbatim).
pub(super) fn ensure_extension(new_name: &str, old_name: &str) -> String {
    if new_name.contains('.') {
        return new_name.to_string();
    }
    match old_name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => format!("{new_name}.{ext}"),
        _ => new_name.to_string(),
    }
}
