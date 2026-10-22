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

//! The parts of a file — a glTF's scenes, meshes, materials, textures — in
//! the browser: folded under their file's tile until it is opened, and never
//! renamed, moved, deleted or duplicated on their own.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{
    AssetEntry, EditorPanel, EditorState, FontFamilyHint, Interaction, TextAlign, UiBuilder,
    UiTheme,
};

use super::super::AssetBrowserPanel;
use crate::gltf_fixture::{file_entry, part_entry, GltfProject, SHIP_GLTF, TRIANGLE_OBJ};
use crate::hot_reload::collect_asset_entries;

const SHIP: &str = "models/ship.gltf";
const CRATE: &str = "models/crate.obj";
const WOOD: &str = "textures/wood.png";

/// The index of `ship.gltf` in [`listing`].
const SHIP_AT: usize = 3;
/// The indices of the parts of `ship.gltf` in [`listing`].
const PARTS_AT: [usize; 3] = [0, 2, 4];
/// The indices of the files in [`listing`], in order.
const FILES_AT: [usize; 3] = [1, 3, 5];

/// A project's list as the index may give it: the parts of `ship.gltf`
/// around and before their file, among other files.
fn listing() -> Vec<AssetEntry> {
    vec![
        part_entry(SHIP, "scene", "prefab"),
        file_entry(CRATE, "mesh"),
        part_entry(SHIP, "mesh0/prim0", "mesh"),
        file_entry(SHIP, "mesh"),
        part_entry(SHIP, "material0", "material"),
        file_entry(WOOD, "texture"),
    ]
}

/// A browser over a project listing `entries`, on its root folder, with no
/// filter or search.
fn browsing(entries: Vec<AssetEntry>) -> (AssetBrowserPanel, Arc<Mutex<EditorState>>) {
    let state = Arc::new(Mutex::new(EditorState {
        project_folder: Some("project".to_owned()),
        asset_entries: entries,
        asset_epoch: 1,
        ..EditorState::default()
    }));
    let mut panel = AssetBrowserPanel::new(Arc::clone(&state), UiTheme::default());
    panel.rescan_if_needed();
    (panel, state)
}

/// **A file's parts are no tiles of its folder while the file is closed —
/// and every file starts closed**: the grid shows the files only.
#[test]
fn a_files_parts_are_no_tiles_of_its_folder_until_it_is_opened() {
    let (panel, _state) = browsing(listing());

    assert_eq!(panel.grid_tiles(), FILES_AT.to_vec());
}

/// **An opened file shows its parts right after its own tile**, before the
/// files that follow it.
#[test]
fn an_opened_file_shows_its_parts_right_after_its_tile() {
    let (mut panel, _state) = browsing(listing());

    panel.toggle_container(SHIP_AT);
    let tiles = panel.grid_tiles();

    assert_eq!(
        tiles.len(),
        6,
        "the three files and the three parts: {tiles:?}"
    );
    let ship = tiles
        .iter()
        .position(|&i| i == SHIP_AT)
        .expect("the file keeps its tile");
    let mut following: Vec<usize> = tiles[ship + 1..ship + 1 + PARTS_AT.len()].to_vec();
    following.sort_unstable();
    assert_eq!(
        following,
        PARTS_AT.to_vec(),
        "the parts follow the file: {tiles:?}"
    );
    let files: Vec<usize> = tiles
        .iter()
        .copied()
        .filter(|i| !PARTS_AT.contains(i))
        .collect();
    assert_eq!(files, FILES_AT.to_vec(), "the files keep their order");
}

/// Closing an opened file folds its parts back under it.
#[test]
fn closing_an_opened_file_folds_its_parts_back() {
    let (mut panel, _state) = browsing(listing());

    panel.toggle_container(SHIP_AT);
    panel.toggle_container(SHIP_AT);

    assert_eq!(panel.grid_tiles(), FILES_AT.to_vec());
}

/// A file's tile counts its parts; a file with none counts none.
#[test]
fn a_files_tile_counts_its_parts() {
    let (panel, _state) = browsing(listing());

    assert_eq!(panel.sub_count(SHIP_AT), 3);
    assert_eq!(panel.sub_count(1), 0, "the OBJ holds no parts");
    assert_eq!(panel.sub_count(5), 0, "the image holds no parts");
}

/// **A part offers no file operation** — no rename, move, delete or
/// duplicate: it follows its file, which keeps offering them.
#[test]
fn a_part_offers_no_file_operation() {
    let (panel, _state) = browsing(listing());

    for file in FILES_AT {
        assert!(
            panel.file_ops_offered(file),
            "the file at {file} offers them"
        );
    }
    for part in PARTS_AT {
        assert!(
            !panel.file_ops_offered(part),
            "the part at {part} offers file operations"
        );
    }
}

/// **Revealing a file's path selects the file, never one of its parts** —
/// though its parts are found at the same path, and listed before it.
#[test]
fn revealing_a_files_path_selects_the_file_not_one_of_its_parts() {
    let (mut panel, state) = browsing(listing());
    state.lock().expect("state").pending_reveal_asset = Some(SHIP.to_owned());

    panel.follow_reveal();

    let state = state.lock().expect("state");
    assert_eq!(state.selected_asset, Some(SHIP_AT), "the file's tile");
    assert_eq!(panel.selected_index, Some(SHIP_AT));
    assert_eq!(state.inspected_asset_path.as_deref(), Some(SHIP));
}

/// **A glTF in the project is one tile, its parts folded under it**: the
/// browser over the project's own list shows the glTF and the OBJ beside
/// it, and the glTF's parts once it is opened.
#[test]
fn a_gltf_in_the_project_is_one_tile_with_its_parts_folded() {
    let project = GltfProject::with("browser-parts", &[(SHIP, SHIP_GLTF), (CRATE, TRIANGLE_OBJ)]);
    let entries = collect_asset_entries(&project.vfs.lock().expect("the project"));
    let ship = project.uuid(SHIP);
    let (mut panel, _state) = browsing(entries.clone());

    let tiles = panel.grid_tiles();
    let shown: Vec<&str> = tiles.iter().map(|&i| entries[i].name.as_str()).collect();
    let mut sorted = shown.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec!["crate.obj", "ship.gltf"],
        "the files only: {shown:?}"
    );

    let file = entries
        .iter()
        .position(|entry| entry.uuid == ship)
        .expect("the glTF is listed");
    panel.toggle_container(file);
    let parts = panel
        .grid_tiles()
        .into_iter()
        .filter(|&i| entries[i].container == Some(ship))
        .count();
    assert_eq!(parts, 7, "every part of the glTF, once opened");
}

/// **A kind filter keeps only tiles of its kind — an opened file's parts
/// included**: under "Meshes", the opened glTF shows its mesh part, never
/// its scene or its material.
#[test]
fn a_kind_filter_keeps_only_the_parts_of_its_kind() {
    use crate::widgets::tile::AssetTileKind;

    let (mut panel, _state) = browsing(listing());
    panel.toggle_container(SHIP_AT);
    panel.selected_filter = Some(AssetTileKind::Mesh);

    let tiles = panel.grid_tiles();

    let entries = listing();
    let foreign: Vec<&str> = tiles
        .iter()
        .filter(|&&i| entries[i].asset_type != "mesh")
        .map(|&i| entries[i].name.as_str())
        .collect();
    assert!(
        foreign.is_empty(),
        "tiles of another kind under the Meshes filter: {foreign:?} (tiles {tiles:?})"
    );
}

// ─── The browser's own actions on a part ───────────────────────────────────

/// A frame of the browser over a 1600 × 1000 panel: the regions named in
/// `clicks` are clicked, every context menu opens, and in them the entry
/// `choose` is picked.
struct BrowserUi {
    clicks: Vec<&'static str>,
    choose: Option<&'static str>,
}

impl UiBuilder for BrowserUi {
    fn paint_rect_filled(&mut self, _: [f32; 2], _: [f32; 2], _: [f32; 4], _: f32) {}
    fn paint_rect_stroke(&mut self, _: [f32; 2], _: [f32; 2], _: [f32; 4], _: f32, _: f32) {}
    fn paint_line(&mut self, _: [f32; 2], _: [f32; 2], _: [f32; 4], _: f32) {}
    fn paint_circle_filled(&mut self, _: [f32; 2], _: f32, _: [f32; 4]) {}
    fn paint_circle_stroke(&mut self, _: [f32; 2], _: f32, _: [f32; 4], _: f32) {}
    fn paint_text(&mut self, _: [f32; 2], _: [f32; 4], _: &str) {}
    fn paint_text_styled(
        &mut self,
        _: [f32; 2],
        _: &str,
        _: f32,
        _: [f32; 4],
        _: FontFamilyHint,
        _: TextAlign,
    ) {
    }
    fn paint_path_filled(&mut self, _: &[[f32; 2]], _: [f32; 4]) {}
    fn interact_rect(&mut self, id: &str, _: [f32; 4]) -> Interaction {
        let clicked = self.clicks.contains(&id);
        Interaction {
            hovered: clicked,
            contains_pointer: clicked,
            clicked,
            ..Default::default()
        }
    }
    fn region_at(&mut self, _: &str, _: [f32; 4], f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn cursor_pos(&self) -> [f32; 2] {
        [0.0, 0.0]
    }
    fn allocate_size(&mut self, size: [f32; 2]) -> [f32; 4] {
        [0.0, 0.0, size[0], size[1]]
    }
    fn panel_rect(&self) -> [f32; 4] {
        [0.0, 0.0, 1600.0, 1000.0]
    }
    fn screen_rect(&self) -> [f32; 4] {
        [0.0, 0.0, 1600.0, 1000.0]
    }
    fn available_width(&self) -> f32 {
        1600.0
    }
    fn available_height(&self) -> f32 {
        1000.0
    }
    fn heading(&mut self, _: &str) {}
    fn label(&mut self, _: &str) {}
    fn colored_label(&mut self, _: [f32; 4], _: &str) {}
    fn colored_label_with_tooltip(&mut self, _: [f32; 4], _: &str, _: &str) {}
    fn small_label(&mut self, _: &str) {}
    fn monospace(&mut self, _: &str) {}
    fn button(&mut self, text: &str) -> bool {
        self.choose == Some(text)
    }
    fn small_button(&mut self, _: &str) -> bool {
        false
    }
    fn selectable_label(&mut self, _: bool, _: &str) -> bool {
        false
    }
    fn selectable_label_double_clicked(&mut self, _: bool, _: &str) -> bool {
        false
    }
    fn checkbox(&mut self, _: &mut bool, _: &str) -> bool {
        false
    }
    fn drag_value_f32(&mut self, _: &str, _: &mut f32, _: f32) -> bool {
        false
    }
    fn slider_f32(&mut self, _: &str, _: &mut f32, _: f32, _: f32) -> bool {
        false
    }
    fn text_edit_singleline(&mut self, _: &mut String) -> bool {
        false
    }
    fn vec3_editor(&mut self, _: &str, _: &mut [f32; 3], _: f32) -> bool {
        false
    }
    fn color_edit(&mut self, _: &str, _: &mut [f32; 4]) -> bool {
        false
    }
    fn combo_box(&mut self, _: &str, _: &str, _: &mut usize, _: &[&str]) -> bool {
        false
    }
    fn horizontal(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn vertical(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn collapsing(&mut self, _: &str, _: bool, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn indent(&mut self, _: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn scroll_area(&mut self, _: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn pointer_position(&self) -> Option<[f32; 2]> {
        None
    }
    fn separator(&mut self) {}
    fn spacing(&mut self, _: f32) {}
    fn is_last_item_double_clicked(&self) -> bool {
        false
    }
    fn is_last_item_hovered(&self) -> bool {
        false
    }
    fn is_last_item_enter_pressed(&self) -> bool {
        false
    }
    fn is_last_item_escape_pressed(&self) -> bool {
        false
    }
    fn context_menu_last(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn viewport_image(
        &mut self,
        _: khora_sdk::ViewportTextureHandle,
        _: [f32; 2],
    ) -> Option<[f32; 2]> {
        None
    }
}

/// **The trash button never deletes a file for one of its parts**: with a
/// part of the glTF selected, the trash asks to delete nothing — a part
/// follows its file and is not deleted on its own.
#[test]
fn the_trash_button_never_deletes_a_file_for_a_selected_part() {
    let (mut panel, _state) = browsing(listing());
    panel.toggle_container(SHIP_AT);
    let material = PARTS_AT[2];
    panel.selected_index = Some(material);

    panel.ui(&mut BrowserUi {
        clicks: vec!["ab-trash"],
        choose: None,
    });

    assert_eq!(
        panel.confirm_delete,
        None,
        "the selected part `{}` asks to delete its file",
        listing()[material].name
    );
}

/// **"Assign to selected" on a glTF's material assigns that material** —
/// the part, by its identity — never the file it is found in, which is a
/// mesh.
#[test]
fn assigning_a_material_part_from_its_menu_assigns_the_part() {
    use khora_sdk::prelude::ecs::{MaterialRef, Name, Transform};
    use khora_sdk::GameWorld;

    use crate::gltf_fixture::ONE_NODE_GLTF;

    let project = GltfProject::with("browser-assign-material-part", &[(SHIP, ONE_NODE_GLTF)]);
    let entries = collect_asset_entries(&project.vfs.lock().expect("the project"));
    let ship = project.uuid(SHIP);
    let file = entries
        .iter()
        .position(|entry| entry.uuid == ship)
        .expect("the glTF is listed");
    let mut world = GameWorld::new();
    let target = world.spawn((Transform::identity(), Name::new("Target")));
    let (mut panel, state) = browsing(entries);
    state.lock().expect("state").select(target);
    panel.toggle_container(file);

    panel.ui(&mut BrowserUi {
        clicks: Vec::new(),
        choose: Some("Assign to selected"),
    });
    crate::commands::process_pending_assign_material(Some(&project.vfs), &mut world, &state);

    let part = project.sub(SHIP, "material_default");
    match world.get_component::<MaterialRef>(target) {
        Some(MaterialRef::Asset(uuid)) => assert_eq!(
            *uuid, part,
            "the entity wears {uuid:?}: the file is {ship:?}, the part {part:?}"
        ),
        Some(MaterialRef::Inline { .. }) => panic!("the entity wears an inline material"),
        None => panic!("nothing was assigned"),
    }
}
