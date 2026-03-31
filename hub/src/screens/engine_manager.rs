// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Engine Manager screen — manage local builds and downloaded engines.

use crate::HubApp;
use crate::Screen;
use crate::download;
use crate::github;
use crate::theme::pal;
use crate::widgets::*;
use eframe::egui;
use std::path::PathBuf;

pub fn show_engine_manager(app: &mut HubApp, ctx: &egui::Context) {
    egui::CentralPanel::default()
        .frame(egui::Frame::none().inner_margin(egui::Margin {
            left: 32.0,
            right: 32.0,
            top: 28.0,
            bottom: 0.0,
        }))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("engine_manager_scroll")
                .show(ui, |ui| {
                let back = ui.add(
                    egui::Button::new(
                        egui::RichText::new("< Back").size(12.0).color(pal::TEXT_DIM),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::NONE),
                );
                if back.clicked() {
                    app.screen = Screen::Home;
                    if app.engine_manager.local_repo.is_empty() {
                        app.config.local_engine_repo = None;
                    } else {
                        app.config.local_engine_repo = Some(app.engine_manager.local_repo.clone());
                    }
                    let _ = app.config.save();
                }
                ui.add_space(16.0);

                ui.label(
                    egui::RichText::new("Engine Manager")
                        .strong()
                        .size(22.0)
                        .color(pal::TEXT),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Manage local builds and downloaded engine versions.")
                        .size(12.0)
                        .color(pal::TEXT_MUTED),
                );
                ui.add_space(20.0);

                // ── Local build card ──────────────────────────────
                section_header(ui, "Local Development Build");
                ui.add_space(8.0);

                egui::Frame::none()
                    .fill(pal::SURFACE2)
                    .stroke(egui::Stroke::new(1.0, pal::BORDER))
                    .rounding(egui::Rounding::same(10_f32))
                    .inner_margin(egui::Margin {
                        left: 18.0,
                        right: 18.0,
                        top: 14.0,
                        bottom: 14.0,
                    })
                    .show(ui, |ui| {
                        ui.set_min_width(520.0);
                        ui.set_max_width(680.0);
                        ui.label(
                            egui::RichText::new(
                                "Point to your local KhoraEngine repository to use the debug build.",
                            )
                            .size(12.0)
                            .color(pal::TEXT_DIM),
                        );
                        ui.add_space(10.0);
                        field_label(ui, "Repository Path");
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let edit = egui::TextEdit::singleline(&mut app.engine_manager.local_repo)
                                .hint_text("e.g. C:/Dev/KhoraEngine")
                                .desired_width(380.0);
                            ui.add(edit);
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
                                    app.engine_manager.local_repo = path.to_string_lossy().to_string();
                                }
                        });
                        ui.add_space(8.0);

                        let local_engine_available = !app.engine_manager.local_repo.is_empty() && {
                            let exe = if cfg!(windows) {
                                "khora-editor.exe"
                            } else {
                                "khora-editor"
                            };
                            PathBuf::from(&app.engine_manager.local_repo)
                                .join("target")
                                .join("debug")
                                .join(exe)
                                .exists()
                        };

                        if local_engine_available {
                            status_chip(ui, "Editor binary found", pal::SUCCESS);
                        } else if !app.engine_manager.local_repo.is_empty() {
                            status_chip(
                                ui,
                                "Binary not found - run `cargo build -p khora-editor`",
                                pal::WARNING,
                            );
                        }

                        ui.add_space(12.0);
                        let save_btn = ui.add(
                            egui::Button::new(
                                egui::RichText::new("Save Path")
                                    .size(12.0)
                                    .color(egui::Color32::WHITE),
                            )
                            .fill(pal::PRIMARY)
                            .stroke(egui::Stroke::NONE),
                        );
                        if save_btn.clicked() {
                            if app.engine_manager.local_repo.is_empty() {
                                app.config.local_engine_repo = None;
                            } else {
                                app.config.local_engine_repo = Some(app.engine_manager.local_repo.clone());
                            }
                            let _ = app.config.save();
                            app.banner = Some(crate::Banner {
                                message: "Local engine path saved.".to_owned(),
                                is_error: false,
                            });
                        }
                    });

                ui.add_space(20.0);

                // ── Installed engines ─────────────────────────────
                section_header(ui, "Installed Engines");
                ui.add_space(8.0);

                if app.config.engines.is_empty() {
                    egui::Frame::none()
                        .fill(pal::SURFACE2)
                        .stroke(egui::Stroke::new(1.0, pal::BORDER))
                        .rounding(egui::Rounding::same(8_f32))
                        .inner_margin(egui::Margin {
                            left: 18.0,
                            right: 18.0,
                            top: 12.0,
                            bottom: 12.0,
                        })
                        .show(ui, |ui| {
                            ui.set_min_width(520.0);
                            ui.label(
                                egui::RichText::new(
                                    "No downloaded engines. Check for updates below.",
                                )
                                .size(12.0)
                                .color(pal::TEXT_MUTED),
                            );
                        });
                } else {
                    for engine in &app.config.engines {
                        egui::Frame::none()
                            .fill(pal::SURFACE2)
                            .stroke(egui::Stroke::new(1.0, pal::BORDER))
                            .rounding(egui::Rounding::same(6_f32))
                            .inner_margin(egui::Margin {
                                left: 14.0,
                                right: 14.0,
                                top: 8.0,
                                bottom: 8.0,
                            })
                            .show(ui, |ui| {
                                ui.set_min_width(520.0);
                                ui.horizontal(|ui| {
                                    badge(ui, &engine.version, pal::PRIMARY_DIM, pal::PRIMARY);
                                    ui.add_space(8.0);
                                    ui.label(
                                        egui::RichText::new(&engine.source)
                                            .size(11.0)
                                            .color(pal::TEXT_DIM),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(&engine.editor_binary)
                                                    .size(10.0)
                                                    .color(pal::TEXT_MUTED),
                                            );
                                        },
                                    );
                                });
                            });
                        ui.add_space(4.0);
                    }
                }

                ui.add_space(20.0);

                // ── GitHub releases ───────────────────────────────
                section_header(ui, "GitHub Releases");
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    let fetch_label = if app.engine_manager.fetching {
                        "Fetching..."
                    } else {
                        "Check for Updates"
                    };
                    let fetch_btn = ui.add_enabled(
                        !app.engine_manager.fetching,
                        egui::Button::new(
                            egui::RichText::new(fetch_label)
                                .size(12.0)
                                .color(egui::Color32::WHITE),
                        )
                        .fill(pal::ACCENT)
                        .stroke(egui::Stroke::NONE),
                    );
                    if fetch_btn.clicked() {
                        app.engine_manager.fetching = true;
                        app.engine_manager.fetch_error = None;
                        let (tx, rx) = std::sync::mpsc::channel();
                        app.engine_manager.fetch_rx = Some(rx);
                        std::thread::spawn(move || {
                            let result = github::fetch_releases()
                                .map_err(|e| e.to_string());
                            let _ = tx.send(result);
                        });
                    }
                });

                if let Some(ref err) = app.engine_manager.fetch_error.clone() {
                    ui.add_space(6.0);
                    status_chip(ui, &format!("Error: {}", err), pal::ERROR);
                }

                // Download progress
                if let Some((downloaded, total)) = app.engine_manager.download_progress {
                    ui.add_space(8.0);
                    let pct = if total > 0 {
                        downloaded as f32 / total as f32
                    } else {
                        0.0
                    };
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::ProgressBar::new(pct)
                                .text(format!(
                                    "Downloading... {:.1} / {:.1} MB",
                                    downloaded as f64 / 1_048_576.0,
                                    total as f64 / 1_048_576.0
                                ))
                                .desired_width(400.0),
                        );
                    });
                }

                if !app.engine_manager.releases.is_empty() {
                    ui.add_space(10.0);
                    let releases = app.engine_manager.releases.clone();
                    for release in &releases {
                        egui::Frame::none()
                            .fill(pal::SURFACE2)
                            .stroke(egui::Stroke::new(1.0, pal::BORDER))
                            .rounding(egui::Rounding::same(6_f32))
                            .inner_margin(egui::Margin {
                                left: 14.0,
                                right: 14.0,
                                top: 8.0,
                                bottom: 8.0,
                            })
                            .show(ui, |ui| {
                                ui.set_min_width(520.0);
                                ui.horizontal(|ui| {
                                    badge(
                                        ui,
                                        &release.tag_name,
                                        pal::PRIMARY_DIM,
                                        pal::PRIMARY,
                                    );
                                    ui.add_space(8.0);
                                    ui.label(
                                        egui::RichText::new(
                                            release.name.as_deref().unwrap_or(""),
                                        )
                                        .size(11.0)
                                        .color(pal::TEXT_DIM),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let already_installed = app
                                                .config
                                                .engines
                                                .iter()
                                                .any(|e| e.version == release.tag_name);
                                            let is_downloading = app.engine_manager.download_progress.is_some();

                                            if already_installed {
                                                ui.label(
                                                    egui::RichText::new("Installed")
                                                        .size(11.0)
                                                        .color(pal::SUCCESS),
                                                );
                                            } else if release.editor_asset().is_none() {
                                                ui.label(
                                                    egui::RichText::new("No binary for this platform")
                                                        .size(11.0)
                                                        .color(pal::TEXT_MUTED),
                                                );
                                            } else {
                                                ui.add_enabled_ui(!is_downloading, |ui| {
                                                    if ui
                                                        .add(
                                                            egui::Button::new(
                                                                egui::RichText::new("Download")
                                                                    .size(11.0)
                                                                    .color(egui::Color32::WHITE),
                                                            )
                                                            .fill(pal::PRIMARY)
                                                            .stroke(egui::Stroke::NONE),
                                                        )
                                                        .clicked()
                                                        && let Some(asset) =
                                                            release.editor_asset()
                                                        {
                                                            app.engine_manager.download_rx = Some(
                                                                download::start_download(
                                                                    asset,
                                                                    &release.tag_name,
                                                                ),
                                                            );
                                                        }
                                                });
                                            }
                                        },
                                    );
                                });
                            });
                        ui.add_space(4.0);
                    }
                }
            });
    });
}
