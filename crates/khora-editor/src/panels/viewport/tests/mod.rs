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

//! An asset tile released over the viewport queues the request its kind
//! calls for, naming the asset by its identity — a glTF file standing for
//! its default scene, a part of a file for itself.

mod broken_play_camera;

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{
    AssetEntry, EditorPanel, EditorState, FontFamilyHint, Interaction, TextAlign, UiBuilder,
    UiTheme,
};
use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::prelude::ecs::EntityId;
use khora_sdk::ViewportTextureHandle;

use super::ViewportPanel;
use crate::camera::EditorCamera;
use crate::drag_payload::pack_asset_drag;
use crate::tests::gltf_fixture::{file_entry, part_entry};

const SHIP: &str = "models/ship.glb";
const CRATE: &str = "models/crate.obj";
const TURRET: &str = "prefabs/turret.kprefab";
const EPOCH: u64 = 1;

/// A frame of the viewport: its image shown over a 640 × 480 panel, the
/// pointer at its centre, and `drop` released over it.
struct DropUi {
    drop: Option<u64>,
}

impl UiBuilder for DropUi {
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
    fn interact_rect(&mut self, _: &str, _: [f32; 4]) -> Interaction {
        Interaction::default()
    }
    fn dnd_take_drop_payload(&mut self) -> Option<u64> {
        self.drop.take()
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
        [0.0, 0.0, 640.0, 480.0]
    }
    fn screen_rect(&self) -> [f32; 4] {
        [0.0, 0.0, 640.0, 480.0]
    }
    fn available_width(&self) -> f32 {
        640.0
    }
    fn available_height(&self) -> f32 {
        480.0
    }
    fn heading(&mut self, _: &str) {}
    fn label(&mut self, _: &str) {}
    fn colored_label(&mut self, _: [f32; 4], _: &str) {}
    fn colored_label_with_tooltip(&mut self, _: [f32; 4], _: &str, _: &str) {}
    fn small_label(&mut self, _: &str) {}
    fn monospace(&mut self, _: &str) {}
    fn button(&mut self, _: &str) -> bool {
        false
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
        Some([320.0, 240.0])
    }
    fn separator(&mut self) {}
    fn spacing(&mut self, _: f32) {}
    fn is_last_item_double_clicked(&self) -> bool {
        false
    }
    fn is_last_item_hovered(&self) -> bool {
        true
    }
    fn is_last_item_enter_pressed(&self) -> bool {
        false
    }
    fn is_last_item_escape_pressed(&self) -> bool {
        false
    }
    fn context_menu_last(&mut self, _: &mut dyn FnMut(&mut dyn UiBuilder)) {}
    fn viewport_image(&mut self, _: ViewportTextureHandle, _: [f32; 2]) -> Option<[f32; 2]> {
        Some([0.0, 0.0])
    }
}

/// The browser's list for a project holding a `.glb` (the file and its
/// parts), an OBJ and a `.kprefab`.
fn entries() -> Vec<AssetEntry> {
    vec![
        file_entry(SHIP, "mesh"),
        part_entry(SHIP, "scene", "prefab"),
        part_entry(SHIP, "scene0", "prefab"),
        part_entry(SHIP, "mesh0/prim0", "mesh"),
        part_entry(SHIP, "mesh1/prim0", "mesh"),
        part_entry(SHIP, "material0", "material"),
        part_entry(SHIP, "material1", "material"),
        part_entry(SHIP, "texture0", "texture"),
        file_entry(CRATE, "mesh"),
        file_entry(TURRET, "prefab"),
    ]
}

fn selected() -> EntityId {
    EntityId {
        index: 7,
        generation: 3,
    }
}

/// Releases the tile of the asset `uuid` over the viewport of an editor
/// listing `entries`, one entity selected; returns the editor state after.
fn drop_on_viewport(entries: Vec<AssetEntry>, uuid: AssetUUID) -> Arc<Mutex<EditorState>> {
    let idx = entries
        .iter()
        .position(|entry| entry.uuid == uuid)
        .expect("the asset is listed");
    let mut editor = EditorState {
        asset_entries: entries,
        asset_epoch: EPOCH,
        ..EditorState::default()
    };
    editor.select(selected());
    let state = Arc::new(Mutex::new(editor));
    let mut viewport = ViewportPanel::new(
        ViewportTextureHandle(1),
        Arc::clone(&state),
        Arc::new(Mutex::new(EditorCamera::default())),
        UiTheme::default(),
    );
    let mut ui = DropUi {
        drop: Some(pack_asset_drag(idx as u32, EPOCH)),
    };
    viewport.ui(&mut ui);
    assert_eq!(ui.drop, None, "the viewport takes the drop");
    state
}

fn ship() -> AssetUUID {
    AssetUUID::new_v5(SHIP)
}

fn part(label: &str) -> AssetUUID {
    AssetUUID::sub_asset(ship(), label)
}

/// **A glTF file dropped onto the viewport spawns its default scene**: a
/// prefab instance of the file's `scene` part.
#[test]
fn a_gltf_file_dropped_on_the_viewport_spawns_its_scene() {
    let state = drop_on_viewport(entries(), ship());

    let state = state.lock().expect("the editor state");
    assert_eq!(state.pending_prefab_spawn, Some((part("scene"), None)));
    assert_eq!(state.pending_spawn_mesh_asset, None, "no single-mesh spawn");
}

/// A glTF file whose scene the index does not list spawns as one mesh of
/// its own identity.
#[test]
fn a_gltf_file_without_its_scene_listed_spawns_as_one_mesh_in_the_viewport() {
    let state = drop_on_viewport(vec![file_entry(SHIP, "mesh")], ship());

    let state = state.lock().expect("the editor state");
    let spawned = state
        .pending_spawn_mesh_asset
        .map(|(uuid, _, parent)| (uuid, parent));
    assert_eq!(spawned, Some((ship(), None)));
    assert_eq!(state.pending_prefab_spawn, None);
}

/// **An OBJ dropped onto the viewport still spawns as one mesh**, of its own
/// identity, at the scene's root.
#[test]
fn an_obj_dropped_on_the_viewport_spawns_its_mesh() {
    let crate_mesh = AssetUUID::new_v5(CRATE);
    let state = drop_on_viewport(entries(), crate_mesh);

    let state = state.lock().expect("the editor state");
    let spawned = state
        .pending_spawn_mesh_asset
        .map(|(uuid, _, parent)| (uuid, parent));
    assert_eq!(spawned, Some((crate_mesh, None)));
    assert_eq!(state.pending_prefab_spawn, None);
}

/// **A mesh part dropped onto the viewport spawns that mesh**, by its own
/// identity.
#[test]
fn a_mesh_part_dropped_on_the_viewport_spawns_that_mesh() {
    let state = drop_on_viewport(entries(), part("mesh0/prim0"));

    let state = state.lock().expect("the editor state");
    let spawned = state
        .pending_spawn_mesh_asset
        .map(|(uuid, _, parent)| (uuid, parent));
    assert_eq!(spawned, Some((part("mesh0/prim0"), None)));
}

/// A `.kprefab` dropped onto the viewport spawns by its own identity.
#[test]
fn a_prefab_file_dropped_on_the_viewport_spawns_it() {
    let turret = AssetUUID::new_v5(TURRET);
    let state = drop_on_viewport(entries(), turret);

    let state = state.lock().expect("the editor state");
    assert_eq!(state.pending_prefab_spawn, Some((turret, None)));
}

/// **A material part dropped onto the viewport is assigned to the
/// selection**, by its own identity.
#[test]
fn a_material_part_dropped_on_the_viewport_is_assigned_to_the_selection() {
    let state = drop_on_viewport(entries(), part("material0"));

    let state = state.lock().expect("the editor state");
    assert_eq!(
        state.pending_assign_texture,
        Some((part("material0"), selected()))
    );
}

/// A texture part dropped onto the viewport is assigned to the selection,
/// by its own identity.
#[test]
fn a_texture_part_dropped_on_the_viewport_is_assigned_to_the_selection() {
    let state = drop_on_viewport(entries(), part("texture0"));

    let state = state.lock().expect("the editor state");
    assert_eq!(
        state.pending_assign_texture,
        Some((part("texture0"), selected()))
    );
}

/// The panel's render-target size, and the rect it records for input.
mod target_size {
    use super::super::*;
    use khora_tool_ui::testing::RecordingUiBuilder;

    /// Lays the viewport panel out once in `ui` and returns the rect it
    /// recorded for the input pipeline.
    fn laid_out_rect(mut ui: RecordingUiBuilder) -> Option<[f32; 4]> {
        let state = Arc::new(Mutex::new(EditorState::default()));
        let mut panel = ViewportPanel::new(
            ViewportTextureHandle(1),
            Arc::clone(&state),
            Arc::new(Mutex::new(EditorCamera::default())),
            UiTheme::default(),
        );
        panel.ui(&mut ui);
        let rect = state.lock().expect("the editor state").viewport_screen_rect;
        rect
    }

    #[test]
    fn target_size_rounds_the_panel_size_to_the_nearest_pixel() {
        assert_eq!(target_size_for([0.0, 0.0, 1199.6, 500.4]), (1200, 500));
        assert_eq!(target_size_for([35.0, 80.0, 640.6, 359.4]), (641, 359));
    }

    #[test]
    fn target_size_ignores_the_panel_position() {
        assert_eq!(
            target_size_for([0.0, 0.0, 800.0, 600.0]),
            target_size_for([412.0, 96.0, 800.0, 600.0])
        );
    }

    #[test]
    fn target_size_of_a_sub_pixel_panel_is_one_pixel() {
        assert_eq!(target_size_for([10.0, 20.0, 0.3, 0.0]), (1, 1));
    }

    #[test]
    fn target_size_of_a_negative_side_is_one_pixel() {
        assert_eq!(target_size_for([0.0, 0.0, -40.0, 300.0]), (1, 300));
        assert_eq!(target_size_for([0.0, 0.0, 300.0, -0.7]), (300, 1));
    }

    #[test]
    fn target_size_of_a_non_finite_side_is_one_pixel() {
        assert_eq!(target_size_for([0.0, 0.0, f32::NAN, 500.0]), (1, 500));
        assert_eq!(target_size_for([0.0, 0.0, 500.0, f32::NAN]), (500, 1));
        assert_eq!(
            target_size_for([0.0, 0.0, f32::INFINITY, f32::NEG_INFINITY]),
            (1, 1)
        );
    }

    /// The input pipeline compares the rect to the platform cursor, which is
    /// in physical pixels: at 1.5 pixels per point a 1200×500-point panel is
    /// 1800×750 pixels.
    #[test]
    fn the_viewport_rect_is_stored_in_physical_pixels() {
        let ui = RecordingUiBuilder::new(1200.0, 500.0)
            .with_viewport_image()
            .with_pixels_per_point(1.5);
        assert_eq!(laid_out_rect(ui), Some([0.0, 0.0, 1800.0, 750.0]));
    }

    #[test]
    fn at_one_pixel_per_point_the_viewport_rect_is_the_panel_size() {
        let ui = RecordingUiBuilder::new(1200.0, 500.0)
            .with_viewport_image()
            .with_pixels_per_point(1.0);
        assert_eq!(laid_out_rect(ui), Some([0.0, 0.0, 1200.0, 500.0]));
    }
}
