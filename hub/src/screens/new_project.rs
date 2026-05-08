// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! New Project screen — create a project + optional Git remote.

use std::path::PathBuf;

use crate::EngineChoice;
use crate::HubApp;
use crate::Screen;
use crate::download;
use crate::github;
use crate::project;
use crate::theme::{pal, tint};
use crate::widgets::{ghost_button, paint_separator, primary_button, rgba};
use khora_sdk::tool_ui::{FontFamilyHint, TextAlign, UiBuilder};

pub fn show_new_project(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    if !app.new_project.has_fetched_once && app.new_project.fetch_rx.is_none() {
        app.new_project.fetch_rx = Some(github::fetch_releases_async());
    }

    ui.spacing(20.0);
    ui.indent("np_root", &mut |ui| {
        ui.horizontal(&mut |row| {
            if ghost_button(row, "np-back", "< Back", [70.0, 26.0]).clicked {
                app.screen = Screen::Home;
            }
        });
        ui.spacing(8.0);
        title_label(ui, "New Project", 20.0);
        ui.colored_label(rgba(pal::TEXT_DIM), "Create a Khora project from a template.");
        ui.spacing(16.0);
        paint_separator(ui, tint(pal::SEPARATOR, 0.55));
        ui.spacing(14.0);

        ui.scroll_area("np_scroll", &mut |ui| {
            ui.colored_label(rgba(pal::TEXT_DIM), "Project name");
            ui.text_edit_singleline(&mut app.new_project.name);
            ui.spacing(8.0);

            ui.colored_label(rgba(pal::TEXT_DIM), "Parent directory");
            ui.horizontal(&mut |row| {
                row.text_edit_singleline(&mut app.new_project.path);
                if ghost_button(row, "np-browse", "Browse…", [110.0, 26.0]).clicked
                    && let Some(path) = rfd::FileDialog::new().pick_folder()
                {
                    app.new_project.path = path.to_string_lossy().to_string();
                }
            });
            ui.spacing(12.0);

            ui.colored_label(rgba(pal::TEXT_DIM), "Engine");
            let choices = app.engine_choices();
            for (i, choice) in choices.iter().enumerate() {
                let label = match choice {
                    EngineChoice::Installed(e) => format!("{} (installed)", e.version),
                    EngineChoice::Remote {
                        version, size, ..
                    } => format!("{} (download {} MB)", version, size / 1_000_000),
                };
                let active = i == app.new_project.engine_idx;
                let salt = format!("np-engine-{}", i);
                let r = ui.panel_rect();
                let w = (r[2] - 16.0).min(560.0);
                let h = 26.0;
                let pos = ui.cursor_pos();
                let int = ui.interact_rect(&salt, [pos[0], pos[1], w, h]);
                let bg = if active {
                    rgba(tint(pal::PRIMARY, 0.20))
                } else if int.hovered {
                    rgba(pal::SURFACE3)
                } else {
                    rgba(pal::SURFACE2)
                };
                ui.paint_rect_filled(pos, [w, h], bg, 4.0);
                ui.paint_text_styled(
                    [pos[0] + 10.0, pos[1] + 7.0],
                    &label,
                    11.5,
                    rgba(pal::TEXT_DIM),
                    FontFamilyHint::Proportional,
                    TextAlign::Left,
                );
                if int.clicked {
                    app.new_project.engine_idx = i;
                }
                ui.spacing(h + 2.0);
            }
            ui.spacing(12.0);

            ui.checkbox(&mut app.new_project.git_init, "Initialize git repository");
            if app.new_project.git_init {
                ui.indent("np-git-opts", &mut |ui| {
                    ui.checkbox(
                        &mut app.new_project.git_remote,
                        "Also create a GitHub repo",
                    );
                    if app.new_project.git_remote {
                        ui.indent("np-remote-opts", &mut |ui| {
                            ui.colored_label(
                                rgba(pal::TEXT_DIM),
                                "Remote name (defaults to project name)",
                            );
                            ui.text_edit_singleline(&mut app.new_project.remote_repo_name);
                            ui.checkbox(&mut app.new_project.remote_private, "Private repo");
                            ui.checkbox(&mut app.new_project.remote_push, "Push initial commit");
                            if !app.settings.auth.is_connected() {
                                ui.colored_label(
                                    rgba(pal::WARNING),
                                    "Not connected to GitHub — will fall back to local-only.",
                                );
                            }
                        });
                    }
                });
            }
            ui.spacing(16.0);

            if let Some((done, total)) = app.new_project.download_progress {
                let pct = if total == 0 {
                    0.0
                } else {
                    (done as f32 / total as f32 * 100.0).clamp(0.0, 100.0)
                };
                ui.colored_label(
                    rgba(pal::TEXT_DIM),
                    &format!("Downloading engine… {:.0}%", pct),
                );
            }
            if let Some(status) = app.new_project.status.as_ref() {
                let color = if app.new_project.success {
                    pal::SUCCESS
                } else {
                    pal::WARNING
                };
                ui.colored_label(rgba(color), status);
            }

            ui.spacing(12.0);
            ui.horizontal(&mut |row| {
                let creating = app.new_project.creating_after_download
                    || app.new_project.download_rx.is_some();
                if !creating
                    && primary_button(row, "np-create", "Create Project", [180.0, 32.0]).clicked
                {
                    handle_create(app, &choices);
                }
                if creating {
                    row.colored_label(rgba(pal::TEXT_DIM), "Working…");
                }
            });
        });
    });
}

fn handle_create(app: &mut HubApp, choices: &[EngineChoice]) {
    if app.new_project.name.trim().is_empty() {
        app.new_project.success = false;
        app.new_project.status = Some("Project name required.".into());
        return;
    }
    if app.new_project.path.trim().is_empty() {
        app.new_project.success = false;
        app.new_project.status = Some("Parent directory required.".into());
        return;
    }
    let Some(choice) = choices.get(app.new_project.engine_idx).cloned() else {
        app.new_project.success = false;
        app.new_project.status = Some("Pick an engine.".into());
        return;
    };

    match choice {
        EngineChoice::Installed(engine) => {
            create_with_engine(app, engine);
        }
        EngineChoice::Remote {
            version,
            download_url,
            size,
            runtime_url,
            runtime_size,
        } => {
            let editor_asset = github::GithubAsset {
                name: format!("khora-engine-{version}"),
                browser_download_url: download_url,
                size,
            };
            let runtime_asset = runtime_url.map(|u| github::GithubAsset {
                name: format!("khora-runtime-{version}"),
                browser_download_url: u,
                size: runtime_size.unwrap_or(0),
            });
            app.new_project.creating_after_download = true;
            app.new_project.download_progress = Some((0, size));
            app.new_project.download_rx = Some(download::start_download(
                &editor_asset,
                runtime_asset.as_ref(),
                &version,
            ));
            app.new_project.status = Some("Downloading engine…".into());
        }
    }
}

fn create_with_engine(app: &mut HubApp, engine: crate::config::EngineInstall) {
    let git = app.build_git_init();
    let parent = PathBuf::from(&app.new_project.path);
    match project::create_project(&app.new_project.name, &parent, &engine.version, &git) {
        Ok(root) => {
            app.new_project.success = true;
            app.new_project.status = Some(format!("Project created at: {}", root.display()));
            app.config
                .push_recent(&app.new_project.name, &root, &engine.version);
            let _ = app.config.save();
            match project::launch_editor(&engine.editor_binary, &root) {
                Ok(()) => {
                    app.banner = Some(crate::Banner::info("Editor launched!"));
                    app.screen = Screen::Home;
                }
                Err(e) => {
                    app.banner = Some(crate::Banner::error(format!(
                        "Project created but editor launch failed: {e}"
                    )));
                }
            }
        }
        Err(e) => {
            app.new_project.success = false;
            app.new_project.status = Some(format!("Error: {e}"));
            app.banner = Some(crate::Banner::error(format!("Project creation failed: {e}")));
        }
    }
}

fn title_label(ui: &mut dyn UiBuilder, text: &str, size: f32) {
    let pos = ui.cursor_pos();
    ui.paint_text_styled(
        pos,
        text,
        size,
        rgba(pal::TEXT),
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
    ui.spacing(size + 8.0);
}
