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

//! The egui [`EditorShell`]: a generic host that applies a theme, lays out the
//! [`PanelLocation`] slots, and hands each slot to the application's [`EditorPanel`]s.
//!
//! ```text
//! ┌──────────────────────────────────────────────────────┐
//! │  TopBar (stack, fixed height)                        │
//! ├────────┬───────────────────────────┬─────────────────┤
//! │ Spine  │                           │                 │
//! │ (fixed │     Center                │   Right         │
//! │ width) │                           │   (resizable)   │
//! │        ├───────────────────────────┴─────────────────┤
//! │   +    │     Bottom (resizable, tabbed)              │
//! │  Left  ├──────────────────────────────────────────────┤
//! │        │     StatusBar (stack, fixed height)         │
//! └────────┴──────────────────────────────────────────────┘
//! ```

use super::theme::apply_theme;
use super::ui_builder::EguiUiBuilder;
use khora_core::ui::editor::panel::{EditorPanel, PanelLocation};
use khora_core::ui::editor::shell::EditorShell;
use khora_core::ui::editor::state::{EditorState, StatusBarData};
use khora_core::ui::editor::viewport_texture::ViewportTextureHandle;
use khora_core::ui::fonts::FontPack;
use khora_core::ui::UiTheme;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Fixed-height/width slots — same on every screen.
const DEFAULT_TOPBAR_HEIGHT: f32 = 32.0;
const DEFAULT_SPINE_WIDTH: f32 = 56.0;
const DEFAULT_STATUSBAR_HEIGHT: f32 = 24.0;

/// Default slot sizes, as fractions of the screen.
///
/// They win over a panel's `preferred_size` above the per-panel minimum, so the
/// dock looks the same on a 1080p laptop and a 4K display.
const LEFT_FRAC: f32 = 0.16;
const RIGHT_FRAC: f32 = 0.20;
const BOTTOM_FRAC: f32 = 0.26;

/// Hard minimums — below these widths panel content begins to collide. The
/// resize handles still let the user shrink to these limits.
const LEFT_MIN: f32 = 240.0;
const LEFT_MAX_FRAC: f32 = 0.35;
const RIGHT_MIN: f32 = 260.0;
const RIGHT_MAX_FRAC: f32 = 0.40;
const BOTTOM_MIN: f32 = 140.0;
const BOTTOM_MAX_FRAC: f32 = 0.55;

/// A floating panel keeps its z-order so the shell can paint them top-down.
struct FloatingEntry {
    z: i32,
    panel: Box<dyn EditorPanel>,
}

/// The proportional defaults, computed once, the first frame with a non-zero screen.
///
/// egui persists the user's resizes; recomputing the defaults each frame could
/// override them.
#[derive(Default, Clone, Copy)]
struct CachedDefaults {
    left_w: Option<f32>,
    right_w: Option<f32>,
    bottom_h: Option<f32>,
}

/// Egui-backed editor shell using native `SidePanel`, `TopBottomPanel`, and
/// `CentralPanel` for the dock layout.
pub struct EguiEditorShell {
    ctx: egui::Context,
    top_panels: Vec<Box<dyn EditorPanel>>,
    spine_panels: Vec<Box<dyn EditorPanel>>,
    left_panels: Vec<Box<dyn EditorPanel>>,
    right_panels: Vec<Box<dyn EditorPanel>>,
    bottom_panels: Vec<Box<dyn EditorPanel>>,
    status_panels: Vec<Box<dyn EditorPanel>>,
    center_panels: Vec<Box<dyn EditorPanel>>,
    floating_panels: Vec<FloatingEntry>,
    /// Full-screen panels painted above everything else.
    cover_panels: Vec<Box<dyn EditorPanel>>,
    theme: UiTheme,
    theme_applied: bool,
    active_bottom_tab: usize,
    /// Maps abstract viewport handles to egui texture IDs.
    viewport_textures: HashMap<ViewportTextureHandle, egui::TextureId>,
    /// Status-bar snapshot. Kept here so shells that want to surface it to a
    /// debug overlay can; the actual status-bar UI lives in editor-side panels.
    status: StatusBarData,
    editor_state: Option<Arc<Mutex<EditorState>>>,
    defaults: CachedDefaults,
}

impl EguiEditorShell {
    /// Creates a new shell using the given egui context (shared with `EguiOverlay`).
    pub fn new(ctx: egui::Context, theme: UiTheme) -> Self {
        Self {
            ctx,
            top_panels: Vec::new(),
            spine_panels: Vec::new(),
            left_panels: Vec::new(),
            right_panels: Vec::new(),
            bottom_panels: Vec::new(),
            status_panels: Vec::new(),
            center_panels: Vec::new(),
            floating_panels: Vec::new(),
            cover_panels: Vec::new(),
            theme,
            theme_applied: false,
            active_bottom_tab: 0,
            viewport_textures: HashMap::new(),
            status: StatusBarData::default(),
            editor_state: None,
            defaults: CachedDefaults::default(),
        }
    }

    /// Registers an abstract viewport handle → egui texture ID mapping.
    ///
    /// Called by the render system after `overlay.register_viewport_texture()`.
    pub fn register_viewport_texture(
        &mut self,
        handle: ViewportTextureHandle,
        egui_id: egui::TextureId,
    ) {
        self.viewport_textures.insert(handle, egui_id);
    }

    /// Returns the egui texture ID for a given viewport handle, if registered.
    pub fn resolve_viewport_texture(
        &self,
        handle: ViewportTextureHandle,
    ) -> Option<egui::TextureId> {
        self.viewport_textures.get(&handle).copied()
    }

    /// Returns the most recent [`StatusBarData`] passed via [`EditorShell::set_status`].
    ///
    /// The editor's status-bar panels read it from their own state; debug
    /// shells and tests read it here.
    pub fn last_status(&self) -> &StatusBarData {
        &self.status
    }
}

impl EditorShell for EguiEditorShell {
    fn register_panel(&mut self, location: PanelLocation, panel: Box<dyn EditorPanel>) {
        log::info!(
            "EditorShell: registered panel '{}' at {:?}",
            panel.id(),
            location
        );
        match location {
            PanelLocation::TopBar => self.top_panels.push(panel),
            PanelLocation::Spine => self.spine_panels.push(panel),
            PanelLocation::Left => self.left_panels.push(panel),
            PanelLocation::Right => self.right_panels.push(panel),
            PanelLocation::Bottom => self.bottom_panels.push(panel),
            PanelLocation::StatusBar => self.status_panels.push(panel),
            PanelLocation::Center => self.center_panels.push(panel),
            PanelLocation::Floating(z) => self.floating_panels.push(FloatingEntry { z, panel }),
            PanelLocation::Cover => self.cover_panels.push(panel),
        }
        // Keep floating panels sorted bottom-to-top so painting respects z-order.
        if !self.floating_panels.is_empty() {
            self.floating_panels.sort_by_key(|e| e.z);
        }
    }

    fn remove_panel(&mut self, id: &str) -> bool {
        let remove_from = |v: &mut Vec<Box<dyn EditorPanel>>| -> bool {
            if let Some(pos) = v.iter().position(|p| p.id() == id) {
                v.remove(pos);
                true
            } else {
                false
            }
        };
        if remove_from(&mut self.top_panels)
            || remove_from(&mut self.spine_panels)
            || remove_from(&mut self.left_panels)
            || remove_from(&mut self.right_panels)
            || remove_from(&mut self.bottom_panels)
            || remove_from(&mut self.status_panels)
            || remove_from(&mut self.center_panels)
            || remove_from(&mut self.cover_panels)
        {
            return true;
        }
        if let Some(pos) = self.floating_panels.iter().position(|e| e.panel.id() == id) {
            self.floating_panels.remove(pos);
            return true;
        }
        false
    }

    fn set_theme(&mut self, theme: UiTheme) {
        self.theme = theme;
        self.theme_applied = false;
    }

    fn set_fonts(&mut self, fonts: FontPack) {
        if fonts.is_empty() {
            return;
        }
        self.ctx.set_fonts(super::fonts::font_definitions(&fonts));
    }

    fn set_status(&mut self, data: StatusBarData) {
        self.status = data;
    }

    fn set_editor_state(&mut self, state: Arc<Mutex<EditorState>>) {
        self.editor_state = Some(state);
    }

    fn show_frame(&mut self) {
        // Apply theme once (or when changed).
        if !self.theme_applied {
            apply_theme(&self.ctx, &self.theme);
            // Snap pixels_per_point to the nearest 0.5 multiple. Fractional
            // DPI scales (e.g. 1.25) cause glyphs to render at sub-pixel
            // positions and look soft. Snapping keeps integer scale on most
            // monitors and a clean half-pixel offset on the rest.
            let raw = self
                .ctx
                .input(|i| i.viewport().native_pixels_per_point)
                .unwrap_or(1.0);
            let snapped = ((raw * 2.0).round() / 2.0).max(1.0);
            self.ctx.set_pixels_per_point(snapped);
            self.theme_applied = true;
        }

        // Cheap Arc clone — avoids borrow conflicts between the panel calls and
        // `&mut self` field accesses.
        let ctx = self.ctx.clone();
        let vt = &self.viewport_textures;

        // Panels nest inside a root `Ui`. This overlay drives `begin_pass` and
        // `end_pass` itself, so the wgpu render sits between them, and builds
        // that root as `Context::run_ui` does.
        let mut root_ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new((ctx.viewport_id(), "__top_ui")),
            egui::UiBuilder::new()
                .layer_id(egui::LayerId::background())
                // `content_rect`, not the deprecated `available_rect`: the
                // former is the area safe to draw content in (it excludes OS
                // notches and status bars), and it is what the proportional
                // panel defaults below already measure against.
                .max_rect(ctx.content_rect()),
        );
        let root_ui = &mut root_ui;

        // Computed once, then cached: egui reads `default_width` only while no
        // `PanelState` exists, and values shifting frame to frame would make
        // panels snap back after a resize.
        let screen = ctx.content_rect();
        let screen_w = screen.width();
        let screen_h = screen.height();
        if self.defaults.left_w.is_none() && screen_w > 200.0 && screen_h > 200.0 {
            self.defaults.left_w = Some((screen_w * LEFT_FRAC).max(LEFT_MIN));
            self.defaults.right_w = Some((screen_w * RIGHT_FRAC).max(RIGHT_MIN));
            self.defaults.bottom_h = Some((screen_h * BOTTOM_FRAC).max(BOTTOM_MIN));
        }
        let left_default = self.defaults.left_w.unwrap_or(LEFT_MIN);
        let right_default = self.defaults.right_w.unwrap_or(RIGHT_MIN);
        let bottom_default = self.defaults.bottom_h.unwrap_or(BOTTOM_MIN);
        // Max widths stay proportional to the current screen so giant
        // monitors aren't artificially capped.
        let left_max = (screen_w * LEFT_MAX_FRAC).max(LEFT_MIN + 60.0);
        let right_max = (screen_w * RIGHT_MAX_FRAC).max(RIGHT_MIN + 60.0);
        let bottom_max = (screen_h * BOTTOM_MAX_FRAC).max(BOTTOM_MIN + 80.0);

        show_top_bars(root_ui, &mut self.top_panels, vt);
        // Declared before the resizable Bottom so egui reserves vertical
        // space for them correctly.
        show_status_bars(root_ui, &mut self.status_panels, vt);
        show_spine(root_ui, &mut self.spine_panels, vt);
        show_bottom(
            root_ui,
            &mut self.bottom_panels,
            &mut self.active_bottom_tab,
            vt,
            bottom_default,
            bottom_max,
        );
        show_sidebar(
            root_ui,
            egui::Panel::left("editor_left"),
            &mut self.left_panels,
            vt,
            [left_default, LEFT_MIN, left_max],
        );
        show_sidebar(
            root_ui,
            egui::Panel::right("editor_right"),
            &mut self.right_panels,
            vt,
            [right_default, RIGHT_MIN, right_max],
        );
        show_center(root_ui, &mut self.center_panels, vt);
        show_floating(&ctx, &mut self.floating_panels, vt);
        show_covers(&ctx, &mut self.cover_panels, vt);
    }
}

/// Shows each `TopBar` panel as its own fixed-height panel, stacked in
/// registration order from the top down.
fn show_top_bars(
    root_ui: &mut egui::Ui,
    panels: &mut [Box<dyn EditorPanel>],
    vt: &HashMap<ViewportTextureHandle, egui::TextureId>,
) {
    for (idx, panel) in panels.iter_mut().enumerate() {
        let height = panel.preferred_size().unwrap_or(DEFAULT_TOPBAR_HEIGHT);
        let panel_id = format!("editor_topbar_{}_{}", idx, panel.id());
        egui::Panel::top(panel_id)
            .exact_size(height)
            .resizable(false)
            .show_inside(root_ui, |ui| {
                let mut builder = EguiUiBuilder::new(ui, vt);
                panel.ui(&mut builder);
            });
    }
}

/// Shows each `StatusBar` panel as its own fixed-height panel, stacked from the
/// bottom up.
fn show_status_bars(
    root_ui: &mut egui::Ui,
    panels: &mut [Box<dyn EditorPanel>],
    vt: &HashMap<ViewportTextureHandle, egui::TextureId>,
) {
    for (idx, panel) in panels.iter_mut().enumerate() {
        let height = panel.preferred_size().unwrap_or(DEFAULT_STATUSBAR_HEIGHT);
        let panel_id = format!("editor_statusbar_{}_{}", idx, panel.id());
        egui::Panel::bottom(panel_id)
            .exact_size(height)
            .resizable(false)
            .show_inside(root_ui, |ui| {
                let mut builder = EguiUiBuilder::new(ui, vt);
                panel.ui(&mut builder);
            });
    }
}

/// Shows the fixed left strip, sized by the first spine panel.
///
/// The layout assumes one spine: further spine panels stack inside the first
/// one's panel.
fn show_spine(
    root_ui: &mut egui::Ui,
    panels: &mut [Box<dyn EditorPanel>],
    vt: &HashMap<ViewportTextureHandle, egui::TextureId>,
) {
    if panels.is_empty() {
        return;
    }
    let width = panels[0].preferred_size().unwrap_or(DEFAULT_SPINE_WIDTH);
    egui::Panel::left("editor_spine")
        .exact_size(width)
        .resizable(false)
        .show_inside(root_ui, |ui| {
            for panel in panels.iter_mut() {
                let mut builder = EguiUiBuilder::new(ui, vt);
                panel.ui(&mut builder);
            }
        });
}

/// Shows the resizable bottom slot, with a tab bar when it holds more than
/// one panel.
fn show_bottom(
    root_ui: &mut egui::Ui,
    panels: &mut [Box<dyn EditorPanel>],
    active_tab: &mut usize,
    vt: &HashMap<ViewportTextureHandle, egui::TextureId>,
    bottom_default: f32,
    bottom_max: f32,
) {
    if panels.is_empty() {
        return;
    }
    // Panel-supplied preferred size is treated as an *additional*
    // floor — useful for panels that need more than the global
    // minimum. Proportional default still wins on big screens.
    let panel_min = panels[0].preferred_size().unwrap_or(0.0);
    let default_h = bottom_default.max(panel_min);

    egui::Panel::bottom("editor_bottom")
        .default_size(default_h)
        .min_size(BOTTOM_MIN)
        .max_size(bottom_max.max(default_h + 1.0))
        .resizable(true)
        .show_inside(root_ui, |ui| {
            // The inner UI spans the whole panel rect: a paint-only
            // panel allocates nothing, so egui would store the minimum
            // in `PanelState` and snap the panel back on every release.
            ui.set_min_size(ui.max_rect().size());
            if panels.len() > 1 {
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    for (i, panel) in panels.iter().enumerate() {
                        let active = *active_tab == i;
                        if ui.selectable_label(active, panel.title()).clicked() {
                            *active_tab = i;
                        }
                        ui.add_space(2.0);
                    }
                });
                ui.add(egui::Separator::default().spacing(2.0));
            }

            if let Some(panel) = panels.get_mut(*active_tab) {
                // Only the area below the tab bar, or the panel's
                // header would paint over the tabs and its content be
                // clipped.
                let remaining = ui.available_rect_before_wrap();
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(remaining)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                child.set_min_size(remaining.size());
                let mut builder = EguiUiBuilder::new(&mut child, vt);
                panel.ui(&mut builder);
            }
        });
}

/// Shows a resizable sidebar on `side`, sized by `[default, min, max]`.
fn show_sidebar(
    root_ui: &mut egui::Ui,
    side: egui::Panel,
    panels: &mut [Box<dyn EditorPanel>],
    vt: &HashMap<ViewportTextureHandle, egui::TextureId>,
    [default, min, max]: [f32; 3],
) {
    if panels.is_empty() {
        return;
    }
    let panel_min = panels[0].preferred_size().unwrap_or(0.0);
    let default_w = default.max(panel_min);
    side.default_size(default_w)
        .min_size(min)
        .max_size(max.max(default_w + 1.0))
        .resizable(true)
        .show_inside(root_ui, |ui| {
            // As for the bottom panel: without `set_min_size`, a
            // resize is reverted on release.
            ui.set_min_size(ui.max_rect().size());
            for panel in panels.iter_mut() {
                let mut builder = EguiUiBuilder::new(ui, vt);
                panel.ui(&mut builder);
            }
        });
}

fn show_center(
    root_ui: &mut egui::Ui,
    panels: &mut [Box<dyn EditorPanel>],
    vt: &HashMap<ViewportTextureHandle, egui::TextureId>,
) {
    egui::CentralPanel::default().show_inside(root_ui, |ui| {
        if panels.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("");
            });
        } else {
            for panel in panels.iter_mut() {
                let mut builder = EguiUiBuilder::new(ui, vt);
                panel.ui(&mut builder);
            }
        }
    });
}

/// Shows each floating panel in its own area, in z-order; the panel places
/// and sizes itself.
fn show_floating(
    ctx: &egui::Context,
    entries: &mut [FloatingEntry],
    vt: &HashMap<ViewportTextureHandle, egui::TextureId>,
) {
    for entry in entries.iter_mut() {
        let area_id = egui::Id::new(("editor_floating", entry.panel.id()));
        // `Area` is not a panel: it floats in its own layer above the
        // layout rather than carving space out of a parent `Ui`, so it
        // still takes the context directly.
        egui::Area::new(area_id)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                let mut builder = EguiUiBuilder::new(ui, vt);
                entry.panel.ui(&mut builder);
            });
    }
}

/// Shows each cover panel full screen, above everything, taking every click.
///
/// Its panel paints in a `Ui` of its own on the area's layer: a new area's
/// content is hidden on its first frame, while egui measures it, and a cover
/// shows at once.
fn show_covers(
    ctx: &egui::Context,
    panels: &mut [Box<dyn EditorPanel>],
    vt: &HashMap<ViewportTextureHandle, egui::TextureId>,
) {
    let screen = ctx.content_rect();
    for panel in panels.iter_mut() {
        let area_id = egui::Id::new(("editor_cover", panel.id()));
        egui::Area::new(area_id)
            .order(egui::Order::Tooltip)
            .fixed_pos(screen.min)
            .interactable(true)
            .fade_in(false)
            .show(ctx, |ui| {
                ui.allocate_rect(screen, egui::Sense::click());
            });
        let layer = egui::LayerId::new(egui::Order::Tooltip, area_id);
        let mut ui = egui::Ui::new(
            ctx.clone(),
            area_id.with("paint"),
            egui::UiBuilder::new().layer_id(layer).max_rect(screen),
        );
        let mut builder = EguiUiBuilder::new(&mut ui, vt);
        panel.ui(&mut builder);
    }
}
