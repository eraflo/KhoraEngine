// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! New Project screen — create a new Khora Engine project.

use crate::HubApp;
use crate::Screen;
use crate::project;
use crate::theme::pal;
use crate::widgets::*;
use eframe::egui;
use std::path::PathBuf;

pub fn show_new_project(app: &mut HubApp, ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(28.0);
        ui.horizontal(|ui| {
            ui.add_space(32.0);
            ui.vertical(|ui| {
                let back = ui.add(
                    egui::Button::new(
                        egui::RichText::new("< Back").size(12.0).color(pal::TEXT_DIM),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::NONE),
                );
                if back.clicked() {
                    app.screen = Screen::Home;
                    app.new_project.status = None;
                }
                ui.add_space(16.0);

                ui.label(
                    egui::RichText::new("New Project")
                        .strong()
                        .size(22.0)
                        .color(pal::TEXT),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Configure your project and create it in one click.")
                        .size(12.0)
                        .color(pal::TEXT_MUTED),
                );
                ui.add_space(20.0);

                // Card wrapper
                egui::Frame::none()
                    .fill(pal::SURFACE2)
                    .stroke(egui::Stroke::new(1.0, pal::BORDER))
                    .rounding(egui::Rounding::same(10_f32))
                    .inner_margin(egui::Margin {
                        left: 20.0,
                        right: 20.0,
                        top: 18.0,
                        bottom: 18.0,
                    })
                    .show(ui, |ui| {
                        ui.set_min_width(480.0);
                        ui.set_max_width(600.0);

                        field_label(ui, "Project Name");
                        ui.add_space(4.0);
                        let name_edit = egui::TextEdit::singleline(&mut app.new_project.name)
                            .hint_text("e.g. MyGame")
                            .desired_width(f32::INFINITY);
                        ui.add(name_edit);
                        ui.add_space(12.0);

                        field_label(ui, "Location");
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let path_edit = egui::TextEdit::singleline(&mut app.new_project.path)
                                .desired_width(340.0);
                            ui.add(path_edit);
                            ui.add_space(4.0);
                            let browse = ui.add(
                                egui::Button::new(
                                    egui::RichText::new("Browse").size(12.0).color(pal::TEXT),
                                )
                                .fill(pal::SURFACE3)
                                .stroke(egui::Stroke::new(1.0, pal::BORDER_LIGHT)),
                            );
                            if browse.clicked()
                                && let Some(path) = rfd::FileDialog::new().pick_folder() {
                                    app.new_project.path = path.to_string_lossy().to_string();
                                }
                        });

                        let engines = app.available_engines();
                        ui.add_space(12.0);
                        field_label(ui, "Engine Version");
                        ui.add_space(4.0);
                        if engines.is_empty() {
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgb(50, 38, 18))
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    pal::WARNING.gamma_multiply(0.4),
                                ))
                                .rounding(egui::Rounding::same(5_f32))
                                .inner_margin(egui::Margin {
                                    left: 10.0,
                                    right: 10.0,
                                    top: 6.0,
                                    bottom: 6.0,
                                })
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(
                                            "No engine configured. Go to Engine Manager.",
                                        )
                                        .size(11.5)
                                        .color(pal::WARNING),
                                    );
                                });
                        } else {
                            egui::ComboBox::from_id_salt("np_engine")
                                .selected_text(
                                    &engines[app.new_project.engine_idx.min(engines.len() - 1)].version,
                                )
                                .show_ui(ui, |ui| {
                                    for (i, e) in engines.iter().enumerate() {
                                        let label = format!("{} ({})", e.version, e.source);
                                        ui.selectable_value(&mut app.new_project.engine_idx, i, &label);
                                    }
                                });
                        }

                        if !app.new_project.name.is_empty() && !app.new_project.path.is_empty() {
                            ui.add_space(12.0);
                            let preview = PathBuf::from(&app.new_project.path)
                                .join(&app.new_project.name)
                                .to_string_lossy()
                                .to_string();
                            ui.label(
                                egui::RichText::new(format!("Will create: {}", preview))
                                    .size(11.0)
                                    .color(pal::TEXT_MUTED),
                            );
                        }

                        ui.add_space(20.0);
                        paint_separator(ui, pal::BORDER);
                        ui.add_space(14.0);

                        let can_create = !app.new_project.name.is_empty()
                            && !app.new_project.path.is_empty()
                            && !engines.is_empty();
                        ui.add_enabled_ui(can_create, |ui| {
                            let create_btn = ui.add_sized(
                                [160.0, 36.0],
                                egui::Button::new(
                                    egui::RichText::new("Create Project")
                                        .size(13.0)
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(if can_create {
                                    pal::PRIMARY
                                } else {
                                    pal::SURFACE3
                                })
                                .stroke(egui::Stroke::NONE),
                            );
                            if create_btn.clicked() {
                                let engine_version = engines
                                    .get(app.new_project.engine_idx)
                                    .map(|e| e.version.clone())
                                    .unwrap_or_else(|| "dev".to_owned());

                                match project::create_project(
                                    &app.new_project.name,
                                    &PathBuf::from(&app.new_project.path),
                                    &engine_version,
                                ) {
                                    Ok(root) => {
                                        app.new_project.success = true;
                                        app.new_project.status = Some(format!(
                                            "Project created at: {}",
                                            root.display()
                                        ));
                                        app.config.push_recent(
                                            &app.new_project.name,
                                            &root,
                                            &engine_version,
                                        );
                                        let _ = app.config.save();

                                        if let Some(engine) = engines.get(app.new_project.engine_idx) {
                                            match project::launch_editor(
                                                &engine.editor_binary,
                                                &root,
                                            ) {
                                                Ok(()) => {
                                                    app.banner = Some(crate::Banner {
                                                        message: "Editor launched!".to_owned(),
                                                        is_error: false,
                                                    });
                                                }
                                                Err(e) => {
                                                    app.banner = Some(crate::Banner {
                                                        message: format!(
                                                            "Project created but could not launch editor: {}",
                                                            e
                                                        ),
                                                        is_error: true,
                                                    });
                                                }
                                            }
                                            app.screen = Screen::Home;
                                        } else {
                                            app.screen = Screen::Home;
                                        }
                                    }
                                    Err(e) => {
                                        app.new_project.success = false;
                                        app.new_project.status = Some(format!("Error: {}", e));
                                    }
                                }
                            }
                        });

                        if let Some(ref status) = app.new_project.status.clone() {
                            ui.add_space(10.0);
                            let (bg, fg) = if app.new_project.success {
                                (egui::Color32::from_rgb(18, 48, 30), pal::SUCCESS)
                            } else {
                                (egui::Color32::from_rgb(60, 20, 18), pal::ERROR)
                            };
                            egui::Frame::none()
                                .fill(bg)
                                .stroke(egui::Stroke::new(1.0, fg.gamma_multiply(0.4)))
                                .rounding(egui::Rounding::same(5_f32))
                                .inner_margin(egui::Margin {
                                    left: 10.0,
                                    right: 10.0,
                                    top: 6.0,
                                    bottom: 6.0,
                                })
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(status).size(11.5).color(fg),
                                    );
                                });
                        }
                    });
            });
        });
    });
}
