// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Khora Engine Hub — standalone project manager and engine launcher.
//!
//! This crate has **no dependency** on any `khora-*` engine crates.
//! It is a self-contained `eframe` application.

mod config;
pub mod download;
mod fonts;
mod github;
mod project;
mod screens;
pub mod theme;
pub mod widgets;

use config::{EngineInstall, HubConfig, RecentProject};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc;
use theme::pal;
use widgets::*;

// ── Screen enum ───────────────────────────────────────────────────

#[derive(Debug, PartialEq, Clone, Copy, Default)]
pub enum Screen {
    #[default]
    Home,
    NewProject,
    EngineManager,
}

// ── State structs ─────────────────────────────────────────────────

/// A temporary top-of-screen info/error banner.
pub struct Banner {
    pub message: String,
    pub is_error: bool,
}

/// Home screen state (project list hover).
#[derive(Default)]
pub struct HomeState {
    pub hovered: Option<usize>,
}

/// New Project form state.
pub struct NewProjectState {
    pub name: String,
    pub path: String,
    pub engine_idx: usize,
    pub status: Option<String>,
    pub success: bool,
}

impl NewProjectState {
    fn new() -> Self {
        let path = dirs::home_dir()
            .map(|h| h.join("KhoraProjects").to_string_lossy().to_string())
            .unwrap_or_default();
        Self {
            name: String::new(),
            path,
            engine_idx: 0,
            status: None,
            success: false,
        }
    }
}

/// Engine Manager screen state (local build + GitHub releases + downloads).
pub struct EngineManagerState {
    pub local_repo: String,
    pub releases: Vec<github::GithubRelease>,
    pub fetch_error: Option<String>,
    pub fetching: bool,
    /// Active background download receiver. `None` when idle.
    pub download_rx: Option<mpsc::Receiver<download::DownloadMessage>>,
    /// Current download progress `(bytes_done, total_bytes)`. `None` when idle.
    pub download_progress: Option<(u64, u64)>,
    /// Background "Check for Updates" receiver. `None` when idle.
    pub fetch_rx: Option<mpsc::Receiver<Result<Vec<github::GithubRelease>, String>>>,
}

impl EngineManagerState {
    fn new(local_repo: String) -> Self {
        Self {
            local_repo,
            releases: Vec::new(),
            fetch_error: None,
            fetching: false,
            download_rx: None,
            download_progress: None,
            fetch_rx: None,
        }
    }
}

// ── App state ─────────────────────────────────────────────────────

pub struct HubApp {
    pub config: HubConfig,
    pub screen: Screen,
    pub banner: Option<Banner>,
    pub home: HomeState,
    pub new_project: NewProjectState,
    pub engine_manager: EngineManagerState,
}

impl HubApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Install brand fonts (Geist / Geist Mono) if available next to the
        // binary or in the crate's assets/fonts/. No-op when missing.
        cc.egui_ctx.set_fonts(fonts::build_definitions());

        let config = HubConfig::load();
        let local_repo = config.local_engine_repo.clone().unwrap_or_default();

        Self {
            engine_manager: EngineManagerState::new(local_repo),
            new_project: NewProjectState::new(),
            config,
            screen: Screen::Home,
            banner: None,
            home: HomeState::default(),
        }
    }

    /// Returns all available engine options (dev + installed).
    pub fn available_engines(&self) -> Vec<EngineInstall> {
        let mut engines = Vec::new();
        if let Some(dev) = self.config.dev_engine() {
            engines.push(dev);
        }
        engines.extend(self.config.engines.clone());
        engines
    }

    pub fn launch_project(&mut self, proj: &RecentProject) {
        let engines = self.available_engines();
        let engine = engines
            .iter()
            .find(|e| e.version == proj.engine_version)
            .or_else(|| engines.first());

        match engine {
            None => {
                self.banner = Some(Banner {
                    message: format!(
                        "No engine available for version '{}'. Configure one in Engine Manager.",
                        proj.engine_version
                    ),
                    is_error: true,
                });
            }
            Some(engine) => {
                let path = PathBuf::from(&proj.path);
                match project::launch_editor(&engine.editor_binary, &path) {
                    Ok(()) => {
                        self.config
                            .push_recent(&proj.name, &path, &proj.engine_version);
                        let _ = self.config.save();
                        self.banner = Some(Banner {
                            message: format!("Opened '{}' in editor.", proj.name),
                            is_error: false,
                        });
                    }
                    Err(e) => {
                        self.banner = Some(Banner {
                            message: format!("Failed to launch editor: {}", e),
                            is_error: true,
                        });
                    }
                }
            }
        }
    }

    pub fn open_existing_project(&mut self, path: PathBuf) {
        let descriptor_path = path.join("project.json");
        let (name, engine_version) = if descriptor_path.exists() {
            let text = std::fs::read_to_string(&descriptor_path).unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            (
                v["name"].as_str().unwrap_or("Unknown").to_owned(),
                v["engine_version"].as_str().unwrap_or("dev").to_owned(),
            )
        } else {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Project".to_owned());
            (name, "dev".to_owned())
        };

        let proj = RecentProject {
            name: name.clone(),
            path: path.to_string_lossy().to_string(),
            engine_version: engine_version.clone(),
            last_opened: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        self.launch_project(&proj);
        self.config.push_recent(&name, &path, &engine_version);
        let _ = self.config.save();
    }
}

impl eframe::App for HubApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        theme::apply_hub_visuals(ctx);

        // ── Poll async GitHub releases fetch ─────────────────────────
        if let Some(ref rx) = self.engine_manager.fetch_rx
            && let Ok(result) = rx.try_recv()
        {
            match result {
                Ok(releases) => {
                    self.engine_manager.releases = releases;
                    self.engine_manager.fetch_error = None;
                }
                Err(e) => {
                    self.engine_manager.fetch_error = Some(e);
                }
            }
            self.engine_manager.fetching = false;
            self.engine_manager.fetch_rx = None;
        }

        // ── Poll background download ───────────────────────────────
        if let Some(ref rx) = self.engine_manager.download_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    download::DownloadMessage::Progress(done, total) => {
                        self.engine_manager.download_progress = Some((done, total));
                    }
                    download::DownloadMessage::Completed { version, install } => {
                        self.engine_manager.download_progress = None;
                        self.config.engines.push(install);
                        let _ = self.config.save();
                        self.banner = Some(Banner {
                            message: format!("Engine {} downloaded and installed!", version),
                            is_error: false,
                        });
                        log::info!("Engine {} installed", version);
                    }
                    download::DownloadMessage::Error(e) => {
                        self.engine_manager.download_progress = None;
                        self.banner = Some(Banner {
                            message: format!("Download failed: {}", e),
                            is_error: true,
                        });
                        log::error!("Download failed: {}", e);
                    }
                }
            }
            // Clean up receiver once download is done.
            if self.engine_manager.download_progress.is_none()
                && self.engine_manager.download_rx.is_some()
                && rx.try_recv().is_err()
            {
                self.engine_manager.download_rx = None;
            }
            ctx.request_repaint();
        }

        // ── Top bar ────────────────────────────────────────────────
        egui::TopBottomPanel::top("hub_topbar")
            .exact_height(44.0)
            .show(ctx, |ui| {
                let r = ui.max_rect();
                ui.painter().rect_filled(r, 0.0, pal::BG);
                ui.painter().line_segment(
                    [r.left_bottom(), r.right_bottom()],
                    egui::Stroke::new(1.0, pal::BORDER),
                );

                ui.horizontal_centered(|ui| {
                    ui.add_space(16.0);
                    let (star_rect, _) =
                        ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                    paint_khora_star(ui.painter(), star_rect.center(), 8.0, pal::PRIMARY);
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new("KhoraEngine")
                            .strong()
                            .size(14.0)
                            .color(pal::TEXT),
                    );

                    ui.add_space(24.0);

                    let sep_rect = egui::Rect::from_center_size(
                        ui.next_widget_position() + egui::vec2(0.0, 0.0),
                        egui::vec2(1.0, 20.0),
                    );
                    ui.painter().rect_filled(sep_rect, 0.0, pal::BORDER);
                    ui.add_space(16.0);

                    tab_pill(ui, "Projects", self.screen == Screen::Home);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(16.0);
                        ui.label(
                            egui::RichText::new("v0.1.0-dev")
                                .size(11.0)
                                .color(pal::TEXT_MUTED),
                        );
                    });
                });
            });

        // Paint global BG behind all panels
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.painter().rect_filled(ui.max_rect(), 0.0, pal::BG);
        });

        match self.screen {
            Screen::Home => screens::show_home(self, ctx),
            Screen::NewProject => screens::show_new_project(self, ctx),
            Screen::EngineManager => screens::show_engine_manager(self, ctx),
        }
    }
}

// ── Entry point ──────────────────────────────────────────────────

/// Load the Khora logo from the embedded PNG asset.
fn load_logo_icon() -> egui::IconData {
    let png_bytes = include_bytes!("../assets/khora_small_logo.png");
    match image::load_from_memory(png_bytes) {
        Ok(img) => {
            let rgba_img = img.to_rgba8();
            let (w, h) = rgba_img.dimensions();
            egui::IconData {
                rgba: rgba_img.into_raw(),
                width: w,
                height: h,
            }
        }
        Err(e) => {
            log::warn!("Failed to decode logo PNG: {}", e);
            egui::IconData {
                rgba: vec![0, 0, 0, 0],
                width: 1,
                height: 1,
            }
        }
    }
}

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Khora Engine Hub")
            .with_inner_size([1100.0, 680.0])
            .with_min_inner_size([740.0, 440.0])
            .with_icon(std::sync::Arc::new(load_logo_icon())),
        ..Default::default()
    };

    eframe::run_native(
        "Khora Engine Hub",
        options,
        Box::new(|cc| Ok(Box::new(HubApp::new(cc)))),
    )
}
