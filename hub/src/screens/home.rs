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

enum ProjectAction {
    Open(RecentProject),
    Remove(usize),
}

pub fn show_home(app: &mut HubApp, ctx: &egui::Context) {
    // ── Left sidebar ──────────────────────────────────────────
    egui::SidePanel::left("hp_sidebar")
        .exact_width(220.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.add_space(24.0);
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                paint_khora_star(ui.painter(), rect.center(), 12.0, pal::PRIMARY);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new("KhoraEngine")
                            .strong()
                            .size(15.0)
                            .color(pal::TEXT),
                    );
                    ui.label(
                        egui::RichText::new("Hub v0.1")
                            .size(11.0)
                            .color(pal::TEXT_MUTED),
                    );
                });
            });

            ui.add_space(24.0);
            paint_separator(ui, pal::BORDER);
            ui.add_space(12.0);

            sidebar_nav_btn(ui, "Projects", app.screen == Screen::Home);

            ui.add_space(6.0);
            ui.add_space(12.0);
            paint_separator(ui, pal::BORDER);
            ui.add_space(12.0);

            // New project CTA
            let new_btn = ui.add_sized(
                [180.0, 34.0],
                egui::Button::new(
                    egui::RichText::new("+ New Project")
                        .size(13.0)
                        .color(egui::Color32::WHITE),
                )
                .fill(pal::PRIMARY)
                .stroke(egui::Stroke::NONE),
            );
            if new_btn.clicked() {
                app.screen = Screen::NewProject;
            }

            ui.add_space(6.0);

            // Open folder
            let open_btn = ui.add_sized(
                [180.0, 30.0],
                egui::Button::new(
                    egui::RichText::new("Open Folder...")
                        .size(12.0)
                        .color(pal::TEXT),
                )
                .fill(pal::SURFACE3)
                .stroke(egui::Stroke::new(1.0, pal::BORDER)),
            );
            if open_btn.clicked()
                && let Some(path) = rfd::FileDialog::new().pick_folder()
            {
                app.open_existing_project(path);
            }

            let h = ui.available_height();
            ui.add_space((h - 56.0).max(12.0));

            paint_separator(ui, pal::BORDER);
            ui.add_space(8.0);

            let eng_btn = ui.add_sized(
                [180.0, 30.0],
                egui::Button::new(
                    egui::RichText::new("Engine Manager")
                        .size(12.0)
                        .color(pal::TEXT_DIM),
                )
                .fill(egui::Color32::TRANSPARENT)
                .stroke(egui::Stroke::NONE),
            );
            if eng_btn.hovered() {
                ctx.request_repaint();
            }
            if eng_btn.clicked() {
                app.screen = Screen::EngineManager;
            }
            ui.add_space(8.0);
        });

    // ── Main content ──────────────────────────────────────────
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(24.0);

        // Banner
        if let Some(banner) = app.banner.as_ref() {
            let mut dismiss_banner = false;
            let (bg, fg) = if banner.is_error {
                (egui::Color32::from_rgb(60, 20, 18), pal::ERROR)
            } else {
                (egui::Color32::from_rgb(18, 48, 30), pal::SUCCESS)
            };
            egui::Frame::none()
                .fill(bg)
                .stroke(egui::Stroke::new(1.0, fg.gamma_multiply(0.5)))
                .rounding(egui::Rounding::same(6_f32))
                .inner_margin(egui::Margin {
                    left: 12.0,
                    right: 12.0,
                    top: 8.0,
                    bottom: 8.0,
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&banner.message).size(12.0).color(fg));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("x").clicked() {
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

        // Section header
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Recent Projects")
                    .strong()
                    .size(16.0)
                    .color(pal::TEXT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{} project(s)", app.config.recent_projects.len()))
                        .size(11.0)
                        .color(pal::TEXT_MUTED),
                );
            });
        });
        ui.add_space(12.0);

        if app.config.recent_projects.is_empty() {
            ui.add_space(48.0);
            ui.vertical_centered(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
                paint_khora_star(ui.painter(), rect.center(), 20.0, pal::TEXT_MUTED);
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new("No projects yet")
                        .size(15.0)
                        .color(pal::TEXT_DIM),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Click \"+ New Project\" in the sidebar to get started.")
                        .size(12.0)
                        .color(pal::TEXT_MUTED),
                );
            });
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut action: Option<ProjectAction> = None;
            let projects = app.config.recent_projects.clone();

            for (i, proj) in projects.iter().enumerate() {
                let is_hovered = app.home.hovered == Some(i);
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

                let resp = egui::Frame::none()
                    .fill(card_fill)
                    .stroke(egui::Stroke::new(1.0, card_border))
                    .rounding(egui::Rounding::same(8_f32))
                    .inner_margin(egui::Margin {
                        left: 14.0,
                        right: 14.0,
                        top: 10.0,
                        bottom: 10.0,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Left accent bar
                            let bar_rect = egui::Rect::from_min_size(
                                ui.next_widget_position() - egui::vec2(14.0, 10.0),
                                egui::vec2(3.0, 50.0),
                            );
                            ui.painter().rect_filled(
                                bar_rect,
                                egui::Rounding::same(2_f32),
                                if is_hovered {
                                    pal::PRIMARY
                                } else {
                                    pal::PRIMARY_DIM
                                },
                            );

                            ui.add_space(8.0);

                            let (icon_rect, _) = ui
                                .allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                            paint_khora_star(
                                ui.painter(),
                                icon_rect.center(),
                                8.0,
                                if is_hovered {
                                    pal::PRIMARY
                                } else {
                                    pal::TEXT_MUTED
                                },
                            );
                            ui.add_space(8.0);

                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new(&proj.name)
                                        .strong()
                                        .size(13.5)
                                        .color(pal::TEXT),
                                );
                                ui.add_space(2.0);
                                ui.label(
                                    egui::RichText::new(&proj.path)
                                        .size(11.0)
                                        .color(pal::TEXT_MUTED),
                                );
                                ui.add_space(2.0);
                                ui.horizontal(|ui| {
                                    badge(
                                        ui,
                                        &format!("v{}", proj.engine_version),
                                        pal::PRIMARY_DIM,
                                        pal::PRIMARY,
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new(format_ts(proj.last_opened))
                                            .size(10.5)
                                            .color(pal::TEXT_MUTED),
                                    );
                                });
                            });

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add_space(4.0);
                                    let del = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new("Remove")
                                                .size(11.0)
                                                .color(pal::TEXT_MUTED),
                                        )
                                        .fill(egui::Color32::TRANSPARENT)
                                        .stroke(egui::Stroke::NONE),
                                    );
                                    if del.clicked() {
                                        action = Some(ProjectAction::Remove(i));
                                    }
                                    ui.add_space(8.0);
                                    let open = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new("Open")
                                                .size(12.0)
                                                .color(egui::Color32::WHITE),
                                        )
                                        .fill(pal::PRIMARY)
                                        .stroke(egui::Stroke::NONE),
                                    );
                                    if open.clicked() {
                                        action = Some(ProjectAction::Open(proj.clone()));
                                    }
                                },
                            );
                        });
                    });

                if resp.response.hovered() {
                    app.home.hovered = Some(i);
                    ctx.request_repaint();
                } else if app.home.hovered == Some(i) {
                    app.home.hovered = None;
                    ctx.request_repaint();
                }

                ui.add_space(6.0);
            }

            match action {
                Some(ProjectAction::Open(proj)) => app.launch_project(&proj),
                Some(ProjectAction::Remove(i)) => {
                    app.config.recent_projects.remove(i);
                    let _ = app.config.save();
                }
                None => {}
            }
        });
    });
}
