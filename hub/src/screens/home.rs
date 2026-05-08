// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Home screen — project list + sidebar nav.
//!
//! All painting goes through `UiBuilder` primitives. Includes a modal
//! confirmation when removing a project from disk, vertical-gradient
//! sidebar backdrop, and accent-stripe project cards.

use crate::HubApp;
use crate::Screen;
use crate::config::RecentProject;
use crate::theme::{pal, tint};
use crate::widgets::{
    badge, format_ts, ghost_button, paint_diamond_filled, paint_diamond_outline, paint_separator,
    paint_vertical_gradient, primary_button, rgba, sidebar_nav_btn, status_chip,
};
use khora_sdk::tool_ui::{FontFamilyHint, LinearRgba, TextAlign, UiBuilder};

enum ProjectAction {
    Open(RecentProject),
    AskRemove(usize),
    AddNativeCode(usize),
}

pub fn show_home(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    ui.left_inset_panel("hp_sidebar", 220.0, &mut |ui| show_sidebar(app, ui));
    ui.central_inset(&mut |ui| show_main(app, ui));

    if let Some(idx) = app.home.remove_confirm {
        show_remove_confirm_modal(app, ui, idx);
    }
}

fn show_sidebar(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    let r = ui.panel_rect();
    paint_vertical_gradient(ui, r, pal::SURFACE, pal::BG, 8);
    ui.paint_line(
        [r[0] + r[2], r[1]],
        [r[0] + r[2], r[1] + r[3]],
        rgba(tint(pal::BORDER, 0.55)),
        1.0,
    );

    ui.spacing(20.0);
    ui.indent("hp_side_actions", &mut |ui| {
        section_label(ui, "PROJECT ACTIONS");
        ui.spacing(8.0);
        if primary_button(ui, "hp-new", "+ New Project", [188.0, 32.0]).clicked {
            app.screen = Screen::NewProject;
        }
        ui.spacing(2.0);
        if ghost_button(ui, "hp-open-folder", "Open Folder…", [188.0, 28.0]).clicked
            && let Some(path) = rfd::FileDialog::new().pick_folder()
        {
            app.open_existing_project(path);
        }

        ui.spacing(18.0);
        paint_separator(ui, tint(pal::BORDER, 0.5));
        ui.spacing(14.0);
        section_label(ui, "VIEW");
        ui.spacing(6.0);

        if sidebar_nav_btn(ui, "hp-nav-projects", "Projects", app.screen == Screen::Home).clicked {
            app.screen = Screen::Home;
        }
        if sidebar_nav_btn(
            ui,
            "hp-nav-engines",
            "Engine Manager",
            app.screen == Screen::EngineManager,
        )
        .clicked
        {
            app.screen = Screen::EngineManager;
        }
        if sidebar_nav_btn(ui, "hp-nav-settings", "Settings", app.screen == Screen::Settings).clicked
        {
            app.settings.local_repo_draft = app.config.local_engine_repo.clone().unwrap_or_default();
            app.screen = Screen::Settings;
        }

        ui.spacing(20.0);
        match &app.settings.auth {
            crate::AuthState::Connected { login, .. } => {
                status_chip(ui, &format!("@{login}"), pal::SUCCESS)
            }
            crate::AuthState::Connecting { .. } => status_chip(ui, "Connecting…", pal::WARNING),
            crate::AuthState::Disconnected => status_chip(ui, "GitHub: offline", pal::TEXT_MUTED),
        }
    });
}

fn show_main(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    ui.spacing(20.0);
    ui.indent("hp_main", &mut |ui| {
        big_label(ui, "Recent Projects", 18.0, pal::TEXT);
        ui.spacing(6.0);

        ui.horizontal(&mut |row| {
            row.label("Filter:");
            row.text_edit_singleline(&mut app.home.filter);
        });
        ui.spacing(10.0);
        paint_separator(ui, tint(pal::SEPARATOR, 0.55));
        ui.spacing(12.0);

        if app.config.recent_projects.is_empty() {
            paint_empty_state(ui);
            ui.spacing(14.0);
            if primary_button(ui, "hp-empty-new", "+ New Project", [200.0, 34.0]).clicked {
                app.screen = Screen::NewProject;
            }
            return;
        }

        let needle = app.home.filter.to_ascii_lowercase();
        let projects: Vec<(usize, RecentProject)> = app
            .config
            .recent_projects
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, p)| {
                if needle.is_empty() {
                    true
                } else {
                    p.name.to_ascii_lowercase().contains(&needle)
                        || p.path.to_ascii_lowercase().contains(&needle)
                }
            })
            .collect();

        if projects.is_empty() {
            ui.colored_label(rgba(pal::TEXT_MUTED), "No projects match your filter.");
            return;
        }

        let mut action: Option<ProjectAction> = None;

        ui.scroll_area("hp-list", &mut |ui| {
            for (src_idx, proj) in projects.iter() {
                let hovered = app.home.hovered == Some(*src_idx);
                let int = project_card(ui, proj, *src_idx, hovered, &mut action);
                if int.hovered && app.home.hovered != Some(*src_idx) {
                    app.home.hovered = Some(*src_idx);
                } else if !int.hovered && app.home.hovered == Some(*src_idx) {
                    app.home.hovered = None;
                }
                ui.spacing(8.0);
            }
        });

        match action {
            Some(ProjectAction::Open(proj)) => app.launch_project(&proj),
            Some(ProjectAction::AskRemove(src_idx)) => {
                app.home.remove_confirm = Some(src_idx);
            }
            Some(ProjectAction::AddNativeCode(src_idx)) => {
                let proj = app.config.recent_projects.get(src_idx).cloned();
                if let Some(proj) = proj {
                    let root = std::path::PathBuf::from(&proj.path);
                    match crate::project::add_native_code(&root, &proj.name, &proj.engine_version)
                    {
                        Ok(()) => {
                            app.banner = Some(crate::Banner::info(format!(
                                "Added native Rust scaffold to '{}'.",
                                proj.name
                            )));
                        }
                        Err(e) => {
                            app.banner = Some(crate::Banner::error(format!(
                                "Add Native Code failed: {e:#}"
                            )));
                        }
                    }
                }
            }
            None => {}
        }
    });
}

fn project_card(
    ui: &mut dyn UiBuilder,
    proj: &RecentProject,
    src_idx: usize,
    hovered: bool,
    action: &mut Option<ProjectAction>,
) -> khora_sdk::tool_ui::Interaction {
    let r = ui.panel_rect();
    let card_w = (r[2] - 16.0).max(200.0);
    let card_h = 84.0;
    let pos = ui.cursor_pos();

    let card_fill = if hovered { pal::SURFACE3 } else { pal::SURFACE2 };
    let card_border = if hovered { pal::BORDER_LIGHT } else { pal::BORDER };
    let accent = if hovered { pal::PRIMARY } else { pal::PRIMARY_DIM };

    // Allocate the card-wide hit region FIRST so subsequent button
    // `interact_rect` calls take click priority over it (egui resolves
    // overlapping rects in last-allocated-wins order). Without this,
    // the card-wide rect would swallow every button click.
    let card_int = ui.interact_rect(
        &format!("hp-card-{}-bg", src_idx),
        [pos[0], pos[1], card_w, card_h],
    );

    ui.paint_rect_filled(pos, [card_w, card_h], rgba(card_fill), 6.0);
    ui.paint_rect_stroke(pos, [card_w, card_h], rgba(card_border), 6.0, 1.0);

    // Left accent stripe.
    ui.paint_rect_filled([pos[0], pos[1]], [3.0, card_h], rgba(accent), 2.0);

    // Diamond mark.
    paint_diamond_filled(ui, [pos[0] + 28.0, pos[1] + card_h * 0.5], 8.0, accent);

    // Title + path + meta.
    ui.paint_text_styled(
        [pos[0] + 50.0, pos[1] + 12.0],
        &proj.name,
        14.0,
        rgba(pal::TEXT),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
    ui.paint_text_styled(
        [pos[0] + 50.0, pos[1] + 32.0],
        &proj.path,
        11.0,
        rgba(pal::TEXT_MUTED),
        FontFamilyHint::Monospace,
        TextAlign::Left,
    );
    let badge_x = pos[0] + 50.0;
    let badge_y = pos[1] + 54.0;
    ui.region_at([badge_x, badge_y, 220.0, 22.0], &mut |ui| {
        badge(
            ui,
            &format!("v{}", proj.engine_version),
            tint(pal::PRIMARY, 0.18),
            pal::PRIMARY,
        )
    });
    ui.paint_text_styled(
        [badge_x + 70.0, badge_y + 4.0],
        &format_ts(proj.last_opened),
        11.0,
        rgba(pal::TEXT_DIM),
        FontFamilyHint::Monospace,
        TextAlign::Left,
    );

    // Right cluster.
    let btn_w = 80.0;
    let btn_h = 28.0;
    let btn_y = pos[1] + (card_h - btn_h) * 0.5;
    let mut x = pos[0] + card_w - 12.0 - btn_w;

    let salt_open = format!("hp-card-{}-open", src_idx);
    let int_open = ui.interact_rect(&salt_open, [x, btn_y, btn_w, btn_h]);
    let open_fill = if int_open.hovered {
        LinearRgba::new(
            (pal::PRIMARY.r * 1.08).min(1.0),
            (pal::PRIMARY.g * 1.08).min(1.0),
            (pal::PRIMARY.b * 1.08).min(1.0),
            pal::PRIMARY.a,
        )
    } else {
        pal::PRIMARY
    };
    ui.paint_rect_filled([x, btn_y], [btn_w, btn_h], rgba(open_fill), 5.0);
    let open_size = ui.measure_text("Open", 12.0, FontFamilyHint::Proportional);
    ui.paint_text_styled(
        [x + (btn_w - open_size[0]) * 0.5, btn_y + (btn_h - open_size[1]) * 0.5],
        "Open",
        12.0,
        rgba(pal::BG),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
    if int_open.clicked {
        *action = Some(ProjectAction::Open(proj.clone()));
    }
    x -= btn_w + 8.0;

    let salt_rm = format!("hp-card-{}-rm", src_idx);
    let int_rm = ui.interact_rect(&salt_rm, [x, btn_y, btn_w, btn_h]);
    let rm_fill = if int_rm.hovered { pal::SURFACE_ACTIVE } else { pal::SURFACE3 };
    ui.paint_rect_filled([x, btn_y], [btn_w, btn_h], rgba(rm_fill), 5.0);
    ui.paint_rect_stroke([x, btn_y], [btn_w, btn_h], rgba(pal::BORDER), 5.0, 1.0);
    let rm_size = ui.measure_text("Remove", 12.0, FontFamilyHint::Proportional);
    ui.paint_text_styled(
        [x + (btn_w - rm_size[0]) * 0.5, btn_y + (btn_h - rm_size[1]) * 0.5],
        "Remove",
        12.0,
        rgba(pal::TEXT_DIM),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
    if int_rm.clicked {
        *action = Some(ProjectAction::AskRemove(src_idx));
    }

    let project_root = std::path::Path::new(&proj.path);
    if crate::project::has_native_code(project_root) {
        x -= 90.0 + 8.0;
        ui.region_at([x, btn_y, 90.0, btn_h], &mut |ui| {
            ui.spacing(2.0);
            status_chip(ui, "Native ✓", pal::PRIMARY);
        });
    } else {
        let native_w = 132.0;
        x -= native_w + 8.0;
        let salt = format!("hp-card-{}-native", src_idx);
        let int = ui.interact_rect(&salt, [x, btn_y, native_w, btn_h]);
        let fill = if int.hovered { pal::SURFACE_ACTIVE } else { pal::SURFACE3 };
        ui.paint_rect_filled([x, btn_y], [native_w, btn_h], rgba(fill), 5.0);
        ui.paint_rect_stroke([x, btn_y], [native_w, btn_h], rgba(pal::BORDER), 5.0, 1.0);
        let s = ui.measure_text("Add Native Code", 12.0, FontFamilyHint::Proportional);
        ui.paint_text_styled(
            [x + (native_w - s[0]) * 0.5, btn_y + (btn_h - s[1]) * 0.5],
            "Add Native Code",
            12.0,
            rgba(pal::TEXT_DIM),
            FontFamilyHint::Proportional,
            TextAlign::Left,
        );
        if int.clicked {
            *action = Some(ProjectAction::AddNativeCode(src_idx));
        }
    }

    ui.spacing(card_h + 4.0);
    card_int
}

fn show_remove_confirm_modal(app: &mut HubApp, ui: &mut dyn UiBuilder, idx: usize) {
    let proj = match app.config.recent_projects.get(idx).cloned() {
        Some(p) => p,
        None => {
            app.home.remove_confirm = None;
            return;
        }
    };

    let mut do_delete = false;
    let mut close = false;

    ui.modal("hp-remove-confirm", [460.0, 200.0], &mut |ui| {
        ui.spacing(18.0);
        ui.indent("hp-modal-body", &mut |ui| {
            big_label(
                ui,
                &format!("Delete '{}' from disk?", proj.name),
                14.0,
                pal::TEXT,
            );
            ui.spacing(6.0);
            ui.colored_label(rgba(pal::TEXT_DIM), &format!("Path: {}", proj.path));
            ui.spacing(6.0);
            ui.colored_label(rgba(pal::WARNING), "This cannot be undone.");
            ui.spacing(20.0);

            ui.horizontal(&mut |row| {
                if ghost_button(row, "hp-modal-cancel", "Cancel", [100.0, 32.0]).clicked {
                    close = true;
                }
                if ghost_button(row, "hp-modal-delete", "Delete", [100.0, 32.0]).clicked {
                    do_delete = true;
                }
            });
        });
    });

    if do_delete {
        let path = std::path::PathBuf::from(&proj.path);
        if path.exists() {
            match std::fs::remove_dir_all(&path) {
                Ok(()) => {
                    app.banner = Some(crate::Banner::info(format!("Deleted '{}'", proj.name)));
                }
                Err(e) => {
                    app.banner = Some(crate::Banner::error(format!(
                        "Failed to delete '{}': {e}",
                        proj.path
                    )));
                }
            }
        }
        app.config.recent_projects.remove(idx);
        let _ = app.config.save();
        app.home.remove_confirm = None;
    } else if close {
        app.home.remove_confirm = None;
    }
}

fn paint_empty_state(ui: &mut dyn UiBuilder) {
    ui.spacing(40.0);
    let r = ui.panel_rect();
    let cx = r[0] + r[2] * 0.5;
    let cy = ui.cursor_pos()[1] + 36.0;
    paint_diamond_outline(ui, [cx, cy], 24.0, tint(pal::PRIMARY, 0.35), 1.5);
    paint_diamond_filled(ui, [cx, cy], 8.0, pal::PRIMARY_DIM);
    ui.spacing(80.0);
    let label = "No projects yet";
    let label_size = ui.measure_text(label, 15.0, FontFamilyHint::Proportional);
    ui.paint_text_styled(
        [cx - label_size[0] * 0.5, ui.cursor_pos()[1]],
        label,
        15.0,
        rgba(pal::TEXT_DIM),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
    ui.spacing(24.0);
    let sub = "Create your first project to get started.";
    let sub_size = ui.measure_text(sub, 12.0, FontFamilyHint::Proportional);
    ui.paint_text_styled(
        [cx - sub_size[0] * 0.5, ui.cursor_pos()[1]],
        sub,
        12.0,
        rgba(pal::TEXT_MUTED),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
    ui.spacing(20.0);
}

fn section_label(ui: &mut dyn UiBuilder, text: &str) {
    let pos = ui.cursor_pos();
    ui.paint_text_styled(
        pos,
        text,
        11.0,
        rgba(pal::TEXT_MUTED),
        FontFamilyHint::Monospace,
        TextAlign::Left,
    );
    ui.spacing(16.0);
}

fn big_label(ui: &mut dyn UiBuilder, text: &str, size: f32, color: LinearRgba) {
    let pos = ui.cursor_pos();
    ui.paint_text_styled(
        pos,
        text,
        size,
        rgba(color),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
    ui.spacing(size + 6.0);
}
