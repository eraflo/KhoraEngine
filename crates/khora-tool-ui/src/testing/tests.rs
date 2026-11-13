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

use super::*;
use khora_core::ui::editor::GradientStop;

#[test]
fn a_radial_fill_is_recorded_with_its_polygon_centre_radius_and_stops() {
    let stops = [
        GradientStop {
            offset: 0.0,
            color: [1.0, 0.0, 0.0, 1.0],
        },
        GradientStop {
            offset: 1.0,
            color: [0.0, 0.0, 1.0, 0.5],
        },
    ];
    let points = [[0.0, 0.0], [40.0, 0.0], [20.0, 30.0]];
    let mut ui = RecordingUiBuilder::new(100.0, 100.0);
    ui.paint_path_filled_radial(
        &points,
        RadialGradient {
            center: [20.0, 10.0],
            radius: 25.0,
            stops: &stops,
        },
    );
    assert_eq!(
        ui.calls(),
        &[PaintCall::PathRadial {
            points: points.to_vec(),
            center: [20.0, 10.0],
            radius: 25.0,
            stops: stops.to_vec(),
        }]
    );
}

#[test]
fn a_click_at_a_point_clicks_every_region_over_it_and_no_other() {
    let mut ui = RecordingUiBuilder::new(100.0, 100.0).with_click_at([15.0, 15.0]);
    assert!(ui.interact_rect("over", [10.0, 10.0, 20.0, 20.0]).clicked);
    assert!(
        ui.interact_rect("also over", [0.0, 0.0, 100.0, 100.0])
            .clicked
    );
    assert!(!ui.interact_rect("beside", [40.0, 10.0, 20.0, 20.0]).clicked);
}

#[test]
fn a_region_scripted_by_its_salt_ignores_the_click_point() {
    let mut ui = RecordingUiBuilder::new(100.0, 100.0)
        .with_click_at([15.0, 15.0])
        .with_hover("scripted");
    let i = ui.interact_rect("scripted", [10.0, 10.0, 20.0, 20.0]);
    assert!(i.hovered && !i.clicked);
}

#[test]
fn a_tooltip_is_recorded_and_a_menu_opens_only_on_a_click() {
    let mut ui = RecordingUiBuilder::new(100.0, 100.0).with_click("more");
    ui.interact_rect("more", [0.0, 0.0, 10.0, 10.0]);
    ui.tooltip_for_last("More actions");
    ui.menu_on_click_last(&mut |menu| menu.paint_text([0.0, 0.0], [1.0; 4], "opened"));
    ui.interact_rect("still", [20.0, 0.0, 10.0, 10.0]);
    ui.menu_on_click_last(&mut |menu| menu.paint_text([0.0, 0.0], [1.0; 4], "not this"));

    assert_eq!(ui.tooltips(), &["More actions".to_owned()]);
    assert!(ui.painted_text("opened"));
    assert!(!ui.painted_text("not this"));
}

#[test]
fn a_scripted_key_is_pressed_and_no_other() {
    let ui = RecordingUiBuilder::new(100.0, 100.0).with_key(KeyCode::Escape);
    assert!(ui.key_pressed(KeyCode::Escape));
    assert!(ui.raw_key_pressed(KeyCode::Escape));
    assert!(!ui.key_pressed(KeyCode::Space));
    assert!(!RecordingUiBuilder::new(1.0, 1.0).key_pressed(KeyCode::Escape));
}
