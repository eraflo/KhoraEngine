// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! New project — the form, and a rail that says what pressing Create will do.

use std::path::PathBuf;

use crate::EngineChoice;
use crate::HubApp;
use crate::Screen;
use crate::download;
use crate::github;
use crate::project;
use khora_sdk::tool_ui::{Icon, UiBuilder, UiTheme};
use khora_tool_ui::brand::khora_dark;
use khora_tool_ui::widgets::{
    self, Button, ButtonKind, StepState, button, checkbox, error_line, field_label, input_frame,
    paint::{display, mono, text},
    progress_row, radio_card, step_rail,
};

const FORM_W: f32 = 520.0;
const RAIL_W: f32 = 260.0;
const ROW: f32 = 32.0;

/// Renders the new-project screen.
pub fn show_new_project(app: &mut HubApp, ui: &mut dyn UiBuilder) {
    if !app.new_project.has_fetched_once && app.new_project.fetch_rx.is_none() {
        app.new_project.fetch_rx = Some(github::fetch_releases_async());
    }

    let t = khora_dark();
    let r = ui.panel_rect();
    let pad = 24.0;

    let total = FORM_W + 40.0 + RAIL_W;
    let x = r[0] + ((r[2] - total) * 0.5).max(pad);
    let form_w = FORM_W.min(r[2] - pad * 2.0);

    let mut y = r[1] + 20.0;

    if button(
        ui,
        &t,
        [x, y, 80.0, 26.0],
        "np-back",
        Button::new("Back", ButtonKind::Ghost).icon(Icon::ArrowLeft),
    )
    .clicked
    {
        app.screen = Screen::Home;
    }
    y += 26.0 + 16.0;

    display(ui, [x, y], "Create a project", 22.0, t.text);
    y += 30.0;
    text(
        ui,
        [x, y],
        "A new Khora project with an engine version pinned and, optionally, a git repository.",
        t.font_size_body,
        t.text_muted,
    );
    y += 28.0;

    // ── Basics ──
    eyebrow(ui, &t, [x, y], "Basics");
    y += 20.0;

    let name_invalid = !app.new_project.name.trim().is_empty()
        && project_dir(app).map(|p| p.exists()).unwrap_or(false);

    field_label(ui, &t, [x, y], "Project name");
    y += 18.0;
    let inner = input_frame(ui, &t, [x, y, form_w, ROW], "np-name", name_invalid);
    ui.region_at("np-name", inner, &mut |ui| {
        ui.text_edit_singleline(&mut app.new_project.name);
    });
    y += ROW + 4.0;
    if name_invalid {
        error_line(
            ui,
            &t,
            [x, y],
            "A folder with that name already exists here",
        );
        y += 18.0;
    }
    y += 8.0;

    let parent = PathBuf::from(app.new_project.path.trim());
    let dir_invalid = !app.new_project.path.trim().is_empty() && !parent.is_dir();

    field_label(ui, &t, [x, y], "Parent directory");
    y += 18.0;
    let field_w = form_w - 108.0;
    let inner = input_frame(ui, &t, [x, y, field_w, ROW], "np-path", dir_invalid);
    ui.region_at("np-path", inner, &mut |ui| {
        ui.text_edit_singleline(&mut app.new_project.path);
    });
    if button(
        ui,
        &t,
        [x + field_w + 8.0, y, 100.0, ROW],
        "np-browse",
        Button::new("Browse…", ButtonKind::Ghost).icon(Icon::FolderOpen),
    )
    .clicked
        && let Some(path) = rfd::FileDialog::new().pick_folder()
    {
        app.new_project.path = path.to_string_lossy().to_string();
    }
    y += ROW + 4.0;
    if dir_invalid {
        error_line(ui, &t, [x, y], "That directory doesn't exist");
        y += 18.0;
    } else if let Some(dest) = project_dir(app) {
        mono(
            ui,
            [x, y],
            &format!("Creates {}", dest.display()),
            t.font_size_caption,
            t.text_disabled,
        );
        y += 16.0;
    }
    y += 12.0;

    // ── Engine ──
    eyebrow(ui, &t, [x, y], "Engine");
    y += 20.0;

    let choices = app.engine_choices();
    for (i, choice) in choices.iter().enumerate() {
        let (label, meta) = match choice {
            EngineChoice::Installed(e) => (e.version.clone(), "installed".to_owned()),
            EngineChoice::Remote { version, size, .. } => (
                version.clone(),
                format!("download · {} MB", size / 1_000_000),
            ),
        };
        if radio_card(
            ui,
            &t,
            [x, y, form_w, 38.0],
            &format!("np-engine-{i}"),
            &label,
            &meta,
            i == app.new_project.engine_idx,
        )
        .clicked
        {
            app.new_project.engine_idx = i;
        }
        y += 38.0 + 6.0;
    }
    y += 8.0;

    // ── Source control ──
    eyebrow(ui, &t, [x, y], "Source control");
    y += 20.0;

    if checkbox(
        ui,
        &t,
        [x, y, form_w, 24.0],
        "np-git",
        "Initialize a git repository",
        app.new_project.git_init,
    )
    .clicked
    {
        app.new_project.git_init = !app.new_project.git_init;
    }
    y += 28.0;

    if app.new_project.git_init {
        // Nesting is shown with a left rule, not indentation alone — it makes
        // the dependency between the options visible.
        let nest_x = x + 26.0;
        let nest_top = y;

        if checkbox(
            ui,
            &t,
            [nest_x, y, form_w - 26.0, 24.0],
            "np-git-remote",
            "Create a GitHub repository",
            app.new_project.git_remote,
        )
        .clicked
        {
            app.new_project.git_remote = !app.new_project.git_remote;
        }
        y += 28.0;

        if app.new_project.git_remote {
            let deep_x = nest_x + 26.0;

            field_label(ui, &t, [deep_x, y], "Repository name");
            y += 18.0;
            let inner = input_frame(ui, &t, [deep_x, y, form_w - 52.0, ROW], "np-repo", false);
            ui.region_at("np-repo", inner, &mut |ui| {
                ui.text_edit_singleline(&mut app.new_project.remote_repo_name);
            });
            y += ROW + 8.0;

            if checkbox(
                ui,
                &t,
                [deep_x, y, form_w - 52.0, 24.0],
                "np-private",
                "Private repository",
                app.new_project.remote_private,
            )
            .clicked
            {
                app.new_project.remote_private = !app.new_project.remote_private;
            }
            y += 26.0;

            if checkbox(
                ui,
                &t,
                [deep_x, y, form_w - 52.0, 24.0],
                "np-push",
                "Push the initial commit",
                app.new_project.remote_push,
            )
            .clicked
            {
                app.new_project.remote_push = !app.new_project.remote_push;
            }
            y += 26.0;

            if !app.settings.auth.is_connected() {
                error_line(
                    ui,
                    &t,
                    [deep_x, y],
                    "Not connected to GitHub — this will fall back to local-only",
                );
                y += 20.0;
            }
        }

        ui.paint_line([x + 12.0, nest_top], [x + 12.0, y - 6.0], t.border, 1.0);
    }
    y += 12.0;

    // ── Progress / status ──
    if let Some((done, total_bytes)) = app.new_project.download_progress {
        let ratio = if total_bytes == 0 {
            0.0
        } else {
            done as f32 / total_bytes as f32
        };
        progress_row(ui, &t, [x, y, form_w, 22.0], "Downloading engine", ratio);
        y += 30.0;
    }
    if let Some(status) = app.new_project.status.as_ref() {
        let color = if app.new_project.success {
            t.success
        } else {
            t.warning
        };
        text(ui, [x, y], status, t.font_size_caption + 1.0, color);
        y += 20.0;
    }

    // ── Actions ──
    y += 6.0;
    ui.paint_line([x, y], [x + form_w, y], t.separator, 1.0);
    y += 16.0;

    let busy = app.new_project.creating_after_download || app.new_project.download_rx.is_some();
    if button(
        ui,
        &t,
        [x, y, 150.0, ROW],
        "np-create",
        Button::new(
            if busy { "Working…" } else { "Create project" },
            ButtonKind::Primary,
        )
        .icon(Icon::Plus)
        .enabled(!busy),
    )
    .clicked
    {
        handle_create(app, &choices);
    }
    if button(
        ui,
        &t,
        [x + 158.0, y, 90.0, ROW],
        "np-cancel",
        Button::new("Cancel", ButtonKind::Ghost),
    )
    .clicked
    {
        app.screen = Screen::Home;
    }

    // ── The rail: what pressing Create actually does ──
    let rail_x = x + form_w + 40.0;
    if widgets::right(r) - rail_x >= RAIL_W {
        show_rail(app, ui, &t, [rail_x, r[1] + 96.0, RAIL_W, 200.0]);
    }
}

fn show_rail(app: &HubApp, ui: &mut dyn UiBuilder, t: &UiTheme, rect: [f32; 4]) {
    text(
        ui,
        [rect[0], rect[1]],
        "What happens",
        t.font_size_body,
        t.text,
    );

    let named = !app.new_project.name.trim().is_empty();
    let git = app.new_project.git_init;

    let engine = app
        .engine_choices()
        .get(app.new_project.engine_idx)
        .map(|c| match c {
            EngineChoice::Installed(e) => e.version.clone(),
            EngineChoice::Remote { version, .. } => version.clone(),
        })
        .unwrap_or_else(|| "—".to_owned());

    let steps: Vec<(&str, String, StepState)> = vec![
        (
            "Scaffold",
            "project files + assets/".to_owned(),
            if named {
                StepState::Done
            } else {
                StepState::Current
            },
        ),
        (
            "Pin engine",
            engine,
            if named {
                StepState::Done
            } else {
                StepState::Pending
            },
        ),
        (
            "git init",
            if git {
                "+ GitHub remote".to_owned()
            } else {
                "skipped".to_owned()
            },
            if git {
                StepState::Current
            } else {
                StepState::Pending
            },
        ),
        ("Open", "in the editor".to_owned(), StepState::Pending),
    ];

    let borrowed: Vec<(&str, &str, StepState)> =
        steps.iter().map(|(a, b, c)| (*a, b.as_str(), *c)).collect();

    step_rail(
        ui,
        t,
        [rect[0], rect[1] + 26.0, rect[2], rect[3]],
        &borrowed,
    );
}

fn eyebrow(ui: &mut dyn UiBuilder, t: &UiTheme, pos: [f32; 2], label: &str) {
    mono(
        ui,
        pos,
        &label.to_uppercase(),
        t.font_size_caption - 1.0,
        t.text_disabled,
    );
}

/// The folder that would be created, if both fields are filled in.
fn project_dir(app: &HubApp) -> Option<PathBuf> {
    let name = app.new_project.name.trim();
    let parent = app.new_project.path.trim();
    if name.is_empty() || parent.is_empty() {
        return None;
    }
    Some(PathBuf::from(parent).join(name.to_lowercase()))
}

fn handle_create(app: &mut HubApp, choices: &[EngineChoice]) {
    if app.new_project.name.trim().is_empty() {
        app.new_project.success = false;
        app.new_project.status = Some("Give the project a name.".into());
        return;
    }
    if app.new_project.path.trim().is_empty() {
        app.new_project.success = false;
        app.new_project.status = Some("Pick a parent directory.".into());
        return;
    }
    let Some(choice) = choices.get(app.new_project.engine_idx).cloned() else {
        app.new_project.success = false;
        app.new_project.status = Some("Pick an engine.".into());
        return;
    };

    match choice {
        EngineChoice::Installed(engine) => create_with_engine(app, engine),
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
            app.new_project.status = Some(format!("Created at {}", root.display()));
            app.config
                .push_recent(&app.new_project.name, &root, &engine.version);
            let _ = app.config.save();
            match project::launch_editor(&engine.editor_binary, &root) {
                Ok(()) => {
                    app.banner = Some(crate::Banner::info("Editor launched."));
                    app.screen = Screen::Home;
                }
                Err(e) => {
                    app.banner = Some(crate::Banner::error(format!(
                        "Project created, but the editor didn't launch: {e}"
                    )));
                }
            }
        }
        Err(e) => {
            app.new_project.success = false;
            app.new_project.status = Some(format!("Error: {e}"));
            app.banner = Some(crate::Banner::error(format!(
                "Couldn't create project: {e}"
            )));
        }
    }
}
