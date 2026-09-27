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

//! Compile-level guard over `khora_tool_ui::dock`, the dock tree the editor
//! lays its panels out with.
//!
//! Every `pub` item of the module is named here at its full path: the module,
//! its types, free functions, constants, type alias, public fields, enum
//! variants (matched exhaustively), inherent `pub` methods and derived trait
//! impls — serde included, since the tree is saved with the editor layout. A
//! reorganisation that moves code between files must keep every one of these
//! paths valid, so this file stops compiling the moment one disappears.
//!
//! Nothing is constructed; the tests only have to type-check.

use std::any::type_name;

fn is_clone<T: Clone>() {}
fn is_copy<T: Copy>() {}
fn is_debug<T: std::fmt::Debug>() {}
fn is_default<T: Default>() {}
fn is_deserialize_owned<T: serde::de::DeserializeOwned>() {}
fn is_eq<T: Eq>() {}
fn is_hash<T: std::hash::Hash>() {}
fn is_partial_eq<T: PartialEq>() {}
fn is_serialize<T: serde::Serialize>() {}

#[allow(unused_imports)]
mod every_pub_mod {
    use khora_tool_ui::dock as _;
}

// ---------------------------------------------------------------------------
// Public fields (read through a reference) and enum variants (matched
// exhaustively). Nothing is constructed.
// ---------------------------------------------------------------------------

fn dock_layout_fields(x: &khora_tool_ui::dock::DockLayout) {
    let _ = (&x.groups, &x.splitters);
}

fn dock_node_variants(x: &khora_tool_ui::dock::DockNode) {
    match x {
        khora_tool_ui::dock::DockNode::Tabs { .. } => {}
        khora_tool_ui::dock::DockNode::Split { .. } => {}
    }
}

fn drop_zone_variants(x: &khora_tool_ui::dock::DropZone) {
    match x {
        khora_tool_ui::dock::DropZone::Center => {}
        khora_tool_ui::dock::DropZone::Left => {}
        khora_tool_ui::dock::DropZone::Right => {}
        khora_tool_ui::dock::DropZone::Top => {}
        khora_tool_ui::dock::DropZone::Bottom => {}
    }
}

fn split_axis_variants(x: &khora_tool_ui::dock::SplitAxis) {
    match x {
        khora_tool_ui::dock::SplitAxis::Horizontal => {}
        khora_tool_ui::dock::SplitAxis::Vertical => {}
    }
}

fn split_id_fields(x: &khora_tool_ui::dock::SplitId) {
    let _ = (&x.0,);
}

fn splitter_layout_fields(x: &khora_tool_ui::dock::SplitterLayout) {
    let _ = (&x.id, &x.rect, &x.axis, &x.bounds);
}

fn tab_group_layout_fields(x: &khora_tool_ui::dock::TabGroupLayout) {
    let _ = (&x.rect, &x.panels, &x.active);
}

#[test]
fn module_dock_paths_still_resolve() {
    // `DockRect` is a plain `[f32; 4]` (x, y, w, h) — the geometry every
    // layout call takes.
    let _: khora_tool_ui::dock::DockRect = [0.0; 4];
    const _: f32 = khora_tool_ui::dock::MIN_PANE;
    const _: f32 = khora_tool_ui::dock::SPLITTER_THICKNESS;
    let _ = type_name::<khora_tool_ui::dock::DockLayout>();
    let _ = dock_layout_fields as fn(&khora_tool_ui::dock::DockLayout);
    is_debug::<khora_tool_ui::dock::DockLayout>();
    is_clone::<khora_tool_ui::dock::DockLayout>();
    is_default::<khora_tool_ui::dock::DockLayout>();
    is_partial_eq::<khora_tool_ui::dock::DockLayout>();
    let _ = type_name::<khora_tool_ui::dock::DockNode>();
    let _ = dock_node_variants as fn(&khora_tool_ui::dock::DockNode);
    is_debug::<khora_tool_ui::dock::DockNode>();
    is_clone::<khora_tool_ui::dock::DockNode>();
    is_partial_eq::<khora_tool_ui::dock::DockNode>();
    is_serialize::<khora_tool_ui::dock::DockNode>();
    is_deserialize_owned::<khora_tool_ui::dock::DockNode>();
    let _ = type_name::<khora_tool_ui::dock::DockRect>();
    let _ = type_name::<khora_tool_ui::dock::DockTree>();
    let _ = khora_tool_ui::dock::DockTree::empty;
    let _: fn(String) -> khora_tool_ui::dock::DockTree = khora_tool_ui::dock::DockTree::single;
    let _ = khora_tool_ui::dock::DockTree::is_empty;
    let _ = khora_tool_ui::dock::DockTree::panels;
    let _ = khora_tool_ui::dock::DockTree::contains;
    let _: fn(
        &mut khora_tool_ui::dock::DockTree,
        String,
        Option<&str>,
        khora_tool_ui::dock::DropZone,
    ) -> Option<khora_tool_ui::dock::SplitId> = khora_tool_ui::dock::DockTree::insert;
    let _ = khora_tool_ui::dock::DockTree::remove;
    let _ = khora_tool_ui::dock::DockTree::activate;
    let _ = khora_tool_ui::dock::DockTree::set_ratio;
    let _ = khora_tool_ui::dock::DockTree::layout;
    is_debug::<khora_tool_ui::dock::DockTree>();
    is_clone::<khora_tool_ui::dock::DockTree>();
    is_partial_eq::<khora_tool_ui::dock::DockTree>();
    is_serialize::<khora_tool_ui::dock::DockTree>();
    is_deserialize_owned::<khora_tool_ui::dock::DockTree>();
    is_default::<khora_tool_ui::dock::DockTree>();
    let _ = type_name::<khora_tool_ui::dock::DropZone>();
    let _ = drop_zone_variants as fn(&khora_tool_ui::dock::DropZone);
    let _ = khora_tool_ui::dock::DropZone::axis;
    let _ = khora_tool_ui::dock::DropZone::takes_first;
    is_debug::<khora_tool_ui::dock::DropZone>();
    is_clone::<khora_tool_ui::dock::DropZone>();
    is_copy::<khora_tool_ui::dock::DropZone>();
    is_partial_eq::<khora_tool_ui::dock::DropZone>();
    is_eq::<khora_tool_ui::dock::DropZone>();
    let _ = khora_tool_ui::dock::MIN_PANE;
    let _ = khora_tool_ui::dock::SPLITTER_THICKNESS;
    let _ = type_name::<khora_tool_ui::dock::SplitAxis>();
    let _ = split_axis_variants as fn(&khora_tool_ui::dock::SplitAxis);
    is_debug::<khora_tool_ui::dock::SplitAxis>();
    is_clone::<khora_tool_ui::dock::SplitAxis>();
    is_copy::<khora_tool_ui::dock::SplitAxis>();
    is_partial_eq::<khora_tool_ui::dock::SplitAxis>();
    is_eq::<khora_tool_ui::dock::SplitAxis>();
    is_serialize::<khora_tool_ui::dock::SplitAxis>();
    is_deserialize_owned::<khora_tool_ui::dock::SplitAxis>();
    let _ = type_name::<khora_tool_ui::dock::SplitId>();
    let _ = split_id_fields as fn(&khora_tool_ui::dock::SplitId);
    is_debug::<khora_tool_ui::dock::SplitId>();
    is_clone::<khora_tool_ui::dock::SplitId>();
    is_copy::<khora_tool_ui::dock::SplitId>();
    is_partial_eq::<khora_tool_ui::dock::SplitId>();
    is_eq::<khora_tool_ui::dock::SplitId>();
    is_hash::<khora_tool_ui::dock::SplitId>();
    is_serialize::<khora_tool_ui::dock::SplitId>();
    is_deserialize_owned::<khora_tool_ui::dock::SplitId>();
    let _ = type_name::<khora_tool_ui::dock::SplitterLayout>();
    let _ = splitter_layout_fields as fn(&khora_tool_ui::dock::SplitterLayout);
    is_debug::<khora_tool_ui::dock::SplitterLayout>();
    is_clone::<khora_tool_ui::dock::SplitterLayout>();
    is_partial_eq::<khora_tool_ui::dock::SplitterLayout>();
    let _ = type_name::<khora_tool_ui::dock::TabGroupLayout>();
    let _ = tab_group_layout_fields as fn(&khora_tool_ui::dock::TabGroupLayout);
    let _ = khora_tool_ui::dock::TabGroupLayout::active_panel;
    is_debug::<khora_tool_ui::dock::TabGroupLayout>();
    is_clone::<khora_tool_ui::dock::TabGroupLayout>();
    is_partial_eq::<khora_tool_ui::dock::TabGroupLayout>();
    let _ = khora_tool_ui::dock::ratio_from_pointer;
    let _ = khora_tool_ui::dock::zone_at;
}

// ---------------------------------------------------------------------------
// Paths other crates import today, spelled exactly as they spell them.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod paths_used_by_other_crates {
    use khora_tool_ui::dock::zone_at as _; // khora-editor
    use khora_tool_ui::dock::DockTree as _; // khora-editor
    use khora_tool_ui::dock::DropZone as _; // khora-editor
}
