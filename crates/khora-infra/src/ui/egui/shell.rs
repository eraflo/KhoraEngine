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

//! Concrete [`EditorShell`] backed by egui native panels.
//!
//! Generic host: the shell knows nothing about Khora branding, menus, toolbars
//! or status-bar contents. It just applies a theme, lays out the slots
//! defined by [`PanelLocation`], and forwards each slot's `egui::Ui` to the
//! application-supplied [`EditorPanel`]s.
//!
//! Slot layout (all registered panels are routed here):
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
use khora_core::ui::editor::fonts::{FontHandle, FontPack, NamedFont};
use khora_core::ui::editor::panel::{EditorPanel, PanelLocation};
use khora_core::ui::editor::shell::EditorShell;
use khora_core::ui::editor::state::{EditorMode, EditorState, StatusBarData};
use khora_core::ui::editor::theme::EditorTheme;
use khora_core::ui::editor::viewport_texture::ViewportTextureHandle;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Installs each [`NamedFont`] into `definitions`, registering it as the
/// primary face for `family` (or as a fallback at the front of the list).
fn install_named(defs: &mut egui::FontDefinitions, family: egui::FontFamily, list: Vec<NamedFont>) {
    for named in list.into_iter() {
        let key = named.name.clone();
        let bytes = match named.data {
            FontHandle::Static(s) => egui::FontData::from_static(s),
            FontHandle::Owned(v) => egui::FontData::from_owned(v),
        };
        defs.font_data
            .insert(key.clone(), std::sync::Arc::new(bytes));
        defs.families
            .entry(family.clone())
            .or_default()
            .insert(0, key);
    }
}

/// Fixed-height/width slots — same on every screen.
const DEFAULT_TOPBAR_HEIGHT: f32 = 32.0;
const DEFAULT_SPINE_WIDTH: f32 = 56.0;
const DEFAULT_STATUSBAR_HEIGHT: f32 = 24.0;

/// Proportional defaults relative to screen size. The [`EditorPanel`]'s own
/// `preferred_size` is treated as a fallback for very small screens — the
/// proportional rule wins above the per-panel minimum so the dock looks the
/// same on a 1080p laptop and a 4K display.
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

/// Cached, one-shot proportional defaults — computed the first frame the
/// shell sees a non-zero screen rect and never recomputed. egui persists
/// the user's resize between frames; recomputing the default each frame
/// would risk overriding it (and was the source of the "panels snap back"
/// bug users reported earlier).
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
    theme: EditorTheme,
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
    pub fn new(ctx: egui::Context, theme: EditorTheme) -> Self {
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

    /// Returns the most recent [`StatusBarData`] passed via [`set_status`].
    /// Editor-side status-bar panels read this through their own state, but
    /// debug shells / tests can introspect it here.
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
        {
            return true;
        }
        if let Some(pos) = self.floating_panels.iter().position(|e| e.panel.id() == id) {
            self.floating_panels.remove(pos);
            return true;
        }
        false
    }

    fn set_theme(&mut self, theme: EditorTheme) {
        self.theme = theme;
        self.theme_applied = false;
    }

    fn set_fonts(&mut self, fonts: FontPack) {
        if fonts.is_empty() {
            return;
        }

        let mut definitions = egui::FontDefinitions::default();
        install_named(
            &mut definitions,
            egui::FontFamily::Proportional,
            fonts.proportional,
        );
        install_named(
            &mut definitions,
            egui::FontFamily::Monospace,
            fonts.monospace,
        );
        install_named(
            &mut definitions,
            egui::FontFamily::Name("icons".into()),
            fonts.icons,
        );
        self.ctx.set_fonts(definitions);
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
            // DPI scales (e.g. 1.25) cause Geist glyphs to render at sub-pixel
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

        // Cheap Arc clone — avoids borrow conflicts between `ctx.show()` and
        // `&mut self` field accesses.
        let ctx = self.ctx.clone();
        let vt = &self.viewport_textures;

        // The Control Plane workspace ships its own dedicated inspector,
        // so the Scene-mode right SidePanel would create a visible doublon
        // when the user switches modes. Read active_mode from editor state
        // and skip the right side accordingly. (Shell already depends on
        // EditorState, so this doesn't add a new layer dependency.)
        let hide_right_panel = self
            .editor_state
            .as_ref()
            .and_then(|s| {
                s.lock()
                    .ok()
                    .map(|g| g.active_mode == EditorMode::ControlPlane)
            })
            .unwrap_or(false);

        // Compute proportional defaults the FIRST frame we see a real
        // screen rect, then cache them. Recomputing each frame would risk
        // overwriting the user's resize — egui persists `PanelState` and
        // checks our `default_width` only when no state exists, but feeding
        // it shifting values frame-to-frame is brittle and was reported as
        // panels "snapping back" after a resize.
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

        // ── Top bar(s) ─────────────────────────────────
        // Each TopBar panel becomes its own fixed-height TopBottomPanel,
        // stacked in registration order from the top down.
        for (idx, panel) in self.top_panels.iter_mut().enumerate() {
            let height = panel.preferred_size().unwrap_or(DEFAULT_TOPBAR_HEIGHT);
            let panel_id = format!("editor_topbar_{}_{}", idx, panel.id());
            egui::TopBottomPanel::top(panel_id)
                .exact_height(height)
                .resizable(false)
                .show(&ctx, |ui| {
                    let mut builder = EguiUiBuilder::new(ui, vt);
                    panel.ui(&mut builder);
                });
        }

        // ── Status bar(s) ──────────────────────────────
        // Stacked from the bottom up. Declared BEFORE the resizable Bottom so
        // egui reserves vertical space for it correctly.
        for (idx, panel) in self.status_panels.iter_mut().enumerate() {
            let height = panel.preferred_size().unwrap_or(DEFAULT_STATUSBAR_HEIGHT);
            let panel_id = format!("editor_statusbar_{}_{}", idx, panel.id());
            egui::TopBottomPanel::bottom(panel_id)
                .exact_height(height)
                .resizable(false)
                .show(&ctx, |ui| {
                    let mut builder = EguiUiBuilder::new(ui, vt);
                    panel.ui(&mut builder);
                });
        }

        // ── Spine (fixed left strip) ──────────────────
        // First spine panel wins (the layout assumes one spine). Additional
        // spine panels are rendered as a vertical stack inside the same panel
        // for now — easy to revisit if real apps need more.
        if !self.spine_panels.is_empty() {
            let width = self.spine_panels[0]
                .preferred_size()
                .unwrap_or(DEFAULT_SPINE_WIDTH);
            egui::SidePanel::left("editor_spine")
                .exact_width(width)
                .resizable(false)
                .show(&ctx, |ui| {
                    for panel in &mut self.spine_panels {
                        let mut builder = EguiUiBuilder::new(ui, vt);
                        panel.ui(&mut builder);
                    }
                });
        }

        // ── Bottom (resizable, tabbed) ─────────────────
        if !self.bottom_panels.is_empty() {
            let active_tab = &mut self.active_bottom_tab;
            let panels = &mut self.bottom_panels;
            // Panel-supplied preferred size is treated as an *additional*
            // floor — useful for panels that need more than the global
            // minimum. Proportional default still wins on big screens.
            let panel_min = panels[0].preferred_size().unwrap_or(0.0);
            let default_h = bottom_default.max(panel_min);

            egui::TopBottomPanel::bottom("editor_bottom")
                .default_height(default_h)
                .min_height(BOTTOM_MIN)
                .max_height(bottom_max.max(default_h + 1.0))
                .resizable(true)
                .show(&ctx, |ui| {
                    // Force the inner UI to span the full panel rect —
                    // otherwise our paint-only panels (no cursor allocation)
                    // make `inner_response.response.rect` shrink to
                    // `width_range.min`, and egui stores THAT in PanelState,
                    // so the panel snaps back to the minimum on every drag
                    // release.
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
                        // Hand the panel ONLY the area below the tab bar
                        // — without this, the panel's `panel_rect()` is
                        // still the full bottom-panel rect, so its header
                        // strip paints over the tab bar and the content
                        // (logs / asset grid) ends up clipped or hidden.
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

        // ── Left sidebar (resizable) ──────────────────
        if !self.left_panels.is_empty() {
            let panel_min = self.left_panels[0].preferred_size().unwrap_or(0.0);
            let default_w = left_default.max(panel_min);
            let panels = &mut self.left_panels;
            egui::SidePanel::left("editor_left")
                .default_width(default_w)
                .min_width(LEFT_MIN)
                .max_width(left_max.max(default_w + 1.0))
                .resizable(true)
                .show(&ctx, |ui| {
                    // See the bottom panel above for the rationale —
                    // without `set_min_size` the resize drag is reverted on
                    // mouse release because PanelState stores the painted
                    // content rect (small) instead of the panel rect.
                    ui.set_min_size(ui.max_rect().size());
                    for panel in panels.iter_mut() {
                        let mut builder = EguiUiBuilder::new(ui, vt);
                        panel.ui(&mut builder);
                    }
                });
        }

        // ── Right sidebar (resizable) ─────────────────
        if !self.right_panels.is_empty() && !hide_right_panel {
            let panel_min = self.right_panels[0].preferred_size().unwrap_or(0.0);
            let default_w = right_default.max(panel_min);
            let panels = &mut self.right_panels;
            egui::SidePanel::right("editor_right")
                .default_width(default_w)
                .min_width(RIGHT_MIN)
                .max_width(right_max.max(default_w + 1.0))
                .resizable(true)
                .show(&ctx, |ui| {
                    // Same rationale as the left panel above.
                    ui.set_min_size(ui.max_rect().size());
                    for panel in panels.iter_mut() {
                        let mut builder = EguiUiBuilder::new(ui, vt);
                        panel.ui(&mut builder);
                    }
                });
        }

        // ── Central area ──────────────────────────────
        egui::CentralPanel::default().show(&ctx, |ui| {
            if self.center_panels.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("");
                });
            } else {
                for panel in &mut self.center_panels {
                    let mut builder = EguiUiBuilder::new(ui, vt);
                    panel.ui(&mut builder);
                }
            }
        });

        // ── Floating overlays (z-ordered) ─────────────
        // egui::Area lets us render free-floating UI on top of the rest. The
        // panel is responsible for all its own positioning / sizing.
        for entry in &mut self.floating_panels {
            let area_id = egui::Id::new(("editor_floating", entry.panel.id()));
            egui::Area::new(area_id)
                .order(egui::Order::Foreground)
                .interactable(true)
                .show(&ctx, |ui| {
                    let mut builder = EguiUiBuilder::new(ui, vt);
                    entry.panel.ui(&mut builder);
                });
        }
    }
}
