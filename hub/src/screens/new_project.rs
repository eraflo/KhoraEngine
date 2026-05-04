// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! New Project screen — create a new Khora Engine project.
//!
//! Features:
//! - Engine combo merging installed engines (incl. dev) and remote releases.
//! - Optional Git initialization (local + optional GitHub remote).
//! - When a remote engine is selected, the project is created only after the
//!   download completes (orchestrated by `HubApp::finalize_new_project_creation`).

use crate::EngineChoice;
use crate::HubApp;
use crate::Screen;
use crate::download;
use crate::github;
use crate::project;
use crate::theme::pal;
use crate::widgets::*;
use eframe::egui;
use std::path::PathBuf;

pub fn show_new_project(app: &mut HubApp, ctx: &egui::Context) {
    // Kick off a release fetch on first visit so the combo is populated.
    if !app.new_project.has_fetched_once && app.new_project.fetch_rx.is_none() {
        let (tx, rx) = std::sync::mpsc::channel();
        app.new_project.fetch_rx = Some(rx);
        std::thread::spawn(move || {
            let r = github::fetch_releases().map_err(|e| e.to_string());
            let _ = tx.send(r);
        });
    }

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(28.0);
        ui.horizontal(|ui| {
            ui.add_space(32.0);
            ui.vertical(|ui| {
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

                // ── Form card ──────────────────────────────────────
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
                        ui.set_min_width(520.0);
                        ui.set_max_width(640.0);

                        let busy = app.new_project.creating_after_download
                            || app.new_project.download_progress.is_some();

                        ui.add_enabled_ui(!busy, |ui| form_body(app, ui));

                        if let Some((done, total)) = app.new_project.download_progress {
                            ui.add_space(12.0);
                            let pct = if total > 0 {
                                done as f32 / total as f32
                            } else {
                                0.0
                            };
                            ui.add(
                                egui::ProgressBar::new(pct)
                                    .text(format!(
                                        "Downloading engine… {:.1} / {:.1} MB",
                                        done as f64 / 1_048_576.0,
                                        total as f64 / 1_048_576.0
                                    ))
                                    .desired_width(f32::INFINITY),
                            );
                        }

                        if let Some(status) = app.new_project.status.clone() {
                            ui.add_space(12.0);
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
                                    ui.label(egui::RichText::new(&status).size(11.0).color(fg));
                                });
                        }
                    });
            });
        });
    });
}

fn form_body(app: &mut HubApp, ui: &mut egui::Ui) {
    field_label(ui, "Project Name");
    ui.add_space(4.0);
    ui.add(
        egui::TextEdit::singleline(&mut app.new_project.name)
            .hint_text("e.g. MyGame")
            .desired_width(f32::INFINITY),
    );
    ui.add_space(12.0);

    field_label(ui, "Location");
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut app.new_project.path)
                .desired_width(380.0)
                .font(egui::TextStyle::Monospace),
        );
        ui.add_space(4.0);
        if ghost_button(ui, "Browse", [80.0, 28.0]).clicked()
            && let Some(path) = rfd::FileDialog::new().pick_folder()
        {
            app.new_project.path = path.to_string_lossy().to_string();
        }
    });

    let choices = app.engine_choices();
    ui.add_space(12.0);
    field_label(ui, "Engine Version");
    ui.add_space(4.0);
    if choices.is_empty() {
        if app.new_project.fetch_rx.is_some() {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new());
                ui.label(
                    egui::RichText::new("Fetching available engine versions…")
                        .size(11.0)
                        .color(pal::TEXT_MUTED),
                );
            });
        } else {
            warning_inline(
                ui,
                "No engine available. Configure a local repo or check Engine Manager.",
            );
        }
    } else {
        let safe_idx = app.new_project.engine_idx.min(choices.len() - 1);
        let selected = &choices[safe_idx];
        let selected_label = engine_choice_label(selected);
        egui::ComboBox::from_id_salt("np_engine")
            .selected_text(selected_label)
            .width(360.0)
            .show_ui(ui, |ui| {
                for (i, c) in choices.iter().enumerate() {
                    ui.selectable_value(&mut app.new_project.engine_idx, i, engine_choice_label(c));
                }
            });

        if matches!(selected, EngineChoice::Remote { .. }) {
            ui.add_space(4.0);
            status_chip(
                ui,
                "Selected version will be downloaded before the project is created.",
                pal::ACCENT_CYAN,
            );
        }
    }

    if !app.new_project.name.is_empty() && !app.new_project.path.is_empty() {
        ui.add_space(10.0);
        let preview = PathBuf::from(&app.new_project.path)
            .join(&app.new_project.name)
            .to_string_lossy()
            .to_string();
        ui.label(
            egui::RichText::new(format!("→ {preview}"))
                .size(11.0)
                .color(pal::TEXT_MUTED)
                .monospace(),
        );
    }

    ui.add_space(16.0);
    paint_separator(ui, pal::BORDER);
    ui.add_space(12.0);

    // ── Git options ────────────────────────────────────────────────
    section_header(ui, "Version control");
    ui.add_space(6.0);
    ui.checkbox(&mut app.new_project.git_init, "Initialize Git repository");

    let connected = app.settings.auth.is_connected();
    ui.add_enabled_ui(app.new_project.git_init && connected, |ui| {
        ui.checkbox(
            &mut app.new_project.git_remote,
            "Create on GitHub and add as origin",
        );
        if app.new_project.git_remote {
            ui.indent("git_remote_opts", |ui| {
                ui.horizontal(|ui| {
                    field_label(ui, "Repo name");
                    if app.new_project.remote_repo_name.is_empty() {
                        app.new_project.remote_repo_name = app.new_project.name.clone();
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut app.new_project.remote_repo_name)
                            .desired_width(220.0),
                    );
                });
                ui.checkbox(&mut app.new_project.remote_private, "Private repository");
                ui.checkbox(&mut app.new_project.remote_push, "Push initial commit");
            });
        }
    });
    if !connected {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Connect GitHub in Settings to enable remote creation.")
                .size(11.0)
                .color(pal::TEXT_MUTED),
        );
    }

    ui.add_space(16.0);
    paint_separator(ui, pal::BORDER);
    ui.add_space(12.0);

    let can_create =
        !app.new_project.name.is_empty() && !app.new_project.path.is_empty() && !choices.is_empty();

    ui.add_enabled_ui(can_create, |ui| {
        if primary_button(ui, "Create Project", [180.0, 36.0]).clicked() {
            handle_create(app, &choices);
        }
    });
}

fn engine_choice_label(c: &EngineChoice) -> String {
    match c {
        EngineChoice::Installed(e) => format!("[installed] {} ({})", e.version, e.source),
        EngineChoice::Remote { version, size, .. } => {
            let mb = *size as f64 / 1_048_576.0;
            format!("[download]  {version} (remote, {mb:.1} MB)")
        }
    }
}

fn warning_inline(ui: &mut egui::Ui, text: &str) {
    egui::Frame::none()
        .fill(egui::Color32::from_rgb(50, 38, 18))
        .stroke(egui::Stroke::new(1.0, pal::WARNING.gamma_multiply(0.4)))
        .rounding(egui::Rounding::same(5_f32))
        .inner_margin(egui::Margin {
            left: 10.0,
            right: 10.0,
            top: 6.0,
            bottom: 6.0,
        })
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(11.0).color(pal::WARNING));
        });
}

fn handle_create(app: &mut HubApp, choices: &[EngineChoice]) {
    let idx = app
        .new_project
        .engine_idx
        .min(choices.len().saturating_sub(1));
    let choice = match choices.get(idx) {
        Some(c) => c.clone(),
        None => {
            app.new_project.success = false;
            app.new_project.status = Some("No engine selected.".to_owned());
            return;
        }
    };

    match choice {
        EngineChoice::Installed(engine) => {
            let git = app.build_git_init();
            match project::create_project(
                &app.new_project.name,
                &PathBuf::from(&app.new_project.path),
                &engine.version,
                &git,
            ) {
                Ok(root) => {
                    app.new_project.success = true;
                    app.new_project.status =
                        Some(format!("Project created at: {}", root.display()));
                    app.config
                        .push_recent(&app.new_project.name, &root, &engine.version);
                    let _ = app.config.save();

                    match project::launch_editor(&engine.editor_binary, &root) {
                        Ok(()) => {
                            app.banner = Some(crate::Banner::info("Editor launched!"));
                        }
                        Err(e) => {
                            app.banner = Some(crate::Banner::error(format!(
                                "Project created but could not launch editor: {e}"
                            )));
                        }
                    }
                    app.screen = Screen::Home;
                }
                Err(e) => {
                    app.new_project.success = false;
                    app.new_project.status = Some(format!("Error: {e}"));
                }
            }
        }
        EngineChoice::Remote {
            version,
            download_url,
            size,
        } => {
            // Trigger an engine download; the project will be created in
            // `HubApp::finalize_new_project_creation` once the engine is ready.
            let asset = github::GithubAsset {
                name: format!("khora-engine-{version}"),
                browser_download_url: download_url,
                size,
            };
            app.new_project.creating_after_download = true;
            app.new_project.download_progress = Some((0, size));
            app.new_project.download_rx = Some(download::start_download(&asset, &version));
            app.new_project.status = Some(format!("Downloading engine {version}…"));
            app.new_project.success = true;
        }
    }
}
