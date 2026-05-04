// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Settings screen — GitHub auth + local engine path.

use crate::AuthState;
use crate::HubApp;
use crate::Screen;
use crate::auth;
use crate::theme::pal;
use crate::widgets::*;
use eframe::egui;
use std::path::PathBuf;

/// Section title preceded by a small diamond — matches the editor's
/// panel-header treatment.
fn sub_section(ui: &mut egui::Ui, label: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
        paint_diamond_filled(ui.painter(), rect.center(), 5.0, pal::PRIMARY_DIM);
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(label)
                .strong()
                .size(13.0)
                .color(pal::TEXT_DIM),
        );
    });
}

pub fn show_settings(app: &mut HubApp, ctx: &egui::Context) {
    egui::CentralPanel::default()
        .frame(egui::Frame::none().inner_margin(egui::Margin {
            left: 32.0,
            right: 32.0,
            top: 28.0,
            bottom: 0.0,
        }))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("settings_scroll")
                .show(ui, |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("< Back")
                                    .size(12.0)
                                    .color(pal::TEXT_DIM),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE),
                        )
                        .clicked()
                    {
                        app.screen = Screen::Home;
                    }
                    ui.add_space(12.0);

                    ui.label(
                        egui::RichText::new("Settings")
                            .strong()
                            .size(22.0)
                            .color(pal::TEXT),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("GitHub account, local engine path, and more.")
                            .size(12.0)
                            .color(pal::TEXT_MUTED),
                    );
                    ui.add_space(20.0);

                    sub_section(ui, "GitHub Account");
                    ui.add_space(8.0);
                    show_github_card(app, ui);

                    ui.add_space(20.0);
                    sub_section(ui, "Local Development Build");
                    ui.add_space(8.0);
                    show_local_repo_card(app, ui);
                });
        });
}

fn show_github_card(app: &mut HubApp, ui: &mut egui::Ui) {
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

            // Inline-clone state to avoid borrow conflicts.
            let auth_state_view = match &app.settings.auth {
                AuthState::Disconnected => "disconnected",
                AuthState::Connecting { .. } => "connecting",
                AuthState::Connected { .. } => "connected",
            };

            match auth_state_view {
                "disconnected" => {
                    ui.label(
                        egui::RichText::new(
                            "Connect to enable creating GitHub repositories from new projects.",
                        )
                        .size(12.0)
                        .color(pal::TEXT_DIM),
                    );
                    ui.add_space(10.0);
                    if primary_button(ui, "Connect GitHub", [180.0, 32.0]).clicked() {
                        app.start_github_auth();
                    }
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(
                            "Uses GitHub's OAuth Device Flow with the `repo` scope.",
                        )
                        .size(11.0)
                        .color(pal::TEXT_MUTED),
                    );
                }
                "connecting" => {
                    if let AuthState::Connecting {
                        device_code,
                        message,
                    } = &app.settings.auth
                    {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new());
                            ui.label(egui::RichText::new(message).size(12.0).color(pal::TEXT_DIM));
                        });

                        if let Some(code) = device_code {
                            ui.add_space(10.0);
                            field_label(ui, "Your device code");
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&code.user_code)
                                            .monospace()
                                            .size(20.0)
                                            .color(pal::PRIMARY),
                                    )
                                    .selectable(true),
                                );
                                ui.add_space(8.0);
                                if ghost_button(ui, "Copy", [60.0, 26.0]).clicked() {
                                    ui.ctx().copy_text(code.user_code.clone());
                                }
                                if ghost_button(ui, "Open", [60.0, 26.0]).clicked() {
                                    let _ = open::that(&code.verification_uri);
                                }
                            });
                            ui.add_space(6.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "Verification URL: {}",
                                    code.verification_uri
                                ))
                                .size(11.0)
                                .color(pal::TEXT_MUTED),
                            );
                        }
                    }
                }
                _ => {
                    if let AuthState::Connected { login, .. } = &app.settings.auth {
                        ui.horizontal(|ui| {
                            status_chip(ui, &format!("Connected as @{login}"), pal::SUCCESS);
                        });
                        ui.add_space(10.0);
                        if ghost_button(ui, "Disconnect", [120.0, 30.0]).clicked() {
                            let _ = auth::forget_token();
                            app.settings.auth = AuthState::Disconnected;
                            app.banner = Some(crate::Banner::info("Disconnected from GitHub."));
                        }
                    }
                }
            }
        });
}

fn show_local_repo_card(app: &mut HubApp, ui: &mut egui::Ui) {
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
                    "Point to your local KhoraEngine repository to use the debug build as 'dev'.",
                )
                .size(12.0)
                .color(pal::TEXT_DIM),
            );
            ui.add_space(10.0);
            field_label(ui, "Repository Path");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut app.settings.local_repo_draft)
                        .hint_text("e.g. C:/Dev/KhoraEngine")
                        .desired_width(380.0)
                        .font(egui::TextStyle::Monospace),
                );
                ui.add_space(4.0);
                if ghost_button(ui, "Browse", [80.0, 28.0]).clicked()
                    && let Some(path) = rfd::FileDialog::new().pick_folder()
                {
                    app.settings.local_repo_draft = path.to_string_lossy().to_string();
                }
            });
            ui.add_space(8.0);

            let exe = if cfg!(windows) {
                "khora-editor.exe"
            } else {
                "khora-editor"
            };
            let editor_present = !app.settings.local_repo_draft.is_empty()
                && PathBuf::from(&app.settings.local_repo_draft)
                    .join("target")
                    .join("debug")
                    .join(exe)
                    .exists();

            if editor_present {
                status_chip(ui, "Editor binary found", pal::SUCCESS);
            } else if !app.settings.local_repo_draft.is_empty() {
                status_chip(
                    ui,
                    "Binary not found — run `cargo build -p khora-editor`",
                    pal::WARNING,
                );
            }

            ui.add_space(12.0);
            if primary_button(ui, "Save Path", [120.0, 32.0]).clicked() {
                if app.settings.local_repo_draft.is_empty() {
                    app.config.local_engine_repo = None;
                } else {
                    app.config.local_engine_repo = Some(app.settings.local_repo_draft.clone());
                }
                app.engine_manager.local_repo = app.settings.local_repo_draft.clone();
                let _ = app.config.save();
                app.banner = Some(crate::Banner::info("Local engine path saved."));
            }
        });
}
