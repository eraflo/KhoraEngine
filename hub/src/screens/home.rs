// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Home screen — project list and sidebar.

use crate::HubApp;
use crate::Screen;
use crate::config::RecentProject;
use crate::theme::pal;
use crate::widgets::*;
use eframe::egui;

/// Action emitted by an interaction with a project card. Returned out of the
/// per-frame loop so we can mutate `HubApp` after the borrow ends.
enum ProjectAction {
    Open(RecentProject),
    /// Open the confirmation modal for this index (in the source list).
    AskRemove(usize),
    /// Scaffolds Cargo.toml + src/main.rs into the project at the given
    /// source-list index, opting it into native-Rust mode. Build Game then
    /// switches from "stamp khora-runtime" to "cargo build" for that
    /// project.
    AddNativeCode(usize),
}

pub fn show_home(app: &mut HubApp, parent_ui: &mut egui::Ui) {
    let ctx = parent_ui.ctx().clone();
    show_sidebar(app, parent_ui);
    show_main(app, parent_ui);

    if let Some(idx) = app.home.remove_confirm {
        show_remove_confirm_modal(app, &ctx, idx);
    }
}

fn show_sidebar(app: &mut HubApp, parent_ui: &mut egui::Ui) {
    egui::Panel::left("hp_sidebar")
        .exact_size(220.0)
        .resizable(false)
        .frame(egui::Frame::new())
        .show_inside(parent_ui, |ui| {
            // Subtle gradient backdrop, like the editor's panel headers.
            let r = ui.max_rect();
            paint_vertical_gradient(ui.painter(), r, pal::SURFACE, pal::BG, 8);
            ui.painter().line_segment(
                [r.right_top(), r.right_bottom()],
                egui::Stroke::new(1.0, tint(pal::BORDER, 0.55)),
            );

            ui.add_space(20.0);
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("PROJECT ACTIONS")
                            .size(11.0)
                            .color(pal::TEXT_MUTED)
                            .monospace(),
                    );
                });
            });
            ui.add_space(8.0);

            ui.allocate_ui_with_layout(
                egui::vec2(220.0, 0.0),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    if primary_button(ui, "+ New Project", [188.0, 32.0]).clicked() {
                        app.screen = Screen::NewProject;
                    }
                    ui.add_space(6.0);
                    if ghost_button(ui, "Open Folder…", [188.0, 28.0]).clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_folder()
                    {
                        app.open_existing_project(path);
                    }
                },
            );

            ui.add_space(18.0);
            paint_separator(ui, tint(pal::BORDER, 0.5));
            ui.add_space(14.0);

            ui.horizontal(|ui| {
                ui.add_space(20.0);
                ui.label(
                    egui::RichText::new("VIEW")
                        .size(11.0)
                        .color(pal::TEXT_MUTED)
                        .monospace(),
                );
            });
            ui.add_space(6.0);

            ui.allocate_ui_with_layout(
                egui::vec2(220.0, 0.0),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    if sidebar_nav_btn(ui, "Projects", app.screen == Screen::Home).clicked() {
                        app.screen = Screen::Home;
                    }
                    if sidebar_nav_btn(ui, "Engine Manager", app.screen == Screen::EngineManager)
                        .clicked()
                    {
                        app.screen = Screen::EngineManager;
                    }
                    if sidebar_nav_btn(ui, "Settings", app.screen == Screen::Settings).clicked() {
                        app.settings.local_repo_draft =
                            app.config.local_engine_repo.clone().unwrap_or_default();
                        app.screen = Screen::Settings;
                    }
                },
            );

            // Footer: GitHub auth chip pinned to the bottom.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    match &app.settings.auth {
                        crate::AuthState::Connected { login, .. } => {
                            status_chip(ui, &format!("@{login}"), pal::SUCCESS);
                        }
                        crate::AuthState::Connecting { .. } => {
                            status_chip(ui, "Connecting…", pal::WARNING);
                        }
                        crate::AuthState::Disconnected => {
                            status_chip(ui, "GitHub: offline", pal::TEXT_MUTED);
                        }
                    }
                });
                ui.add_space(10.0);
                paint_separator(ui, tint(pal::BORDER, 0.5));
            });
        });
}

fn show_main(app: &mut HubApp, parent_ui: &mut egui::Ui) {
    egui::CentralPanel::default().show_inside(parent_ui, |ui| {
        ui.add_space(20.0);

        // Banner — slim, with side accent stripe (mirrors editor logger lines).
        if let Some(banner) = app.banner.as_ref() {
            let mut dismiss_banner = false;
            let fg = if banner.is_error {
                pal::ERROR
            } else {
                pal::SUCCESS
            };
            egui::Frame::new()
                .fill(tint(fg, 0.12))
                .stroke(egui::Stroke::new(1.0, tint(fg, 0.45)))
                .corner_radius(egui::CornerRadius::same(5))
                .inner_margin(egui::Margin {
                    left: 12,
                    right: 10,
                    top: 7,
                    bottom: 7,
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (dot, _) =
                            ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        ui.painter().circle_filled(dot.center(), 3.5, fg);
                        ui.painter()
                            .circle_filled(dot.center(), 5.5, tint(fg, 0.18));
                        ui.label(egui::RichText::new(&banner.message).size(12.0).color(fg));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("×").size(13.0).color(fg),
                                    )
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                                )
                                .clicked()
                            {
                                dismiss_banner = true;
                            }
                        });
                    });
                });
            if dismiss_banner {
                app.banner = None;
            }
            ui.add_space(12.0);
        }

        // Header + search.
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Recent Projects")
                    .strong()
                    .size(18.0)
                    .color(pal::TEXT),
            );
            ui.add_space(12.0);
            ui.add(
                egui::TextEdit::singleline(&mut app.home.filter)
                    .hint_text("Filter by name or path…")
                    .desired_width(220.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{} projects", app.config.recent_projects.len()))
                        .size(11.0)
                        .color(pal::TEXT_MUTED)
                        .monospace(),
                );
            });
        });
        ui.add_space(10.0);
        paint_separator(ui, tint(pal::SEPARATOR, 0.55));
        ui.add_space(12.0);

        // Empty state.
        if app.config.recent_projects.is_empty() {
            ui.add_space(64.0);
            ui.vertical_centered(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(72.0, 72.0), egui::Sense::hover());
                paint_diamond_outline(
                    ui.painter(),
                    rect.center(),
                    24.0,
                    tint(pal::PRIMARY, 0.35),
                    1.5,
                );
                paint_diamond_filled(ui.painter(), rect.center(), 8.0, pal::PRIMARY_DIM);
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new("No projects yet")
                        .strong()
                        .size(15.0)
                        .color(pal::TEXT_DIM),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Create your first project to get started.")
                        .size(12.0)
                        .color(pal::TEXT_MUTED),
                );
                ui.add_space(16.0);
                if primary_button(ui, "+ New Project", [200.0, 34.0]).clicked() {
                    app.screen = Screen::NewProject;
                }
            });
            return;
        }

        // Filtered list of (source_index, project) pairs.
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
            ui.add_space(32.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("No projects match your filter.")
                        .size(13.0)
                        .color(pal::TEXT_MUTED),
                );
            });
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut action: Option<ProjectAction> = None;

            for (src_idx, proj) in projects.iter() {
                let is_hovered = app.home.hovered == Some(*src_idx);
                let card_fill = if is_hovered {
                    pal::SURFACE3
                } else {
                    pal::SURFACE2
                };
                let card_border = if is_hovered {
                    pal::BORDER_LIGHT
                } else {
                    pal::BORDER
                };
                let accent = if is_hovered {
                    pal::PRIMARY
                } else {
                    pal::PRIMARY_DIM
                };

                let frame_resp = egui::Frame::new()
                    .fill(card_fill)
                    .stroke(egui::Stroke::new(1.0, card_border))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin {
                        left: 0,
                        right: 12,
                        top: 10,
                        bottom: 10,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Left accent stripe (3 px) — full card height.
                            let (stripe, _) =
                                ui.allocate_exact_size(egui::vec2(3.0, 56.0), egui::Sense::hover());
                            ui.painter()
                                .rect_filled(stripe, egui::CornerRadius::same(2), accent);
                            ui.add_space(14.0);

                            // Diamond mark, vertically centered against the row.
                            let (icon_rect, _) = ui
                                .allocate_exact_size(egui::vec2(22.0, 56.0), egui::Sense::hover());
                            paint_diamond_filled(ui.painter(), icon_rect.center(), 8.0, accent);
                            ui.add_space(10.0);

                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new(&proj.name)
                                        .strong()
                                        .size(14.0)
                                        .color(pal::TEXT),
                                );
                                ui.add_space(1.0);
                                ui.label(
                                    egui::RichText::new(&proj.path)
                                        .size(11.0)
                                        .color(pal::TEXT_MUTED)
                                        .monospace(),
                                );
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    badge(
                                        ui,
                                        &format!("v{}", proj.engine_version),
                                        tint(pal::PRIMARY, 0.18),
                                        pal::PRIMARY,
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new(format_ts(proj.last_opened))
                                            .size(11.0)
                                            .color(pal::TEXT_MUTED)
                                            .monospace(),
                                    );
                                });
                            });

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ghost_button(ui, "Remove", [80.0, 28.0]).clicked() {
                                        action = Some(ProjectAction::AskRemove(*src_idx));
                                    }
                                    ui.add_space(8.0);
                                    if primary_button(ui, "Open", [80.0, 28.0]).clicked() {
                                        action = Some(ProjectAction::Open(proj.clone()));
                                    }
                                    ui.add_space(8.0);
                                    // Native-code toggle: shown as a small
                                    // status chip when already enabled, as
                                    // a clickable ghost button otherwise.
                                    let project_root = std::path::Path::new(&proj.path);
                                    if crate::project::has_native_code(project_root) {
                                        status_chip(ui, "Native ✓", pal::PRIMARY);
                                    } else if ghost_button(ui, "Add Native Code", [130.0, 28.0])
                                        .clicked()
                                    {
                                        action = Some(ProjectAction::AddNativeCode(*src_idx));
                                    }
                                },
                            );
                        });
                    });

                // Hover state — sense over the frame's response (no ui.interact
                // overlay, which would otherwise swallow clicks intended for
                // the inner buttons).
                let hovered_now =
                    frame_resp.response.hovered() || frame_resp.response.contains_pointer();
                if hovered_now {
                    if app.home.hovered != Some(*src_idx) {
                        app.home.hovered = Some(*src_idx);
                        ui.ctx().request_repaint();
                    }
                } else if app.home.hovered == Some(*src_idx) {
                    app.home.hovered = None;
                    ui.ctx().request_repaint();
                }

                ui.add_space(8.0);
            }

            match action {
                Some(ProjectAction::Open(proj)) => app.launch_project(&proj),
                Some(ProjectAction::AskRemove(src_idx)) => {
                    app.home.remove_confirm = Some(src_idx);
                }
                Some(ProjectAction::AddNativeCode(src_idx)) => {
                    let proj = app.config.recent_projects.get(src_idx).cloned();
                    if let Some(proj) = proj {
                        let root = std::path::PathBuf::from(&proj.path);
                        match crate::project::add_native_code(
                            &root,
                            &proj.name,
                            &proj.engine_version,
                        ) {
                            Ok(()) => {
                                app.banner = Some(crate::Banner::info(format!(
                                    "Added native Rust scaffold to '{}'. Build Game will \
                                     now use cargo build.",
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
    });
}

fn show_remove_confirm_modal(app: &mut HubApp, ctx: &egui::Context, idx: usize) {
    // Resolve the project before painting (modal closes on confirm/cancel).
    let proj = match app.config.recent_projects.get(idx).cloned() {
        Some(p) => p,
        None => {
            app.home.remove_confirm = None;
            return;
        }
    };

    let mut close = false;
    let mut do_delete = false;

    egui::Window::new("Delete project")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            egui::Frame::window(&ctx.global_style())
                .fill(pal::SURFACE2)
                .stroke(egui::Stroke::new(1.0, pal::BORDER_LIGHT)),
        )
        .show(ctx, |ui| {
            ui.set_min_width(420.0);
            ui.label(
                egui::RichText::new(format!("Delete '{}' from disk?", proj.name))
                    .strong()
                    .size(14.0)
                    .color(pal::TEXT),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!(
                    "This will permanently delete:\n{}\n\nThis cannot be undone.",
                    proj.path
                ))
                .size(12.0)
                .color(pal::TEXT_DIM),
            );
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ghost_button(ui, "Cancel", [100.0, 32.0]).clicked() {
                    close = true;
                }
                ui.add_space(8.0);
                if danger_button(ui, "Delete", [100.0, 32.0]).clicked() {
                    do_delete = true;
                }
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
        } else {
            // Already gone — silent no-op, just remove from the list.
            log::info!("Project path '{}' already absent on disk", proj.path);
        }
        app.config.recent_projects.remove(idx);
        let _ = app.config.save();
        app.home.remove_confirm = None;
    } else if close {
        app.home.remove_confirm = None;
    }
}
