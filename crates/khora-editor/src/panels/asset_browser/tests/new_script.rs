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

//! Making a new script from the browser: where it goes, the behaviour name
//! the typed text gives, the source it starts with, and what a placeholder
//! tile queues when it is committed, cancelled, or left behind.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{AssetEntry, EditorState, UiTheme};
use khora_sdk::khora_core::asset::AssetUUID;

use super::super::new_script::{behavior_name, script_folder_for, script_source};
use super::super::AssetBrowserPanel;
use crate::widgets::tile::AssetTileKind;

fn entry(path: &str, asset_type: &str) -> AssetEntry {
    AssetEntry {
        name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        asset_type: asset_type.to_owned(),
        source_path: path.to_owned(),
        uuid: AssetUUID::new_v5(path),
        container: None,
    }
}

/// A browser over a project holding `entries`, its list read, its tree on
/// `props` with a search typed and the mesh filter chosen.
fn browsing(entries: Vec<AssetEntry>) -> (AssetBrowserPanel, Arc<Mutex<EditorState>>) {
    let state = Arc::new(Mutex::new(EditorState {
        project_folder: Some("project".to_owned()),
        asset_entries: entries,
        asset_epoch: 1,
        ..EditorState::default()
    }));
    let mut panel = AssetBrowserPanel::new(Arc::clone(&state), UiTheme::default());
    panel.rescan_if_needed();
    panel.current_folder = Some("props".to_owned());
    panel.search_filter = "crate".to_owned();
    panel.selected_filter = Some(AssetTileKind::Mesh);
    (panel, state)
}

/// A project with a prop, a guard script under `scripts/ai`, and a material.
fn a_small_project() -> Vec<AssetEntry> {
    vec![
        entry("props/crate.obj", "mesh"),
        entry("scripts/ai/guard.erg", "script"),
        entry("materials/stone.kmat", "material"),
    ]
}

/// What the placeholder holds, as `(folder, name)`.
fn placeholder(panel: &AssetBrowserPanel) -> Option<(String, String)> {
    panel
        .new_script
        .as_ref()
        .map(|script| (script.folder.clone(), script.name.clone()))
}

/// Types `text` into the placeholder's name field.
fn type_name(panel: &mut AssetBrowserPanel, text: &str) {
    panel
        .new_script
        .as_mut()
        .expect("a script is being named")
        .name = text.to_owned();
}

fn queued(state: &Arc<Mutex<EditorState>>) -> Option<(String, String)> {
    state.lock().expect("state").pending_create_script.clone()
}

// ─── Where a script goes ─────────────────────────────────────────────

#[test]
fn a_script_made_outside_scripts_goes_into_scripts() {
    for (from, to) in [
        ("", "scripts"),
        ("props", "scripts"),
        ("scriptsextra", "scripts"),
        ("props/scripts", "scripts"),
    ] {
        assert_eq!(script_folder_for(from), to, "made from `{from}`");
    }
}

#[test]
fn a_script_made_in_scripts_or_under_it_stays_there() {
    for folder in ["scripts", "scripts/ai", "scripts/ai/bosses"] {
        assert_eq!(script_folder_for(folder), folder);
    }
}

// ─── The behaviour name the typed text gives ─────────────────────────

#[test]
fn typed_text_becomes_a_pascal_case_behavior_name() {
    for (typed, name) in [
        ("player", "Player"),
        ("my player", "MyPlayer"),
        ("enemy-ai.erg", "EnemyAi"),
        ("Guard.ERG", "Guard"),
        ("mainMenu", "MainMenu"),
        ("snake_case", "Snake_case"),
        ("  padded  ", "Padded"),
        ("a.b c", "ABC"),
    ] {
        assert_eq!(
            behavior_name(typed).as_deref(),
            Some(name),
            "typed `{typed}`"
        );
    }
}

#[test]
fn a_name_starting_with_a_digit_is_prefixed_so_it_is_an_identifier() {
    assert_eq!(behavior_name("2d mover").as_deref(), Some("_2dMover"));
}

#[test]
fn text_with_nothing_usable_gives_no_behavior_name() {
    for typed in ["", "   ", "!!!", ".erg", " .ERG ", "-- --"] {
        assert_eq!(behavior_name(typed), None, "typed `{typed}`");
    }
}

// ─── The source it starts with ───────────────────────────────────────

#[test]
fn a_new_scripts_source_compiles_without_a_diagnostic() {
    for name in ["Player", "NewScript", "_2dMover", "Snake_case"] {
        let file = format!("{name}.erg");
        let loader = khora_script::MemoryLoader::new().with(&file, script_source(name));
        let outcome = khora_script::compile_module(&loader, &file);

        assert!(
            outcome.diagnostics.is_empty(),
            "`{name}`'s source does not compile cleanly: {:?}",
            outcome.diagnostics
        );
        assert!(outcome.program.is_some(), "`{name}` produced no program");
    }
}

#[test]
fn a_new_scripts_source_declares_a_behavior_named_after_it() {
    let source = script_source("Mover");
    assert!(
        source.contains("behavior Mover"),
        "no `behavior Mover` in:\n{source}"
    );

    let loader = khora_script::MemoryLoader::new().with("Mover.erg", source);
    let program = khora_script::compile_module(&loader, "Mover.erg")
        .program
        .expect("the source compiles");
    assert!(
        program.layout("Mover").is_some(),
        "the program declares no behaviour `Mover`"
    );
}

// ─── Starting one ────────────────────────────────────────────────────

#[test]
fn starting_a_script_outside_scripts_opens_scripts_with_nothing_hiding_it() {
    let (mut panel, _state) = browsing(a_small_project());

    panel.start_new_script("props");

    assert_eq!(
        placeholder(&panel),
        Some(("scripts".to_owned(), "NewScript".to_owned()))
    );
    assert_eq!(panel.current_folder.as_deref(), Some("scripts"));
    for folder in ["", "scripts"] {
        assert_eq!(
            panel.expanded_folders.get(folder),
            Some(&true),
            "`{folder}` is expanded"
        );
    }
    assert!(panel.search_filter.is_empty(), "no search hides it");
    assert_eq!(panel.selected_filter, None, "the mesh filter would hide it");
    assert!(panel.rename_focus_pending, "the name field takes the focus");
}

#[test]
fn starting_a_script_keeps_a_script_filter() {
    let (mut panel, _state) = browsing(a_small_project());
    panel.selected_filter = Some(AssetTileKind::Script);

    panel.start_new_script("props");

    assert_eq!(panel.selected_filter, Some(AssetTileKind::Script));
}

#[test]
fn starting_a_script_under_scripts_stays_in_that_folder() {
    let (mut panel, _state) = browsing(a_small_project());

    panel.start_new_script("scripts/ai");

    assert_eq!(
        placeholder(&panel).map(|(folder, _)| folder).as_deref(),
        Some("scripts/ai")
    );
    assert_eq!(panel.current_folder.as_deref(), Some("scripts/ai"));
    for folder in ["", "scripts", "scripts/ai"] {
        assert_eq!(
            panel.expanded_folders.get(folder),
            Some(&true),
            "`{folder}` is expanded"
        );
    }
}

/// Behaviour names are project-wide and Windows paths ignore case, so the
/// default is free against every script, in any folder, whatever its case.
/// Files that are not scripts do not take a name.
#[test]
fn a_new_scripts_default_name_is_free_across_every_folder_ignoring_case() {
    let (mut panel, _state) = browsing(vec![
        entry("props/newscript.erg", "script"),
        entry("scripts/ai/NEWSCRIPT2.erg", "script"),
        entry("materials/NewScript3.kmat", "material"),
    ]);

    panel.start_new_script("scripts");

    assert_eq!(
        placeholder(&panel).map(|(_, name)| name).as_deref(),
        Some("NewScript3")
    );
}

#[test]
fn starting_a_script_abandons_a_tile_rename_and_a_folder_rename() {
    let (mut panel, _state) = browsing(a_small_project());
    panel.renaming_index = Some(0);
    panel.renaming_folder = Some("props".to_owned());

    panel.start_new_script("props");

    assert_eq!(panel.renaming_index, None, "the tile rename is abandoned");
    assert_eq!(
        panel.renaming_folder, None,
        "the folder rename is abandoned"
    );
    assert!(panel.new_script.is_some());
}

// ─── The file name it would write ────────────────────────────────────

#[test]
fn the_placeholder_previews_the_file_name_it_would_write() {
    let (mut panel, _state) = browsing(vec![entry("scripts/ai/Guard.erg", "script")]);
    panel.start_new_script("scripts");
    assert_eq!(
        panel.new_script_file_name().as_deref(),
        Some("NewScript.erg")
    );

    type_name(&mut panel, "my player");
    assert_eq!(
        panel.new_script_file_name().as_deref(),
        Some("MyPlayer.erg")
    );

    type_name(&mut panel, "guard");
    assert_eq!(
        panel.new_script_file_name().as_deref(),
        Some("Guard2.erg"),
        "`Guard` is taken by `scripts/ai/Guard.erg`"
    );

    type_name(&mut panel, "!!!");
    assert_eq!(panel.new_script_file_name(), None);
}

#[test]
fn with_no_script_being_named_there_is_no_file_name_to_preview() {
    let (panel, _state) = browsing(a_small_project());

    assert_eq!(panel.new_script_file_name(), None);
}

// ─── Committing, cancelling, leaving ─────────────────────────────────

#[test]
fn committing_a_script_queues_its_file_named_after_its_behavior() {
    let (mut panel, state) = browsing(a_small_project());
    panel.start_new_script("scripts/ai");
    type_name(&mut panel, "player");

    panel.commit_new_script();

    let (rel, source) = queued(&state).expect("the script is queued");
    assert_eq!(rel, "scripts/ai/Player.erg");
    assert!(
        source.contains("behavior Player"),
        "no `behavior Player` in:\n{source}"
    );
    assert_eq!(source, script_source("Player"));
    assert!(panel.new_script.is_none(), "the placeholder is gone");
}

#[test]
fn a_script_made_from_the_root_is_queued_under_scripts() {
    let (mut panel, state) = browsing(a_small_project());
    panel.start_new_script("");

    panel.commit_new_script();

    assert_eq!(
        queued(&state).map(|(rel, _)| rel).as_deref(),
        Some("scripts/NewScript.erg")
    );
}

#[test]
fn a_committed_name_another_script_holds_gets_a_number() {
    let (mut panel, state) = browsing(vec![entry("scripts/other/player.erg", "script")]);
    panel.start_new_script("scripts/ai");
    type_name(&mut panel, "Player");

    panel.commit_new_script();

    let (rel, source) = queued(&state).expect("the script is queued");
    assert_eq!(rel, "scripts/ai/Player2.erg");
    assert_eq!(source, script_source("Player2"));
}

#[test]
fn committing_a_name_with_nothing_usable_queues_nothing() {
    let (mut panel, state) = browsing(a_small_project());
    panel.start_new_script("scripts");
    type_name(&mut panel, "  !!! ");

    panel.commit_new_script();

    assert_eq!(queued(&state), None);
    assert!(panel.new_script.is_none(), "the placeholder is gone");
}

#[test]
fn cancelling_a_script_queues_nothing() {
    let (mut panel, state) = browsing(a_small_project());
    panel.start_new_script("scripts");
    type_name(&mut panel, "player");

    panel.cancel_new_script();

    assert_eq!(queued(&state), None);
    assert!(panel.new_script.is_none(), "the placeholder is gone");
}

#[test]
fn closing_the_project_drops_a_script_being_named() {
    let (mut panel, state) = browsing(a_small_project());
    panel.start_new_script("scripts");

    state.lock().expect("state").project_folder = None;
    panel.rescan_if_needed();

    assert!(panel.new_script.is_none(), "no project to write it in");
    assert_eq!(queued(&state), None);
}

// ─── The source compiles where the engine compiles it ────────────────

/// A loader serving one script, refusing what the engine's own loader
/// refuses: a component or behaviour named like a component the engine
/// stores.
struct EngineLoader {
    scripts: khora_script::MemoryLoader,
    engine: Vec<String>,
}

impl khora_script::SourceLoader for EngineLoader {
    fn load(&self, path: &str) -> Option<String> {
        self.scripts.load(path)
    }

    fn engine_components(&self) -> &[String] {
        &self.engine
    }
}

/// Every name the engine's components hold, former and retired ones too —
/// the list the engine's script loader serves.
fn engine_component_names() -> Vec<String> {
    inventory::iter::<khora_sdk::ComponentRegistration>
        .into_iter()
        .flat_map(|registration| {
            std::iter::once(registration.type_name).chain(registration.formerly.iter().copied())
        })
        .chain(
            inventory::iter::<khora_sdk::khora_data::scene::RetiredComponent>
                .into_iter()
                .map(|retired| retired.name),
        )
        .map(str::to_owned)
        .collect()
}

/// The errors the engine reports compiling the script queued at `rel`.
fn engine_errors(rel: &str, source: &str) -> Vec<String> {
    let module = rel.strip_prefix("scripts/").unwrap_or(rel);
    let loader = EngineLoader {
        scripts: khora_script::MemoryLoader::new().with(module, source),
        engine: engine_component_names(),
    };
    khora_script::compile_module(&loader, module)
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.clone())
        .collect()
}

#[test]
fn a_script_named_like_an_engine_component_still_compiles_in_the_engine() {
    let mut refused = Vec::new();
    for typed in [
        "camera",
        "light",
        "transform",
        "rigid body",
        "audio source",
        "tag",
    ] {
        let (mut panel, state) = browsing(a_small_project());
        panel.start_new_script("scripts");
        type_name(&mut panel, typed);

        panel.commit_new_script();

        let (rel, source) = queued(&state).expect("the script is queued");
        let errors = engine_errors(&rel, &source);
        if !errors.is_empty() {
            refused.push(format!("typed `{typed}` → {rel}: {errors:?}"));
        }
    }
    assert!(
        refused.is_empty(),
        "the engine refuses the new script:\n{}",
        refused.join("\n")
    );
}

/// An asset type is a type of the language: a behaviour may not take its
/// name, and `sound` or `material` are names a first script is given.
#[test]
fn a_script_named_like_an_asset_type_still_compiles() {
    let mut refused = Vec::new();
    for typed in ["prefab", "sound", "material", "mesh", "texture", "scene"] {
        let (mut panel, state) = browsing(a_small_project());
        panel.start_new_script("scripts");
        type_name(&mut panel, typed);

        panel.commit_new_script();

        let (rel, source) = queued(&state).expect("the script is queued");
        let errors = engine_errors(&rel, &source);
        if !errors.is_empty() {
            refused.push(format!("typed `{typed}` → {rel}: {errors:?}"));
        }
    }
    assert!(
        refused.is_empty(),
        "the new script does not compile:\n{}",
        refused.join("\n")
    );
}

/// The placeholder takes the grid's first slot: a grid left scrolled down
/// would hide it, its focused name field with it.
#[test]
fn starting_a_script_scrolls_the_grid_back_to_its_placeholder() {
    let (mut panel, _state) = browsing(a_small_project());
    panel.current_folder = Some("scripts".to_owned());
    panel
        .grid_scroll
        .drag_to(1_000.0, [0.0, 0.0, 300.0, 100.0], 1_000.0);
    assert!(
        panel.grid_scroll.offset() > 0.0,
        "the grid is scrolled down"
    );

    panel.start_new_script("scripts");

    assert_eq!(
        panel.grid_scroll.offset(),
        0.0,
        "the placeholder in the first slot is scrolled out of view"
    );
}
