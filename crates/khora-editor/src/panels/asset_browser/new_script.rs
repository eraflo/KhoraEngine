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

//! Making a new Ergon script from the asset browser: the name is typed first,
//! on a placeholder tile, and the file is written named after the behaviour
//! inside it, under `scripts/` where the engine compiles it.

use khora_sdk::editor_ui::{FontFamilyHint, InlineEditEvent, TextAlign, UiBuilder, UiTheme};

use super::AssetBrowserPanel;
use crate::panels::problems::view::SCRIPT_DIR;
use crate::widgets::tile::{paint_asset_tile, AssetTileKind};

/// The name a new script is offered before anything is typed.
const DEFAULT_NAME: &str = "NewScript";

/// The extension of an Ergon file.
const EXTENSION: &str = "erg";

/// The folder a new script goes in when made from `folder`.
///
/// The engine compiles the scripts under `scripts/` and no others, so a script
/// made anywhere else would never run: it goes to `scripts/` instead. Made in
/// `scripts/` or under it, it stays where the author is.
pub(super) fn script_folder_for(folder: &str) -> String {
    let inside = folder == SCRIPT_DIR
        || folder
            .strip_prefix(SCRIPT_DIR)
            .is_some_and(|rest| rest.starts_with('/'));
    if inside {
        folder.to_owned()
    } else {
        SCRIPT_DIR.to_owned()
    }
}

/// The behaviour name `typed` gives, or None when nothing usable is left.
///
/// An Ergon name is ASCII letters, digits and `_`, not starting with a digit.
/// Anything else separates words, and each word starts with a capital:
/// `my player` is `MyPlayer`. A leading digit is kept behind a `_`.
pub(super) fn behavior_name(typed: &str) -> Option<String> {
    let typed = typed.trim();
    let stem = match typed.len().checked_sub(EXTENSION.len() + 1) {
        Some(cut)
            if typed.is_char_boundary(cut)
                && typed[cut..].eq_ignore_ascii_case(&format!(".{EXTENSION}")) =>
        {
            &typed[..cut]
        }
        _ => typed,
    };
    let mut name = String::with_capacity(stem.len() + 1);
    for word in stem
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .filter(|word| !word.is_empty())
    {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            name.push(first.to_ascii_uppercase());
            name.extend(chars);
        }
    }
    if name.is_empty() {
        return None;
    }
    if name.starts_with(|ch: char| ch.is_ascii_digit()) {
        name.insert(0, '_');
    }
    Some(name)
}

/// The source a new script named `name` starts with: a behaviour that runs,
/// each part saying what it is for, so a first-time author can start from it.
pub(super) fn script_source(name: &str) -> String {
    format!(
        "// {name} — a behavior, written in Ergon.\n\
         //\n\
         // To give it to an entity: select the entity, then in the Inspector\n\
         // click + Add Component and choose {name}. It runs when you press Play.\n\
         //\n\
         // Save this file and the editor reads it again. If something is wrong,\n\
         // the Problems tab says which line, and why.\n\
         \n\
         behavior {name} {{\n\
         \x20   // A field: every entity holding {name} has its own, and the\n\
         \x20   // Inspector edits it.\n\
         \x20   float speed = 1.0;\n\
         \n\
         \x20   // Once per entity, the first time this behavior runs on it.\n\
         \x20   void OnSpawn() {{\n\
         \x20       Log(\"{name} is running\");\n\
         \x20   }}\n\
         \n\
         \x20   // Once per frame. `dt` is the time since the last frame, in seconds.\n\
         \x20   void Update(float dt) {{\n\
         \x20   }}\n\
         }}\n"
    )
}

/// A script being named on its placeholder tile, not yet written.
pub(super) struct NewScript {
    /// Forward-slash folder under `assets/` it will be written in.
    pub folder: String,
    /// What is typed in the placeholder's name field.
    pub name: String,
}

impl AssetBrowserPanel {
    /// Puts a placeholder script tile, named after a free default, in the
    /// folder a script made from `folder` goes in, and opens that folder.
    pub(super) fn start_new_script(&mut self, folder: &str) {
        let folder = script_folder_for(folder);
        self.expanded_folders.insert(String::new(), true);
        let mut at = folder.as_str();
        while !at.is_empty() {
            self.expanded_folders.insert(at.to_owned(), true);
            at = at.rfind('/').map_or("", |p| &at[..p]);
        }
        self.current_folder = Some(folder.clone());
        self.search_filter.clear();
        if self.selected_filter != Some(AssetTileKind::Script) {
            self.selected_filter = None;
        }
        self.renaming_index = None;
        self.renaming_folder = None;
        self.rename_focus_pending = true;
        // The placeholder takes the first slot: a grid left scrolled down
        // would hide it, and the name field holding the keyboard with it.
        self.grid_scroll.reset();
        let name = self.free_script_name(DEFAULT_NAME);
        self.new_script = Some(NewScript { folder, name });
    }

    /// The file name the placeholder would create now (`"MyPlayer.erg"`), or None.
    pub(super) fn new_script_file_name(&self) -> Option<String> {
        let typed = &self.new_script.as_ref()?.name;
        let name = self.free_script_name(&behavior_name(typed)?);
        Some(format!("{name}.{EXTENSION}"))
    }

    /// Queues the placeholder's file for writing, when its name is usable.
    pub(super) fn commit_new_script(&mut self) {
        let Some(script) = self.new_script.take() else {
            return;
        };
        let Some(name) = behavior_name(&script.name) else {
            log::warn!(
                "Asset browser: no script made — `{}` holds no letter or digit to name it by",
                script.name.trim()
            );
            return;
        };
        let name = self.free_script_name(&name);
        let rel = format!("{}/{name}.{EXTENSION}", script.folder);
        if let Ok(mut state) = self.state.lock() {
            state.pending_create_script = Some((rel.clone(), script_source(&name)));
        }
        log::info!("Asset browser: creating script '{rel}'");
    }

    /// Drops the placeholder; nothing is written.
    pub(super) fn cancel_new_script(&mut self) {
        self.new_script = None;
    }

    /// `name`, or `name2`, `name3`, … — the first neither the engine nor a
    /// script of the project already holds. The engine's names — its
    /// components', its asset kinds' — no script can declare. Behaviour names
    /// are the project's, whatever folder declares them, and file names differ
    /// only by case on some systems, so every `.erg` counts, its stem compared
    /// without case.
    fn free_script_name(&self, name: &str) -> String {
        let taken = |candidate: &str| {
            khora_sdk::scripts::is_engine_name(candidate)
                || self.flat.iter().any(|asset| {
                    asset.name.rsplit_once('.').is_some_and(|(stem, ext)| {
                        ext.eq_ignore_ascii_case(EXTENSION) && stem.eq_ignore_ascii_case(candidate)
                    })
                })
        };
        if !taken(name) {
            return name.to_owned();
        }
        (2..)
            .map(|n| format!("{name}{n}"))
            .find(|candidate| !taken(candidate))
            .unwrap_or_else(|| name.to_owned())
    }
}

/// Paints the placeholder of the script being named, if any, at `origin`: a
/// selected Script tile whose label is the name field, the file it will write
/// (`file`) named on its thumbnail. `focus` asks the field for the keyboard
/// once. Returns what the field did this frame.
///
/// A free function over the panel's fields rather than a method: the grid
/// holds the asset list borrowed while it paints.
pub(super) fn paint_new_script_tile(
    ui: &mut dyn UiBuilder,
    origin: [f32; 2],
    size: [f32; 2],
    theme: &UiTheme,
    script: Option<&mut NewScript>,
    focus: &mut bool,
    file: Option<&str>,
) -> InlineEditEvent {
    let Some(script) = script else {
        return InlineEditEvent::Idle;
    };
    let [x, y] = origin;
    let [w, _] = size;
    paint_asset_tile(
        ui,
        "tile-new-script",
        origin,
        size,
        "",
        AssetTileKind::Script,
        true,
        None,
        theme,
    );
    // The name typed is not always the name written: say which file it will
    // be, where the tile's own thumbnail has room for it.
    if let Some(file) = file {
        let shown = fit(ui, file, w - 16.0);
        ui.paint_text_styled(
            [x + w * 0.5, y + 10.0],
            &shown,
            9.5,
            [1.0, 1.0, 1.0, 0.8],
            FontFamilyHint::Monospace,
            TextAlign::Center,
        );
    }
    let event = ui.inline_text_field(
        [x + 3.0, y + w - 2.0, w - 6.0, 20.0],
        "new-script",
        &mut script.name,
        *focus,
    );
    *focus = false;
    event
}

/// `text` cut to `room` pixels at the thumbnail's type, its end replaced by
/// `…` when it does not fit.
fn fit(ui: &mut dyn UiBuilder, text: &str, room: f32) -> String {
    let width =
        |ui: &mut dyn UiBuilder, s: &str| ui.measure_text(s, 9.5, FontFamilyHint::Monospace)[0];
    if width(ui, text) <= room {
        return text.to_owned();
    }
    let mut kept: String = text.to_owned();
    while kept.pop().is_some() {
        let candidate = format!("{kept}…");
        if width(ui, &candidate) <= room {
            return candidate;
        }
    }
    "…".to_owned()
}
