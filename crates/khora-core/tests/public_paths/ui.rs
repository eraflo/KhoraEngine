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

//! Compile-level guard over `khora_core::ui` (the rest of the crate is in
//! `public_paths.rs` and its siblings).
//!
//! Every `pub` item reachable through a `pub` path is named here at its full
//! path: each `pub mod`, the `pub use` re-exports, their types, free functions,
//! constants, statics, type aliases, public fields, enum variants (matched
//! exhaustively), inherent `pub` methods (turbofished where generic), trait
//! items, and the std / serde / bincode / bytemuck / operator trait impls. A
//! reorganisation that moves code between files must keep every one of these
//! paths valid, so this file stops compiling the moment one disappears.
//!
//! Re-exports are also checked for *identity*: a `pub use` that starts pointing
//! at a different item with the same name is a compile error (`same_type` for
//! types, `same_item` for functions, bound forwarding for traits, `ptr::eq`
//! for statics).
//!
//! The list was generated from `khora_core`'s rustdoc JSON, so it is complete
//! for the tree it was written against. Nothing is constructed; the tests only
//! have to type-check.

use std::any::type_name;
use std::marker::PhantomData;

/// Compiles only when both arguments name the same type.
fn same_type<T: ?Sized>(_: PhantomData<T>, _: PhantomData<T>) {}

fn is_borrow_decode<T: bincode::BorrowDecode<'static, ()>>() {}
fn is_clone<T: Clone>() {}
fn is_copy<T: Copy>() {}
fn is_debug<T: std::fmt::Debug>() {}
fn is_decode<T: bincode::Decode<()>>() {}
fn is_default<T: Default>() {}
fn is_deserialize_owned<T: serde::de::DeserializeOwned>() {}
fn is_display<T: std::fmt::Display>() {}
fn is_encode<T: bincode::Encode>() {}
fn is_eq<T: Eq>() {}
fn is_error<T: std::error::Error>() {}
fn is_hash<T: std::hash::Hash>() {}
fn is_partial_eq<T: PartialEq>() {}
fn is_pod<T: bytemuck::Pod>() {}
fn is_serialize<T: serde::Serialize>() {}
fn is_zeroable<T: bytemuck::Zeroable>() {}

// ---------------------------------------------------------------------------
// Every `pub mod`, down to the leaves.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod every_pub_mod {
    use khora_core::ui as _;
    use khora_core::ui::app as _;
    use khora_core::ui::app::app_context as _;
    use khora_core::ui::app::runtime as _;
    use khora_core::ui::editor as _;
    use khora_core::ui::editor::gizmo as _;
    use khora_core::ui::editor::icons as _;
    use khora_core::ui::editor::overlay as _;
    use khora_core::ui::editor::panel as _;
    use khora_core::ui::editor::shell as _;
    use khora_core::ui::editor::state as _;
    use khora_core::ui::editor::ui_builder as _;
    use khora_core::ui::editor::viewport_texture as _;
    use khora_core::ui::fonts as _;
    use khora_core::ui::geometry as _;
    use khora_core::ui::layout as _;
    use khora_core::ui::theme as _;
    use khora_core::ui::types as _;
}

// ---------------------------------------------------------------------------
// Public fields (read through a reference) and enum variants (matched
// exhaustively). Nothing is constructed.
// ---------------------------------------------------------------------------

fn gizmo_line_instance_fields(x: &khora_core::ui::editor::gizmo::GizmoLineInstance) {
    let _ = (&x.start, &x.end, &x.color);
}

fn icon_variants(x: &khora_core::ui::editor::icons::Icon) {
    match x {
        khora_core::ui::editor::icons::Icon::Search => {}
        khora_core::ui::editor::icons::Icon::ChevronDown => {}
        khora_core::ui::editor::icons::Icon::ChevronRight => {}
        khora_core::ui::editor::icons::Icon::ArrowRight => {}
        khora_core::ui::editor::icons::Icon::Cube => {}
        khora_core::ui::editor::icons::Icon::Light => {}
        khora_core::ui::editor::icons::Icon::Camera => {}
        khora_core::ui::editor::icons::Icon::Folder => {}
        khora_core::ui::editor::icons::Icon::Layers => {}
        khora_core::ui::editor::icons::Icon::Globe => {}
        khora_core::ui::editor::icons::Icon::Axes => {}
        khora_core::ui::editor::icons::Icon::Grid => {}
        khora_core::ui::editor::icons::Icon::Sparkles => {}
        khora_core::ui::editor::icons::Icon::Package => {}
        khora_core::ui::editor::icons::Icon::Play => {}
        khora_core::ui::editor::icons::Icon::Pause => {}
        khora_core::ui::editor::icons::Icon::Stop => {}
        khora_core::ui::editor::icons::Icon::StepForward => {}
        khora_core::ui::editor::icons::Icon::Eye => {}
        khora_core::ui::editor::icons::Icon::EyeOff => {}
        khora_core::ui::editor::icons::Icon::Lock => {}
        khora_core::ui::editor::icons::Icon::Plus => {}
        khora_core::ui::editor::icons::Icon::More => {}
        khora_core::ui::editor::icons::Icon::Filter => {}
        khora_core::ui::editor::icons::Icon::Trash => {}
        khora_core::ui::editor::icons::Icon::Bell => {}
        khora_core::ui::editor::icons::Icon::Settings => {}
        khora_core::ui::editor::icons::Icon::Share => {}
        khora_core::ui::editor::icons::Icon::Hammer => {}
        khora_core::ui::editor::icons::Icon::Branch => {}
        khora_core::ui::editor::icons::Icon::Menu => {}
        khora_core::ui::editor::icons::Icon::Tag => {}
        khora_core::ui::editor::icons::Icon::Move => {}
        khora_core::ui::editor::icons::Icon::Rotate => {}
        khora_core::ui::editor::icons::Icon::Scale => {}
        khora_core::ui::editor::icons::Icon::Hand => {}
        khora_core::ui::editor::icons::Icon::Crosshair => {}
        khora_core::ui::editor::icons::Icon::Database => {}
        khora_core::ui::editor::icons::Icon::Film => {}
        khora_core::ui::editor::icons::Icon::Terminal => {}
        khora_core::ui::editor::icons::Icon::Code => {}
        khora_core::ui::editor::icons::Icon::Image => {}
        khora_core::ui::editor::icons::Icon::Music => {}
        khora_core::ui::editor::icons::Icon::Pen => {}
        khora_core::ui::editor::icons::Icon::Info => {}
        khora_core::ui::editor::icons::Icon::Warn => {}
        khora_core::ui::editor::icons::Icon::Error => {}
        khora_core::ui::editor::icons::Icon::Zap => {}
        khora_core::ui::editor::icons::Icon::Command => {}
        khora_core::ui::editor::icons::Icon::Wifi => {}
        khora_core::ui::editor::icons::Icon::Cpu => {}
        khora_core::ui::editor::icons::Icon::Memory => {}
        khora_core::ui::editor::icons::Icon::Check => {}
        khora_core::ui::editor::icons::Icon::Close => {}
        khora_core::ui::editor::icons::Icon::FolderOpen => {}
        khora_core::ui::editor::icons::Icon::Save => {}
        khora_core::ui::editor::icons::Icon::Copy => {}
        khora_core::ui::editor::icons::Icon::Pencil => {}
        khora_core::ui::editor::icons::Icon::Download => {}
        khora_core::ui::editor::icons::Icon::Refresh => {}
        khora_core::ui::editor::icons::Icon::Github => {}
        khora_core::ui::editor::icons::Icon::ArrowLeft => {}
        khora_core::ui::editor::icons::Icon::Box => {}
        khora_core::ui::editor::icons::Icon::Dot => {}
        khora_core::ui::editor::icons::Icon::Circle => {}
        khora_core::ui::editor::icons::Icon::CheckCircle => {}
    }
}

fn panel_location_variants(x: &khora_core::ui::editor::panel::PanelLocation) {
    match x {
        khora_core::ui::editor::panel::PanelLocation::TopBar => {}
        khora_core::ui::editor::panel::PanelLocation::Spine => {}
        khora_core::ui::editor::panel::PanelLocation::Left => {}
        khora_core::ui::editor::panel::PanelLocation::Right => {}
        khora_core::ui::editor::panel::PanelLocation::Bottom => {}
        khora_core::ui::editor::panel::PanelLocation::StatusBar => {}
        khora_core::ui::editor::panel::PanelLocation::Center => {}
        khora_core::ui::editor::panel::PanelLocation::Floating(..) => {}
    }
}

fn asset_entry_fields(x: &khora_core::ui::editor::state::AssetEntry) {
    let _ = (&x.name, &x.asset_type, &x.source_path);
}

fn component_json_fields(x: &khora_core::ui::editor::state::ComponentJson) {
    let _ = (&x.type_name, &x.domain, &x.value);
}

fn editor_mode_variants(x: &khora_core::ui::editor::state::EditorMode) {
    match x {
        khora_core::ui::editor::state::EditorMode::Scene => {}
        khora_core::ui::editor::state::EditorMode::ControlPlane => {}
    }
}

fn editor_state_fields(x: &khora_core::ui::editor::state::EditorState) {
    let _ = (
        &x.scene_roots,
        &x.selection,
        &x.entity_count,
        &x.search_filter,
        &x.ctrl_held,
        &x.pending_spawn,
        &x.pending_delete,
        &x.pending_duplicate,
        &x.renaming_entity,
        &x.rename_buffer,
        &x.pending_rename,
        &x.pending_reparent,
        &x.inspected,
        &x.pending_edits,
        &x.log_entries,
        &x.status,
        &x.asset_entries,
        &x.viewport_hovered,
        &x.viewport_screen_rect,
        &x.gizmo_mode,
        &x.selected_asset,
        &x.inspected_asset_path,
        &x.pending_menu_action,
        &x.project_folder,
        &x.project_name,
        &x.project_engine_version,
        &x.pending_browse_project_folder,
        &x.play_mode,
        &x.scene_snapshot,
        &x.current_scene_path,
        &x.pending_scene_load,
        &x.pending_add_component,
        &x.component_domain_registry,
        &x.active_mode,
        &x.command_palette_open,
        &x.inspector_card_open,
        &x.inspector_card_enabled,
        &x.current_git_branch,
        &x.hidden_entities,
        &x.pending_visibility_toggle,
        &x.pending_save_as_prefab,
        &x.pending_save_as_prefab_at,
        &x.pending_prefab_spawn,
        &x.pending_save_as_material,
        &x.pending_assign_material,
        &x.asset_dirs,
        &x.asset_epoch,
        &x.pending_create_folder,
        &x.pending_rename_asset,
        &x.pending_move_asset,
        &x.pending_delete_asset,
        &x.pending_duplicate_asset,
        &x.pending_spawn_mesh_asset,
        &x.pending_assign_texture,
    );
}

fn entity_icon_variants(x: &khora_core::ui::editor::state::EntityIcon) {
    match x {
        khora_core::ui::editor::state::EntityIcon::Empty => {}
        khora_core::ui::editor::state::EntityIcon::Camera => {}
        khora_core::ui::editor::state::EntityIcon::Light => {}
        khora_core::ui::editor::state::EntityIcon::Mesh => {}
        khora_core::ui::editor::state::EntityIcon::Audio => {}
    }
}

fn gizmo_mode_variants(x: &khora_core::ui::editor::state::GizmoMode) {
    match x {
        khora_core::ui::editor::state::GizmoMode::Select => {}
        khora_core::ui::editor::state::GizmoMode::Move => {}
        khora_core::ui::editor::state::GizmoMode::Rotate => {}
        khora_core::ui::editor::state::GizmoMode::Scale => {}
    }
}

fn inspected_entity_fields(x: &khora_core::ui::editor::state::InspectedEntity) {
    let _ = (&x.entity, &x.name, &x.components_json);
}

fn log_entry_fields(x: &khora_core::ui::editor::state::LogEntry) {
    let _ = (&x.level, &x.message, &x.target, &x.time);
}

fn log_level_variants(x: &khora_core::ui::editor::state::LogLevel) {
    match x {
        khora_core::ui::editor::state::LogLevel::Error => {}
        khora_core::ui::editor::state::LogLevel::Warn => {}
        khora_core::ui::editor::state::LogLevel::Info => {}
        khora_core::ui::editor::state::LogLevel::Debug => {}
        khora_core::ui::editor::state::LogLevel::Trace => {}
    }
}

fn play_mode_variants(x: &khora_core::ui::editor::state::PlayMode) {
    match x {
        khora_core::ui::editor::state::PlayMode::Editing => {}
        khora_core::ui::editor::state::PlayMode::Playing => {}
        khora_core::ui::editor::state::PlayMode::Paused => {}
    }
}

fn property_edit_variants(x: &khora_core::ui::editor::state::PropertyEdit) {
    match x {
        khora_core::ui::editor::state::PropertyEdit::SetName(..) => {}
        khora_core::ui::editor::state::PropertyEdit::SetComponentJson { .. } => {}
        khora_core::ui::editor::state::PropertyEdit::RemoveComponent { .. } => {}
    }
}

fn scene_node_fields(x: &khora_core::ui::editor::state::SceneNode) {
    let _ = (&x.entity, &x.name, &x.icon, &x.children, &x.tag_count);
}

fn status_bar_data_fields(x: &khora_core::ui::editor::state::StatusBarData) {
    let _ = (
        &x.fps,
        &x.frame_time_ms,
        &x.entity_count,
        &x.memory_used_mb,
        &x.draw_calls,
        &x.triangles,
        &x.vram_mb,
        &x.cpu_load,
        &x.gpu_load,
    );
}

fn font_family_hint_variants(x: &khora_core::ui::editor::ui_builder::FontFamilyHint) {
    match x {
        khora_core::ui::editor::ui_builder::FontFamilyHint::Proportional => {}
        khora_core::ui::editor::ui_builder::FontFamilyHint::Monospace => {}
        khora_core::ui::editor::ui_builder::FontFamilyHint::Display => {}
        khora_core::ui::editor::ui_builder::FontFamilyHint::Icons => {}
    }
}

fn inline_edit_event_variants(x: &khora_core::ui::editor::ui_builder::InlineEditEvent) {
    match x {
        khora_core::ui::editor::ui_builder::InlineEditEvent::Idle => {}
        khora_core::ui::editor::ui_builder::InlineEditEvent::Changed => {}
        khora_core::ui::editor::ui_builder::InlineEditEvent::Committed => {}
        khora_core::ui::editor::ui_builder::InlineEditEvent::Cancelled => {}
    }
}

fn interaction_fields(x: &khora_core::ui::editor::ui_builder::Interaction) {
    let _ = (
        &x.hovered,
        &x.clicked,
        &x.pressed,
        &x.double_clicked,
        &x.focused,
    );
}

fn text_align_variants(x: &khora_core::ui::editor::ui_builder::TextAlign) {
    match x {
        khora_core::ui::editor::ui_builder::TextAlign::Left => {}
        khora_core::ui::editor::ui_builder::TextAlign::Center => {}
        khora_core::ui::editor::ui_builder::TextAlign::Right => {}
    }
}

fn viewport_texture_handle_fields(
    x: &khora_core::ui::editor::viewport_texture::ViewportTextureHandle,
) {
    let _ = (&x.0,);
}

fn overlay_error_fields(x: &khora_core::ui::editor::overlay::OverlayError) {
    let _ = (&x.0,);
}

fn overlay_screen_descriptor_fields(x: &khora_core::ui::editor::overlay::OverlayScreenDescriptor) {
    let _ = (&x.width_px, &x.height_px, &x.scale_factor);
}

fn font_handle_variants(x: &khora_core::ui::fonts::FontHandle) {
    match x {
        khora_core::ui::fonts::FontHandle::Static(..) => {}
        khora_core::ui::fonts::FontHandle::Owned(..) => {}
    }
}

fn font_pack_fields(x: &khora_core::ui::fonts::FontPack) {
    let _ = (&x.proportional, &x.monospace, &x.display, &x.icons);
}

fn named_font_fields(x: &khora_core::ui::fonts::NamedFont) {
    let _ = (&x.name, &x.data);
}

fn align_variants(x: &khora_core::ui::geometry::Align) {
    match x {
        khora_core::ui::geometry::Align::Min => {}
        khora_core::ui::geometry::Align::Center => {}
        khora_core::ui::geometry::Align::Max => {}
    }
}

fn align2_fields(x: &khora_core::ui::geometry::Align2) {
    let _ = (&x.x, &x.y);
}

fn corner_radius_fields(x: &khora_core::ui::geometry::CornerRadius) {
    let _ = (&x.nw, &x.ne, &x.sw, &x.se);
}

fn margin_fields(x: &khora_core::ui::geometry::Margin) {
    let _ = (&x.top, &x.bottom, &x.left, &x.right);
}

fn stroke_fields(x: &khora_core::ui::geometry::Stroke) {
    let _ = (&x.color, &x.width);
}

fn ui_theme_fields(x: &khora_core::ui::theme::UiTheme) {
    let _ = (
        &x.background,
        &x.surface,
        &x.surface_elevated,
        &x.surface_interactive,
        &x.surface_active,
        &x.separator,
        &x.border,
        &x.border_strong,
        &x.text,
        &x.text_dim,
        &x.text_muted,
        &x.text_disabled,
        &x.text_inverse,
        &x.primary,
        &x.primary_dim,
        &x.accent_a,
        &x.accent_b,
        &x.accent_c,
        &x.success,
        &x.warning,
        &x.error,
        &x.axis_x,
        &x.axis_y,
        &x.axis_z,
        &x.radius_sm,
        &x.radius_md,
        &x.radius_lg,
        &x.radius_xl,
        &x.font_size_caption,
        &x.font_size_body,
        &x.font_size_title,
        &x.font_size_display,
        &x.pad_row,
        &x.pad_card,
    );
}

fn ui_border_fields(x: &khora_core::ui::types::UiBorder) {
    let _ = (&x.width, &x.color, &x.radius);
}

fn ui_color_fields(x: &khora_core::ui::types::UiColor) {
    let _ = (&x.0,);
}

fn ui_flex_direction_variants(x: &khora_core::ui::types::UiFlexDirection) {
    match x {
        khora_core::ui::types::UiFlexDirection::Column => {}
        khora_core::ui::types::UiFlexDirection::Row => {}
    }
}

fn ui_image_fields(x: &khora_core::ui::types::UiImage) {
    let _ = (&x.texture,);
}

fn ui_node_fields(x: &khora_core::ui::types::UiNode) {
    let _ = (
        &x.width,
        &x.height,
        &x.min_width,
        &x.min_height,
        &x.max_width,
        &x.max_height,
        &x.padding,
        &x.margin,
        &x.flex_direction,
        &x.flex_grow,
        &x.flex_shrink,
    );
}

fn ui_rect_fields(x: &khora_core::ui::types::UiRect<f32>) {
    let _ = (&x.left, &x.right, &x.top, &x.bottom);
}

fn ui_text_fields(x: &khora_core::ui::types::UiText) {
    let _ = (&x.content, &x.font, &x.size, &x.color);
}

fn ui_transform_fields(x: &khora_core::ui::types::UiTransform) {
    let _ = (&x.pos, &x.size, &x.z_index);
}

fn ui_val_variants(x: &khora_core::ui::types::UiVal) {
    match x {
        khora_core::ui::types::UiVal::Px(..) => {}
        khora_core::ui::types::UiVal::Percent(..) => {}
        khora_core::ui::types::UiVal::Auto => {}
    }
}

// ---------------------------------------------------------------------------
// Trait items, named through a generic parameter. A generic body is
// type-checked where it is written, so these need no implementor and are
// never instantiated. A re-exported trait is checked for identity by
// forwarding a bound both ways.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
mod trait_items {
    fn app_context_trait_items<T: khora_core::ui::app::app_context::AppContext>() {
        let _ = <T as khora_core::ui::app::app_context::AppContext>::central;
        let _ = <T as khora_core::ui::app::app_context::AppContext>::set_theme;
        let _ = <T as khora_core::ui::app::app_context::AppContext>::set_fonts;
        let _ = <T as khora_core::ui::app::app_context::AppContext>::screen_size;
        let _ = <T as khora_core::ui::app::app_context::AppContext>::pixels_per_point;
        let _ = <T as khora_core::ui::app::app_context::AppContext>::request_repaint;
        let _ = <T as khora_core::ui::app::app_context::AppContext>::request_close;
    }

    fn app_context_trait_identity<T: khora_core::ui::app::AppContext>() {
        app_context_trait_items::<T>();
    }

    fn app_context_trait_identity_rev<T: khora_core::ui::app::app_context::AppContext>() {
        app_context_trait_identity::<T>();
    }

    fn app_context_trait_identity_2<T: khora_core::ui::AppContext>() {
        app_context_trait_items::<T>();
    }

    fn app_context_trait_identity_2_rev<T: khora_core::ui::app::app_context::AppContext>() {
        app_context_trait_identity_2::<T>();
    }

    fn app_trait_items<T: khora_core::ui::app::runtime::App>() {
        let _ = <T as khora_core::ui::app::runtime::App>::update;
        let _ = <T as khora_core::ui::app::runtime::App>::on_start;
        let _ = <T as khora_core::ui::app::runtime::App>::on_exit;
    }

    fn app_trait_identity<T: khora_core::ui::app::App>() {
        app_trait_items::<T>();
    }

    fn app_trait_identity_rev<T: khora_core::ui::app::runtime::App>() {
        app_trait_identity::<T>();
    }

    fn app_trait_identity_2<T: khora_core::ui::App>() {
        app_trait_items::<T>();
    }

    fn app_trait_identity_2_rev<T: khora_core::ui::app::runtime::App>() {
        app_trait_identity_2::<T>();
    }

    fn app_lifecycle_trait_items<T: khora_core::ui::app::runtime::AppLifecycle>() {
        let _ = <T as khora_core::ui::app::runtime::AppLifecycle>::on_start;
        let _ = <T as khora_core::ui::app::runtime::AppLifecycle>::on_exit;
    }

    fn app_lifecycle_trait_identity<T: khora_core::ui::app::AppLifecycle>() {
        app_lifecycle_trait_items::<T>();
    }

    fn app_lifecycle_trait_identity_rev<T: khora_core::ui::app::runtime::AppLifecycle>() {
        app_lifecycle_trait_identity::<T>();
    }

    fn app_lifecycle_trait_identity_2<T: khora_core::ui::AppLifecycle>() {
        app_lifecycle_trait_items::<T>();
    }

    fn app_lifecycle_trait_identity_2_rev<T: khora_core::ui::app::runtime::AppLifecycle>() {
        app_lifecycle_trait_identity_2::<T>();
    }

    fn editor_panel_trait_items<T: khora_core::ui::editor::panel::EditorPanel>() {
        let _ = <T as khora_core::ui::editor::panel::EditorPanel>::id;
        let _ = <T as khora_core::ui::editor::panel::EditorPanel>::title;
        let _ = <T as khora_core::ui::editor::panel::EditorPanel>::ui;
        let _ = <T as khora_core::ui::editor::panel::EditorPanel>::preferred_size;
    }

    fn editor_panel_trait_identity<T: khora_core::ui::editor::EditorPanel>() {
        editor_panel_trait_items::<T>();
    }

    fn editor_panel_trait_identity_rev<T: khora_core::ui::editor::panel::EditorPanel>() {
        editor_panel_trait_identity::<T>();
    }

    fn editor_panel_trait_identity_2<T: khora_core::ui::EditorPanel>() {
        editor_panel_trait_items::<T>();
    }

    fn editor_panel_trait_identity_2_rev<T: khora_core::ui::editor::panel::EditorPanel>() {
        editor_panel_trait_identity_2::<T>();
    }

    fn editor_shell_trait_items<T: khora_core::ui::editor::shell::EditorShell>() {
        let _ = <T as khora_core::ui::editor::shell::EditorShell>::register_panel;
        let _ = <T as khora_core::ui::editor::shell::EditorShell>::remove_panel;
        let _ = <T as khora_core::ui::editor::shell::EditorShell>::set_theme;
        let _ = <T as khora_core::ui::editor::shell::EditorShell>::set_fonts;
        let _ = <T as khora_core::ui::editor::shell::EditorShell>::set_status;
        let _ = <T as khora_core::ui::editor::shell::EditorShell>::set_editor_state;
        let _ = <T as khora_core::ui::editor::shell::EditorShell>::show_frame;
    }

    fn editor_shell_trait_identity<T: khora_core::ui::editor::EditorShell>() {
        editor_shell_trait_items::<T>();
    }

    fn editor_shell_trait_identity_rev<T: khora_core::ui::editor::shell::EditorShell>() {
        editor_shell_trait_identity::<T>();
    }

    fn editor_shell_trait_identity_2<T: khora_core::ui::EditorShell>() {
        editor_shell_trait_items::<T>();
    }

    fn editor_shell_trait_identity_2_rev<T: khora_core::ui::editor::shell::EditorShell>() {
        editor_shell_trait_identity_2::<T>();
    }

    fn ui_builder_trait_items<T: khora_core::ui::editor::ui_builder::UiBuilder>() {
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::heading;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::label;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::colored_label;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::small_label;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::monospace;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::button;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::small_button;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::selectable_label;
        let _ =
            <T as khora_core::ui::editor::ui_builder::UiBuilder>::selectable_label_double_clicked;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::checkbox;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::drag_value_f32;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::slider_f32;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::text_edit_singleline;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::vec3_editor;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::color_edit;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::combo_box;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::horizontal;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::vertical;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::collapsing;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::indent;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::scroll_area;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::top_inset_panel;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::bottom_inset_panel;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::left_inset_panel;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::right_inset_panel;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::central_inset;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::frame_box;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::modal;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::separator;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::spacing;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::is_last_item_double_clicked;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::is_last_item_hovered;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::is_last_item_enter_pressed;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::is_last_item_escape_pressed;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::is_last_item_dragged;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::key_pressed;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::keyboard_captured;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::raw_key_pressed;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::focus_last_item;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::push_clip_rect;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::pop_clip_rect;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::scroll_delta_in;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::context_menu_last;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::context_menu_panel;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::close_menu;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::menu_button;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::paint_line;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::paint_rect_filled;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::paint_text;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::paint_rect_stroke;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::paint_circle_filled;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::paint_circle_stroke;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::paint_text_styled;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::paint_path_filled;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::interact_rect;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::dnd_attach_drag_payload;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::dnd_take_drop_payload;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::pointer_position;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::is_drag_active;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::inline_text_field;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::overlay_rect_filled;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::overlay_rect_stroke;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::overlay_text;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::tooltip_for_last;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::region_at;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::cursor_pos;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::allocate_size;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::measure_text;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::available_width;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::available_height;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::panel_rect;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::screen_rect;
        let _ = <T as khora_core::ui::editor::ui_builder::UiBuilder>::viewport_image;
    }

    fn ui_builder_trait_identity<T: khora_core::ui::editor::UiBuilder>() {
        ui_builder_trait_items::<T>();
    }

    fn ui_builder_trait_identity_rev<T: khora_core::ui::editor::ui_builder::UiBuilder>() {
        ui_builder_trait_identity::<T>();
    }

    fn ui_builder_trait_identity_2<T: khora_core::ui::UiBuilder>() {
        ui_builder_trait_items::<T>();
    }

    fn ui_builder_trait_identity_2_rev<T: khora_core::ui::editor::ui_builder::UiBuilder>() {
        ui_builder_trait_identity_2::<T>();
    }

    fn editor_overlay_trait_items<T: khora_core::ui::editor::overlay::EditorOverlay>() {
        let _ = <T as khora_core::ui::editor::overlay::EditorOverlay>::handle_window_event;
        let _ = <T as khora_core::ui::editor::overlay::EditorOverlay>::begin_frame;
        let _ = <T as khora_core::ui::editor::overlay::EditorOverlay>::ui_context;
        let _ = <T as khora_core::ui::editor::overlay::EditorOverlay>::end_frame_and_render;
        let _ = <T as khora_core::ui::editor::overlay::EditorOverlay>::wants_pointer_input;
        let _ = <T as khora_core::ui::editor::overlay::EditorOverlay>::wants_keyboard_input;
        let _ = <T as khora_core::ui::editor::overlay::EditorOverlay>::as_any;
        let _ = <T as khora_core::ui::editor::overlay::EditorOverlay>::as_any_mut;
    }

    fn editor_overlay_trait_identity<T: khora_core::ui::EditorOverlay>() {
        editor_overlay_trait_items::<T>();
    }

    fn editor_overlay_trait_identity_rev<T: khora_core::ui::editor::overlay::EditorOverlay>() {
        editor_overlay_trait_identity::<T>();
    }

    fn editor_overlay_trait_identity_editor<T: khora_core::ui::editor::EditorOverlay>() {
        editor_overlay_trait_items::<T>();
    }

    fn editor_overlay_trait_identity_editor_rev<
        T: khora_core::ui::editor::overlay::EditorOverlay,
    >() {
        editor_overlay_trait_identity_editor::<T>();
    }

    fn layout_system_trait_items<T: khora_core::ui::layout::LayoutSystem>() {
        let _ = <T as khora_core::ui::layout::LayoutSystem>::compute_layouts;
        let _ = <T as khora_core::ui::layout::LayoutSystem>::as_any;
        let _ = <T as khora_core::ui::layout::LayoutSystem>::as_any_mut;
    }

    fn layout_system_trait_identity<T: khora_core::ui::LayoutSystem>() {
        layout_system_trait_items::<T>();
    }

    fn layout_system_trait_identity_rev<T: khora_core::ui::layout::LayoutSystem>() {
        layout_system_trait_identity::<T>();
    }

    fn ui_layout_view_trait_items<T: khora_core::ui::layout::UiLayoutView>() {
        let _ = <T as khora_core::ui::layout::UiLayoutView>::get_all_ui_entities;
        let _ = <T as khora_core::ui::layout::UiLayoutView>::get_node;
        let _ = <T as khora_core::ui::layout::UiLayoutView>::get_children;
        let _ = <T as khora_core::ui::layout::UiLayoutView>::has_parent;
        let _ = <T as khora_core::ui::layout::UiLayoutView>::set_transform;
        let _ = <T as khora_core::ui::layout::UiLayoutView>::viewport_size;
    }

    fn ui_layout_view_trait_identity<T: khora_core::ui::UiLayoutView>() {
        ui_layout_view_trait_items::<T>();
    }

    fn ui_layout_view_trait_identity_rev<T: khora_core::ui::layout::UiLayoutView>() {
        ui_layout_view_trait_identity::<T>();
    }
}

#[test]
fn module_ui_paths_still_resolve() {
    // trait `khora_core::ui::app::app_context::AppContext`: see `app_context_trait_items`
    // trait `khora_core::ui::app::AppContext`: see `app_context_trait_items`
    // trait `khora_core::ui::AppContext`: see `app_context_trait_items`
    // trait `khora_core::ui::app::runtime::App`: see `app_trait_items`
    // trait `khora_core::ui::app::App`: see `app_trait_items`
    // trait `khora_core::ui::App`: see `app_trait_items`
    // trait `khora_core::ui::app::runtime::AppLifecycle`: see `app_lifecycle_trait_items`
    // trait `khora_core::ui::app::AppLifecycle`: see `app_lifecycle_trait_items`
    // trait `khora_core::ui::AppLifecycle`: see `app_lifecycle_trait_items`
    let _ = type_name::<khora_core::ui::editor::gizmo::GizmoLineInstance>();
    let _ = type_name::<khora_core::ui::editor::GizmoLineInstance>();
    same_type(
        PhantomData::<khora_core::ui::editor::GizmoLineInstance>,
        PhantomData::<khora_core::ui::editor::gizmo::GizmoLineInstance>,
    );
    let _ = gizmo_line_instance_fields as fn(&khora_core::ui::editor::gizmo::GizmoLineInstance);
    let _ = khora_core::ui::editor::gizmo::GizmoLineInstance::new;
    is_debug::<khora_core::ui::editor::gizmo::GizmoLineInstance>();
    is_clone::<khora_core::ui::editor::gizmo::GizmoLineInstance>();
    is_copy::<khora_core::ui::editor::gizmo::GizmoLineInstance>();
    is_pod::<khora_core::ui::editor::gizmo::GizmoLineInstance>();
    is_zeroable::<khora_core::ui::editor::gizmo::GizmoLineInstance>();
    let _ = type_name::<khora_core::ui::editor::icons::Icon>();
    let _ = type_name::<khora_core::ui::editor::Icon>();
    same_type(
        PhantomData::<khora_core::ui::editor::Icon>,
        PhantomData::<khora_core::ui::editor::icons::Icon>,
    );
    let _ = icon_variants as fn(&khora_core::ui::editor::icons::Icon);
    let _ = khora_core::ui::editor::icons::Icon::glyph;
    is_debug::<khora_core::ui::editor::icons::Icon>();
    is_clone::<khora_core::ui::editor::icons::Icon>();
    is_copy::<khora_core::ui::editor::icons::Icon>();
    is_partial_eq::<khora_core::ui::editor::icons::Icon>();
    is_eq::<khora_core::ui::editor::icons::Icon>();
    is_hash::<khora_core::ui::editor::icons::Icon>();
    // trait `khora_core::ui::editor::panel::EditorPanel`: see `editor_panel_trait_items`
    // trait `khora_core::ui::editor::EditorPanel`: see `editor_panel_trait_items`
    // trait `khora_core::ui::EditorPanel`: see `editor_panel_trait_items`
    let _ = type_name::<khora_core::ui::editor::panel::PanelLocation>();
    let _ = type_name::<khora_core::ui::editor::PanelLocation>();
    let _ = type_name::<khora_core::ui::PanelLocation>();
    same_type(
        PhantomData::<khora_core::ui::editor::PanelLocation>,
        PhantomData::<khora_core::ui::editor::panel::PanelLocation>,
    );
    same_type(
        PhantomData::<khora_core::ui::PanelLocation>,
        PhantomData::<khora_core::ui::editor::panel::PanelLocation>,
    );
    let _ = panel_location_variants as fn(&khora_core::ui::editor::panel::PanelLocation);
    is_debug::<khora_core::ui::editor::panel::PanelLocation>();
    is_clone::<khora_core::ui::editor::panel::PanelLocation>();
    is_copy::<khora_core::ui::editor::panel::PanelLocation>();
    is_partial_eq::<khora_core::ui::editor::panel::PanelLocation>();
    is_eq::<khora_core::ui::editor::panel::PanelLocation>();
    is_hash::<khora_core::ui::editor::panel::PanelLocation>();
    // trait `khora_core::ui::editor::shell::EditorShell`: see `editor_shell_trait_items`
    // trait `khora_core::ui::editor::EditorShell`: see `editor_shell_trait_items`
    // trait `khora_core::ui::EditorShell`: see `editor_shell_trait_items`
    let _ = type_name::<khora_core::ui::editor::state::AssetEntry>();
    let _ = type_name::<khora_core::ui::editor::AssetEntry>();
    same_type(
        PhantomData::<khora_core::ui::editor::AssetEntry>,
        PhantomData::<khora_core::ui::editor::state::AssetEntry>,
    );
    let _ = asset_entry_fields as fn(&khora_core::ui::editor::state::AssetEntry);
    is_debug::<khora_core::ui::editor::state::AssetEntry>();
    is_clone::<khora_core::ui::editor::state::AssetEntry>();
    let _ = type_name::<khora_core::ui::editor::state::ComponentJson>();
    let _ = type_name::<khora_core::ui::editor::ComponentJson>();
    same_type(
        PhantomData::<khora_core::ui::editor::ComponentJson>,
        PhantomData::<khora_core::ui::editor::state::ComponentJson>,
    );
    let _ = component_json_fields as fn(&khora_core::ui::editor::state::ComponentJson);
    is_debug::<khora_core::ui::editor::state::ComponentJson>();
    is_clone::<khora_core::ui::editor::state::ComponentJson>();
    let _ = type_name::<khora_core::ui::editor::state::EditorMode>();
    let _ = type_name::<khora_core::ui::editor::EditorMode>();
    same_type(
        PhantomData::<khora_core::ui::editor::EditorMode>,
        PhantomData::<khora_core::ui::editor::state::EditorMode>,
    );
    let _ = editor_mode_variants as fn(&khora_core::ui::editor::state::EditorMode);
    is_debug::<khora_core::ui::editor::state::EditorMode>();
    is_clone::<khora_core::ui::editor::state::EditorMode>();
    is_copy::<khora_core::ui::editor::state::EditorMode>();
    is_partial_eq::<khora_core::ui::editor::state::EditorMode>();
    is_eq::<khora_core::ui::editor::state::EditorMode>();
    is_hash::<khora_core::ui::editor::state::EditorMode>();
    is_default::<khora_core::ui::editor::state::EditorMode>();
    let _ = type_name::<khora_core::ui::editor::state::EditorState>();
    let _ = type_name::<khora_core::ui::editor::EditorState>();
    same_type(
        PhantomData::<khora_core::ui::editor::EditorState>,
        PhantomData::<khora_core::ui::editor::state::EditorState>,
    );
    let _ = editor_state_fields as fn(&khora_core::ui::editor::state::EditorState);
    let _ = khora_core::ui::editor::state::EditorState::is_selected;
    let _ = khora_core::ui::editor::state::EditorState::select;
    let _ = khora_core::ui::editor::state::EditorState::toggle_select;
    let _ = khora_core::ui::editor::state::EditorState::clear_selection;
    let _ = khora_core::ui::editor::state::EditorState::clear_entity_references;
    let _ = khora_core::ui::editor::state::EditorState::single_selected;
    let _ = khora_core::ui::editor::state::EditorState::push_edit;
    let _ = khora_core::ui::editor::state::EditorState::drain_edits;
    is_debug::<khora_core::ui::editor::state::EditorState>();
    is_clone::<khora_core::ui::editor::state::EditorState>();
    is_default::<khora_core::ui::editor::state::EditorState>();
    let _ = type_name::<khora_core::ui::editor::state::EntityIcon>();
    let _ = type_name::<khora_core::ui::editor::EntityIcon>();
    same_type(
        PhantomData::<khora_core::ui::editor::EntityIcon>,
        PhantomData::<khora_core::ui::editor::state::EntityIcon>,
    );
    let _ = entity_icon_variants as fn(&khora_core::ui::editor::state::EntityIcon);
    is_debug::<khora_core::ui::editor::state::EntityIcon>();
    is_clone::<khora_core::ui::editor::state::EntityIcon>();
    is_copy::<khora_core::ui::editor::state::EntityIcon>();
    is_partial_eq::<khora_core::ui::editor::state::EntityIcon>();
    is_eq::<khora_core::ui::editor::state::EntityIcon>();
    let _ = type_name::<khora_core::ui::editor::state::GizmoMode>();
    let _ = type_name::<khora_core::ui::editor::GizmoMode>();
    same_type(
        PhantomData::<khora_core::ui::editor::GizmoMode>,
        PhantomData::<khora_core::ui::editor::state::GizmoMode>,
    );
    let _ = gizmo_mode_variants as fn(&khora_core::ui::editor::state::GizmoMode);
    is_debug::<khora_core::ui::editor::state::GizmoMode>();
    is_clone::<khora_core::ui::editor::state::GizmoMode>();
    is_copy::<khora_core::ui::editor::state::GizmoMode>();
    is_partial_eq::<khora_core::ui::editor::state::GizmoMode>();
    is_eq::<khora_core::ui::editor::state::GizmoMode>();
    is_default::<khora_core::ui::editor::state::GizmoMode>();
    let _ = type_name::<khora_core::ui::editor::state::InspectedEntity>();
    let _ = type_name::<khora_core::ui::editor::InspectedEntity>();
    same_type(
        PhantomData::<khora_core::ui::editor::InspectedEntity>,
        PhantomData::<khora_core::ui::editor::state::InspectedEntity>,
    );
    let _ = inspected_entity_fields as fn(&khora_core::ui::editor::state::InspectedEntity);
    is_debug::<khora_core::ui::editor::state::InspectedEntity>();
    is_clone::<khora_core::ui::editor::state::InspectedEntity>();
    let _ = type_name::<khora_core::ui::editor::state::LogEntry>();
    let _ = type_name::<khora_core::ui::editor::LogEntry>();
    same_type(
        PhantomData::<khora_core::ui::editor::LogEntry>,
        PhantomData::<khora_core::ui::editor::state::LogEntry>,
    );
    let _ = log_entry_fields as fn(&khora_core::ui::editor::state::LogEntry);
    is_debug::<khora_core::ui::editor::state::LogEntry>();
    is_clone::<khora_core::ui::editor::state::LogEntry>();
    let _ = type_name::<khora_core::ui::editor::state::LogLevel>();
    let _ = type_name::<khora_core::ui::editor::LogLevel>();
    same_type(
        PhantomData::<khora_core::ui::editor::LogLevel>,
        PhantomData::<khora_core::ui::editor::state::LogLevel>,
    );
    let _ = log_level_variants as fn(&khora_core::ui::editor::state::LogLevel);
    is_debug::<khora_core::ui::editor::state::LogLevel>();
    is_clone::<khora_core::ui::editor::state::LogLevel>();
    is_copy::<khora_core::ui::editor::state::LogLevel>();
    is_partial_eq::<khora_core::ui::editor::state::LogLevel>();
    is_eq::<khora_core::ui::editor::state::LogLevel>();
    let _ = type_name::<khora_core::ui::editor::state::PlayMode>();
    let _ = type_name::<khora_core::ui::editor::PlayMode>();
    same_type(
        PhantomData::<khora_core::ui::editor::PlayMode>,
        PhantomData::<khora_core::ui::editor::state::PlayMode>,
    );
    let _ = play_mode_variants as fn(&khora_core::ui::editor::state::PlayMode);
    is_debug::<khora_core::ui::editor::state::PlayMode>();
    is_clone::<khora_core::ui::editor::state::PlayMode>();
    is_copy::<khora_core::ui::editor::state::PlayMode>();
    is_partial_eq::<khora_core::ui::editor::state::PlayMode>();
    is_eq::<khora_core::ui::editor::state::PlayMode>();
    is_default::<khora_core::ui::editor::state::PlayMode>();
    let _ = type_name::<khora_core::ui::editor::state::PropertyEdit>();
    let _ = type_name::<khora_core::ui::editor::PropertyEdit>();
    same_type(
        PhantomData::<khora_core::ui::editor::PropertyEdit>,
        PhantomData::<khora_core::ui::editor::state::PropertyEdit>,
    );
    let _ = property_edit_variants as fn(&khora_core::ui::editor::state::PropertyEdit);
    is_debug::<khora_core::ui::editor::state::PropertyEdit>();
    is_clone::<khora_core::ui::editor::state::PropertyEdit>();
    let _ = type_name::<khora_core::ui::editor::state::SceneNode>();
    let _ = type_name::<khora_core::ui::editor::SceneNode>();
    same_type(
        PhantomData::<khora_core::ui::editor::SceneNode>,
        PhantomData::<khora_core::ui::editor::state::SceneNode>,
    );
    let _ = scene_node_fields as fn(&khora_core::ui::editor::state::SceneNode);
    is_debug::<khora_core::ui::editor::state::SceneNode>();
    is_clone::<khora_core::ui::editor::state::SceneNode>();
    let _ = type_name::<khora_core::ui::editor::state::StatusBarData>();
    let _ = type_name::<khora_core::ui::editor::StatusBarData>();
    same_type(
        PhantomData::<khora_core::ui::editor::StatusBarData>,
        PhantomData::<khora_core::ui::editor::state::StatusBarData>,
    );
    let _ = status_bar_data_fields as fn(&khora_core::ui::editor::state::StatusBarData);
    is_debug::<khora_core::ui::editor::state::StatusBarData>();
    is_clone::<khora_core::ui::editor::state::StatusBarData>();
    is_default::<khora_core::ui::editor::state::StatusBarData>();
    let _ = type_name::<khora_core::ui::editor::ui_builder::FontFamilyHint>();
    let _ = type_name::<khora_core::ui::editor::FontFamilyHint>();
    same_type(
        PhantomData::<khora_core::ui::editor::FontFamilyHint>,
        PhantomData::<khora_core::ui::editor::ui_builder::FontFamilyHint>,
    );
    let _ = font_family_hint_variants as fn(&khora_core::ui::editor::ui_builder::FontFamilyHint);
    is_debug::<khora_core::ui::editor::ui_builder::FontFamilyHint>();
    is_clone::<khora_core::ui::editor::ui_builder::FontFamilyHint>();
    is_copy::<khora_core::ui::editor::ui_builder::FontFamilyHint>();
    is_partial_eq::<khora_core::ui::editor::ui_builder::FontFamilyHint>();
    is_eq::<khora_core::ui::editor::ui_builder::FontFamilyHint>();
    let _ = type_name::<khora_core::ui::editor::ui_builder::InlineEditEvent>();
    let _ = type_name::<khora_core::ui::editor::InlineEditEvent>();
    same_type(
        PhantomData::<khora_core::ui::editor::InlineEditEvent>,
        PhantomData::<khora_core::ui::editor::ui_builder::InlineEditEvent>,
    );
    let _ = inline_edit_event_variants as fn(&khora_core::ui::editor::ui_builder::InlineEditEvent);
    is_debug::<khora_core::ui::editor::ui_builder::InlineEditEvent>();
    is_clone::<khora_core::ui::editor::ui_builder::InlineEditEvent>();
    is_copy::<khora_core::ui::editor::ui_builder::InlineEditEvent>();
    is_partial_eq::<khora_core::ui::editor::ui_builder::InlineEditEvent>();
    is_eq::<khora_core::ui::editor::ui_builder::InlineEditEvent>();
    is_default::<khora_core::ui::editor::ui_builder::InlineEditEvent>();
    let _ = type_name::<khora_core::ui::editor::ui_builder::Interaction>();
    let _ = type_name::<khora_core::ui::editor::Interaction>();
    same_type(
        PhantomData::<khora_core::ui::editor::Interaction>,
        PhantomData::<khora_core::ui::editor::ui_builder::Interaction>,
    );
    let _ = interaction_fields as fn(&khora_core::ui::editor::ui_builder::Interaction);
    is_debug::<khora_core::ui::editor::ui_builder::Interaction>();
    is_clone::<khora_core::ui::editor::ui_builder::Interaction>();
    is_copy::<khora_core::ui::editor::ui_builder::Interaction>();
    is_default::<khora_core::ui::editor::ui_builder::Interaction>();
    let _ = type_name::<khora_core::ui::editor::ui_builder::TextAlign>();
    let _ = type_name::<khora_core::ui::editor::TextAlign>();
    same_type(
        PhantomData::<khora_core::ui::editor::TextAlign>,
        PhantomData::<khora_core::ui::editor::ui_builder::TextAlign>,
    );
    let _ = text_align_variants as fn(&khora_core::ui::editor::ui_builder::TextAlign);
    is_debug::<khora_core::ui::editor::ui_builder::TextAlign>();
    is_clone::<khora_core::ui::editor::ui_builder::TextAlign>();
    is_copy::<khora_core::ui::editor::ui_builder::TextAlign>();
    is_partial_eq::<khora_core::ui::editor::ui_builder::TextAlign>();
    is_eq::<khora_core::ui::editor::ui_builder::TextAlign>();
    // trait `khora_core::ui::editor::ui_builder::UiBuilder`: see `ui_builder_trait_items`
    // trait `khora_core::ui::editor::UiBuilder`: see `ui_builder_trait_items`
    // trait `khora_core::ui::UiBuilder`: see `ui_builder_trait_items`
    let _ = type_name::<khora_core::ui::editor::viewport_texture::ViewportTextureHandle>();
    let _ = type_name::<khora_core::ui::editor::ViewportTextureHandle>();
    let _ = type_name::<khora_core::ui::ViewportTextureHandle>();
    same_type(
        PhantomData::<khora_core::ui::editor::ViewportTextureHandle>,
        PhantomData::<khora_core::ui::editor::viewport_texture::ViewportTextureHandle>,
    );
    same_type(
        PhantomData::<khora_core::ui::ViewportTextureHandle>,
        PhantomData::<khora_core::ui::editor::viewport_texture::ViewportTextureHandle>,
    );
    let _ = viewport_texture_handle_fields
        as fn(&khora_core::ui::editor::viewport_texture::ViewportTextureHandle);
    is_debug::<khora_core::ui::editor::viewport_texture::ViewportTextureHandle>();
    is_clone::<khora_core::ui::editor::viewport_texture::ViewportTextureHandle>();
    is_copy::<khora_core::ui::editor::viewport_texture::ViewportTextureHandle>();
    is_partial_eq::<khora_core::ui::editor::viewport_texture::ViewportTextureHandle>();
    is_eq::<khora_core::ui::editor::viewport_texture::ViewportTextureHandle>();
    is_hash::<khora_core::ui::editor::viewport_texture::ViewportTextureHandle>();
    // trait `khora_core::ui::editor::overlay::EditorOverlay`: see `editor_overlay_trait_items`
    // trait `khora_core::ui::EditorOverlay`: see `editor_overlay_trait_items`
    // trait `khora_core::ui::editor::EditorOverlay`: see `editor_overlay_trait_items`
    let _ = type_name::<khora_core::ui::editor::overlay::OverlayError>();
    let _ = type_name::<khora_core::ui::OverlayError>();
    same_type(
        PhantomData::<khora_core::ui::OverlayError>,
        PhantomData::<khora_core::ui::editor::overlay::OverlayError>,
    );
    let _ = type_name::<khora_core::ui::editor::OverlayError>();
    same_type(
        PhantomData::<khora_core::ui::editor::OverlayError>,
        PhantomData::<khora_core::ui::editor::overlay::OverlayError>,
    );
    let _ = overlay_error_fields as fn(&khora_core::ui::editor::overlay::OverlayError);
    is_debug::<khora_core::ui::editor::overlay::OverlayError>();
    is_display::<khora_core::ui::editor::overlay::OverlayError>();
    is_error::<khora_core::ui::editor::overlay::OverlayError>();
    let _ = type_name::<khora_core::ui::editor::overlay::OverlayScreenDescriptor>();
    let _ = type_name::<khora_core::ui::OverlayScreenDescriptor>();
    same_type(
        PhantomData::<khora_core::ui::OverlayScreenDescriptor>,
        PhantomData::<khora_core::ui::editor::overlay::OverlayScreenDescriptor>,
    );
    let _ = type_name::<khora_core::ui::editor::OverlayScreenDescriptor>();
    same_type(
        PhantomData::<khora_core::ui::editor::OverlayScreenDescriptor>,
        PhantomData::<khora_core::ui::editor::overlay::OverlayScreenDescriptor>,
    );
    let _ = overlay_screen_descriptor_fields
        as fn(&khora_core::ui::editor::overlay::OverlayScreenDescriptor);
    is_debug::<khora_core::ui::editor::overlay::OverlayScreenDescriptor>();
    is_clone::<khora_core::ui::editor::overlay::OverlayScreenDescriptor>();
    is_copy::<khora_core::ui::editor::overlay::OverlayScreenDescriptor>();
    let _ = type_name::<khora_core::ui::fonts::FontHandle>();
    let _ = type_name::<khora_core::ui::FontHandle>();
    same_type(
        PhantomData::<khora_core::ui::FontHandle>,
        PhantomData::<khora_core::ui::fonts::FontHandle>,
    );
    let _ = font_handle_variants as fn(&khora_core::ui::fonts::FontHandle);
    let _ = khora_core::ui::fonts::FontHandle::as_bytes;
    is_debug::<khora_core::ui::fonts::FontHandle>();
    is_clone::<khora_core::ui::fonts::FontHandle>();
    let _ = type_name::<khora_core::ui::fonts::FontPack>();
    let _ = type_name::<khora_core::ui::FontPack>();
    same_type(
        PhantomData::<khora_core::ui::FontPack>,
        PhantomData::<khora_core::ui::fonts::FontPack>,
    );
    let _ = font_pack_fields as fn(&khora_core::ui::fonts::FontPack);
    let _ = khora_core::ui::fonts::FontPack::is_empty;
    is_debug::<khora_core::ui::fonts::FontPack>();
    is_clone::<khora_core::ui::fonts::FontPack>();
    is_default::<khora_core::ui::fonts::FontPack>();
    let _ = type_name::<khora_core::ui::fonts::NamedFont>();
    let _ = type_name::<khora_core::ui::NamedFont>();
    same_type(
        PhantomData::<khora_core::ui::NamedFont>,
        PhantomData::<khora_core::ui::fonts::NamedFont>,
    );
    let _ = named_font_fields as fn(&khora_core::ui::fonts::NamedFont);
    is_debug::<khora_core::ui::fonts::NamedFont>();
    is_clone::<khora_core::ui::fonts::NamedFont>();
    let _ = type_name::<khora_core::ui::geometry::Align>();
    let _ = type_name::<khora_core::ui::Align>();
    same_type(
        PhantomData::<khora_core::ui::Align>,
        PhantomData::<khora_core::ui::geometry::Align>,
    );
    let _ = align_variants as fn(&khora_core::ui::geometry::Align);
    is_debug::<khora_core::ui::geometry::Align>();
    is_clone::<khora_core::ui::geometry::Align>();
    is_copy::<khora_core::ui::geometry::Align>();
    is_partial_eq::<khora_core::ui::geometry::Align>();
    is_eq::<khora_core::ui::geometry::Align>();
    is_default::<khora_core::ui::geometry::Align>();
    let _ = type_name::<khora_core::ui::geometry::Align2>();
    let _ = type_name::<khora_core::ui::Align2>();
    same_type(
        PhantomData::<khora_core::ui::Align2>,
        PhantomData::<khora_core::ui::geometry::Align2>,
    );
    let _ = align2_fields as fn(&khora_core::ui::geometry::Align2);
    let _ = khora_core::ui::geometry::Align2::LEFT_TOP;
    let _ = khora_core::ui::geometry::Align2::CENTER_TOP;
    let _ = khora_core::ui::geometry::Align2::RIGHT_TOP;
    let _ = khora_core::ui::geometry::Align2::LEFT_CENTER;
    let _ = khora_core::ui::geometry::Align2::CENTER_CENTER;
    let _ = khora_core::ui::geometry::Align2::RIGHT_CENTER;
    let _ = khora_core::ui::geometry::Align2::LEFT_BOTTOM;
    let _ = khora_core::ui::geometry::Align2::CENTER_BOTTOM;
    let _ = khora_core::ui::geometry::Align2::RIGHT_BOTTOM;
    is_debug::<khora_core::ui::geometry::Align2>();
    is_clone::<khora_core::ui::geometry::Align2>();
    is_copy::<khora_core::ui::geometry::Align2>();
    is_partial_eq::<khora_core::ui::geometry::Align2>();
    is_eq::<khora_core::ui::geometry::Align2>();
    is_default::<khora_core::ui::geometry::Align2>();
    let _ = type_name::<khora_core::ui::geometry::CornerRadius>();
    let _ = type_name::<khora_core::ui::CornerRadius>();
    same_type(
        PhantomData::<khora_core::ui::CornerRadius>,
        PhantomData::<khora_core::ui::geometry::CornerRadius>,
    );
    let _ = corner_radius_fields as fn(&khora_core::ui::geometry::CornerRadius);
    let _ = khora_core::ui::geometry::CornerRadius::same;
    let _ = khora_core::ui::geometry::CornerRadius::ZERO;
    let _ = khora_core::ui::geometry::CornerRadius::max;
    is_debug::<khora_core::ui::geometry::CornerRadius>();
    is_clone::<khora_core::ui::geometry::CornerRadius>();
    is_copy::<khora_core::ui::geometry::CornerRadius>();
    is_partial_eq::<khora_core::ui::geometry::CornerRadius>();
    is_default::<khora_core::ui::geometry::CornerRadius>();
    let _ = type_name::<khora_core::ui::geometry::Margin>();
    let _ = type_name::<khora_core::ui::Margin>();
    same_type(
        PhantomData::<khora_core::ui::Margin>,
        PhantomData::<khora_core::ui::geometry::Margin>,
    );
    let _ = margin_fields as fn(&khora_core::ui::geometry::Margin);
    let _ = khora_core::ui::geometry::Margin::ZERO;
    let _ = khora_core::ui::geometry::Margin::same;
    let _ = khora_core::ui::geometry::Margin::symmetric;
    is_debug::<khora_core::ui::geometry::Margin>();
    is_clone::<khora_core::ui::geometry::Margin>();
    is_copy::<khora_core::ui::geometry::Margin>();
    is_partial_eq::<khora_core::ui::geometry::Margin>();
    is_default::<khora_core::ui::geometry::Margin>();
    let _ = type_name::<khora_core::ui::geometry::Stroke>();
    let _ = type_name::<khora_core::ui::Stroke>();
    same_type(
        PhantomData::<khora_core::ui::Stroke>,
        PhantomData::<khora_core::ui::geometry::Stroke>,
    );
    let _ = stroke_fields as fn(&khora_core::ui::geometry::Stroke);
    let _ = khora_core::ui::geometry::Stroke::NONE;
    let _ = khora_core::ui::geometry::Stroke::new;
    is_debug::<khora_core::ui::geometry::Stroke>();
    is_clone::<khora_core::ui::geometry::Stroke>();
    is_copy::<khora_core::ui::geometry::Stroke>();
    is_partial_eq::<khora_core::ui::geometry::Stroke>();
    is_default::<khora_core::ui::geometry::Stroke>();
    // trait `khora_core::ui::layout::LayoutSystem`: see `layout_system_trait_items`
    // trait `khora_core::ui::LayoutSystem`: see `layout_system_trait_items`
    // trait `khora_core::ui::layout::UiLayoutView`: see `ui_layout_view_trait_items`
    // trait `khora_core::ui::UiLayoutView`: see `ui_layout_view_trait_items`
    let _ = type_name::<khora_core::ui::theme::UiTheme>();
    let _ = type_name::<khora_core::ui::UiTheme>();
    same_type(
        PhantomData::<khora_core::ui::UiTheme>,
        PhantomData::<khora_core::ui::theme::UiTheme>,
    );
    let _ = ui_theme_fields as fn(&khora_core::ui::theme::UiTheme);
    is_debug::<khora_core::ui::theme::UiTheme>();
    is_clone::<khora_core::ui::theme::UiTheme>();
    is_default::<khora_core::ui::theme::UiTheme>();
    let _ = type_name::<khora_core::ui::types::UiBorder>();
    let _ = ui_border_fields as fn(&khora_core::ui::types::UiBorder);
    is_debug::<khora_core::ui::types::UiBorder>();
    is_clone::<khora_core::ui::types::UiBorder>();
    is_copy::<khora_core::ui::types::UiBorder>();
    is_partial_eq::<khora_core::ui::types::UiBorder>();
    is_default::<khora_core::ui::types::UiBorder>();
    is_serialize::<khora_core::ui::types::UiBorder>();
    is_deserialize_owned::<khora_core::ui::types::UiBorder>();
    is_encode::<khora_core::ui::types::UiBorder>();
    is_decode::<khora_core::ui::types::UiBorder>();
    is_borrow_decode::<khora_core::ui::types::UiBorder>();
    let _ = type_name::<khora_core::ui::types::UiColor>();
    let _ = ui_color_fields as fn(&khora_core::ui::types::UiColor);
    is_debug::<khora_core::ui::types::UiColor>();
    is_clone::<khora_core::ui::types::UiColor>();
    is_copy::<khora_core::ui::types::UiColor>();
    is_partial_eq::<khora_core::ui::types::UiColor>();
    is_serialize::<khora_core::ui::types::UiColor>();
    is_deserialize_owned::<khora_core::ui::types::UiColor>();
    is_encode::<khora_core::ui::types::UiColor>();
    is_decode::<khora_core::ui::types::UiColor>();
    is_borrow_decode::<khora_core::ui::types::UiColor>();
    is_default::<khora_core::ui::types::UiColor>();
    let _ = type_name::<khora_core::ui::types::UiFlexDirection>();
    let _ = ui_flex_direction_variants as fn(&khora_core::ui::types::UiFlexDirection);
    is_debug::<khora_core::ui::types::UiFlexDirection>();
    is_clone::<khora_core::ui::types::UiFlexDirection>();
    is_copy::<khora_core::ui::types::UiFlexDirection>();
    is_partial_eq::<khora_core::ui::types::UiFlexDirection>();
    is_eq::<khora_core::ui::types::UiFlexDirection>();
    is_default::<khora_core::ui::types::UiFlexDirection>();
    is_serialize::<khora_core::ui::types::UiFlexDirection>();
    is_deserialize_owned::<khora_core::ui::types::UiFlexDirection>();
    is_encode::<khora_core::ui::types::UiFlexDirection>();
    is_decode::<khora_core::ui::types::UiFlexDirection>();
    is_borrow_decode::<khora_core::ui::types::UiFlexDirection>();
    let _ = type_name::<khora_core::ui::types::UiImage>();
    let _ = ui_image_fields as fn(&khora_core::ui::types::UiImage);
    is_debug::<khora_core::ui::types::UiImage>();
    is_clone::<khora_core::ui::types::UiImage>();
    is_copy::<khora_core::ui::types::UiImage>();
    is_partial_eq::<khora_core::ui::types::UiImage>();
    is_serialize::<khora_core::ui::types::UiImage>();
    is_deserialize_owned::<khora_core::ui::types::UiImage>();
    is_encode::<khora_core::ui::types::UiImage>();
    is_decode::<khora_core::ui::types::UiImage>();
    is_borrow_decode::<khora_core::ui::types::UiImage>();
    let _ = type_name::<khora_core::ui::types::UiNode>();
    let _ = ui_node_fields as fn(&khora_core::ui::types::UiNode);
    is_debug::<khora_core::ui::types::UiNode>();
    is_clone::<khora_core::ui::types::UiNode>();
    is_partial_eq::<khora_core::ui::types::UiNode>();
    is_default::<khora_core::ui::types::UiNode>();
    is_serialize::<khora_core::ui::types::UiNode>();
    is_deserialize_owned::<khora_core::ui::types::UiNode>();
    is_encode::<khora_core::ui::types::UiNode>();
    is_decode::<khora_core::ui::types::UiNode>();
    is_borrow_decode::<khora_core::ui::types::UiNode>();
    let _ = type_name::<khora_core::ui::types::UiRect<f32>>();
    let _ = ui_rect_fields as fn(&khora_core::ui::types::UiRect<f32>);
    let _ = <khora_core::ui::types::UiRect<f32>>::all;
    is_debug::<khora_core::ui::types::UiRect<f32>>();
    is_clone::<khora_core::ui::types::UiRect<f32>>();
    is_copy::<khora_core::ui::types::UiRect<f32>>();
    is_partial_eq::<khora_core::ui::types::UiRect<f32>>();
    is_default::<khora_core::ui::types::UiRect<f32>>();
    is_serialize::<khora_core::ui::types::UiRect<f32>>();
    is_deserialize_owned::<khora_core::ui::types::UiRect<f32>>();
    is_encode::<khora_core::ui::types::UiRect<f32>>();
    is_decode::<khora_core::ui::types::UiRect<f32>>();
    is_borrow_decode::<khora_core::ui::types::UiRect<f32>>();
    let _ = type_name::<khora_core::ui::types::UiText>();
    let _ = ui_text_fields as fn(&khora_core::ui::types::UiText);
    is_debug::<khora_core::ui::types::UiText>();
    is_clone::<khora_core::ui::types::UiText>();
    is_partial_eq::<khora_core::ui::types::UiText>();
    is_serialize::<khora_core::ui::types::UiText>();
    is_deserialize_owned::<khora_core::ui::types::UiText>();
    is_encode::<khora_core::ui::types::UiText>();
    is_decode::<khora_core::ui::types::UiText>();
    is_borrow_decode::<khora_core::ui::types::UiText>();
    is_default::<khora_core::ui::types::UiText>();
    let _ = type_name::<khora_core::ui::types::UiTransform>();
    let _ = ui_transform_fields as fn(&khora_core::ui::types::UiTransform);
    is_debug::<khora_core::ui::types::UiTransform>();
    is_clone::<khora_core::ui::types::UiTransform>();
    is_copy::<khora_core::ui::types::UiTransform>();
    is_partial_eq::<khora_core::ui::types::UiTransform>();
    is_default::<khora_core::ui::types::UiTransform>();
    is_serialize::<khora_core::ui::types::UiTransform>();
    is_deserialize_owned::<khora_core::ui::types::UiTransform>();
    is_encode::<khora_core::ui::types::UiTransform>();
    is_decode::<khora_core::ui::types::UiTransform>();
    is_borrow_decode::<khora_core::ui::types::UiTransform>();
    let _ = type_name::<khora_core::ui::types::UiVal>();
    let _ = ui_val_variants as fn(&khora_core::ui::types::UiVal);
    is_debug::<khora_core::ui::types::UiVal>();
    is_clone::<khora_core::ui::types::UiVal>();
    is_copy::<khora_core::ui::types::UiVal>();
    is_partial_eq::<khora_core::ui::types::UiVal>();
    is_serialize::<khora_core::ui::types::UiVal>();
    is_deserialize_owned::<khora_core::ui::types::UiVal>();
    is_encode::<khora_core::ui::types::UiVal>();
    is_decode::<khora_core::ui::types::UiVal>();
    is_borrow_decode::<khora_core::ui::types::UiVal>();
    is_default::<khora_core::ui::types::UiVal>();
}
