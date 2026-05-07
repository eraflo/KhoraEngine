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

pub mod auth;
mod config;
pub mod download;
mod fonts;
pub mod git;
pub mod github;
mod project;
mod screens;
pub mod theme;
pub mod widgets;

use config::{EngineInstall, HubConfig, RecentProject};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use theme::pal;
use widgets::*;

// ── Screen enum ───────────────────────────────────────────────────

#[derive(Debug, PartialEq, Clone, Copy, Default)]
pub enum Screen {
    #[default]
    Home,
    NewProject,
    EngineManager,
    Settings,
}

// ── State structs ─────────────────────────────────────────────────

/// A temporary top-of-screen info/error banner. Expires automatically after
/// [`Banner::DEFAULT_TTL`].
pub struct Banner {
    pub message: String,
    pub is_error: bool,
    pub expires_at: Instant,
}

impl Banner {
    const DEFAULT_TTL: Duration = Duration::from_secs(5);

    pub fn info(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            is_error: false,
            expires_at: Instant::now() + Self::DEFAULT_TTL,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            is_error: true,
            expires_at: Instant::now() + Self::DEFAULT_TTL * 2,
        }
    }

    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }
}

/// Home screen state (project list hover, search, pending deletions).
#[derive(Default)]
pub struct HomeState {
    pub hovered: Option<usize>,
    /// Free-text filter applied to project name + path.
    pub filter: String,
    /// Project pending a deletion confirmation modal — index into the
    /// (filtered) list, resolved against the source list when the user
    /// confirms.
    pub remove_confirm: Option<usize>,
}

/// One row of the New Project engine combo: either an already-installed engine
/// or a remote release that would be downloaded on demand.
#[derive(Debug, Clone)]
pub enum EngineChoice {
    Installed(EngineInstall),
    Remote {
        version: String,
        download_url: String,
        size: u64,
        /// Matching `khora-runtime-<host>` archive URL when the release ships
        /// one (introduced alongside the editor's "Build Game" feature).
        /// Older releases predate the runtime artifact; the engine downloads
        /// without it and Build Game falls back to a clear error until the
        /// user upgrades to a release that includes the runtime.
        runtime_url: Option<String>,
        runtime_size: Option<u64>,
    },
}

impl EngineChoice {
    pub fn version(&self) -> &str {
        match self {
            Self::Installed(e) => &e.version,
            Self::Remote { version, .. } => version,
        }
    }
}

/// New Project form state.
pub struct NewProjectState {
    pub name: String,
    pub path: String,
    pub engine_idx: usize,
    pub status: Option<String>,
    pub success: bool,

    /// Initialize a local Git repo on creation.
    pub git_init: bool,
    /// Also create a GitHub repo and push (only enabled when authenticated).
    pub git_remote: bool,
    pub remote_repo_name: String,
    pub remote_private: bool,
    pub remote_push: bool,

    /// Cached releases for the engine combo. Populated by background fetch.
    pub releases: Vec<github::GithubRelease>,
    /// Background fetch receiver — `None` once a fetch has completed.
    pub fetch_rx: Option<mpsc::Receiver<Result<Vec<github::GithubRelease>, String>>>,
    pub has_fetched_once: bool,

    /// In-flight download (when a remote engine is being installed).
    pub download_rx: Option<mpsc::Receiver<download::DownloadMessage>>,
    pub download_progress: Option<(u64, u64)>,
    /// True while the create-button-triggered download is running. While true,
    /// the form is disabled and the project will be created automatically once
    /// the engine is installed.
    pub creating_after_download: bool,
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

            git_init: true,
            git_remote: false,
            remote_repo_name: String::new(),
            remote_private: true,
            remote_push: true,

            releases: Vec::new(),
            fetch_rx: None,
            has_fetched_once: false,

            download_rx: None,
            download_progress: None,
            creating_after_download: false,
        }
    }
}

/// GitHub authentication state.
#[derive(Debug, Default)]
pub enum AuthState {
    #[default]
    Disconnected,
    /// Device flow in progress: waiting for the user to authorize.
    Connecting {
        device_code: Option<auth::DeviceCode>,
        message: String,
    },
    Connected {
        token: String,
        login: String,
    },
}

impl AuthState {
    pub fn token(&self) -> Option<&str> {
        if let Self::Connected { token, .. } = self {
            Some(token.as_str())
        } else {
            None
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }
}

/// Settings screen state (auth flow + local repo path editing).
#[derive(Default)]
pub struct SettingsState {
    pub auth: AuthState,
    pub auth_rx: Option<mpsc::Receiver<auth::AuthMessage>>,
    pub local_repo_draft: String,
}

/// Engine Manager screen state (local build + GitHub releases + downloads).
pub struct EngineManagerState {
    pub local_repo: String,
    pub releases: Vec<github::GithubRelease>,
    pub fetch_error: Option<String>,
    pub fetching: bool,
    /// True once the screen has triggered at least one fetch. Drives the
    /// auto-fetch on first visit.
    pub has_fetched_once: bool,
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
            has_fetched_once: false,
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
    pub settings: SettingsState,
}

impl HubApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Install brand fonts (Geist / Geist Mono) if available next to the
        // binary or in the crate's assets/fonts/. No-op when missing.
        cc.egui_ctx.set_fonts(fonts::build_definitions());

        // Crisp text: snap `pixels_per_point` to a half-pixel multiple so
        // glyph edges land on device-pixel boundaries. Without this, fractional
        // DPI scales (e.g. 1.25 on Windows) render Geist soft / "smudged".
        let raw = cc
            .egui_ctx
            .input(|i| i.viewport().native_pixels_per_point)
            .unwrap_or(1.0);
        let snapped = (raw * 2.0).round() / 2.0;
        cc.egui_ctx.set_pixels_per_point(snapped.max(1.0));

        let config = HubConfig::load();
        let local_repo = config.local_engine_repo.clone().unwrap_or_default();

        // Restore previous GitHub session if a token is on disk.
        let auth = match auth::load_token() {
            Some(token) => match github::get_authenticated_user(&token) {
                Ok(user) => AuthState::Connected {
                    token,
                    login: user.login,
                },
                Err(e) => {
                    log::warn!("Stored GitHub token is invalid, forgetting: {e}");
                    let _ = auth::forget_token();
                    AuthState::Disconnected
                }
            },
            None => AuthState::Disconnected,
        };

        let settings = SettingsState {
            auth,
            auth_rx: None,
            local_repo_draft: local_repo.clone(),
        };

        Self {
            engine_manager: EngineManagerState::new(local_repo),
            new_project: NewProjectState::new(),
            config,
            screen: Screen::Home,
            banner: None,
            home: HomeState::default(),
            settings,
        }
    }

    /// Builds the engine choices for the New Project combo: installed engines
    /// (incl. dev) followed by remote releases not yet installed and that have
    /// a binary for the current platform.
    pub fn engine_choices(&self) -> Vec<EngineChoice> {
        let mut out: Vec<EngineChoice> = self
            .available_engines()
            .into_iter()
            .map(EngineChoice::Installed)
            .collect();

        let installed: std::collections::HashSet<String> =
            out.iter().map(|c| c.version().to_owned()).collect();

        for r in &self.new_project.releases {
            if installed.contains(&r.tag_name) {
                continue;
            }
            if let Some(asset) = r.editor_asset() {
                let runtime = r.runtime_asset();
                out.push(EngineChoice::Remote {
                    version: r.tag_name.clone(),
                    download_url: asset.browser_download_url.clone(),
                    size: asset.size,
                    runtime_url: runtime.map(|a| a.browser_download_url.clone()),
                    runtime_size: runtime.map(|a| a.size),
                });
            }
        }
        out
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
                self.banner = Some(Banner::error(format!(
                    "No engine available for version '{}'. Configure one in Engine Manager.",
                    proj.engine_version
                )));
            }
            Some(engine) => {
                let path = PathBuf::from(&proj.path);
                match project::launch_editor(&engine.editor_binary, &path) {
                    Ok(()) => {
                        self.config
                            .push_recent(&proj.name, &path, &proj.engine_version);
                        let _ = self.config.save();
                        self.banner =
                            Some(Banner::info(format!("Opened '{}' in editor.", proj.name)));
                    }
                    Err(e) => {
                        self.banner = Some(Banner::error(format!("Failed to launch editor: {e}")));
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        theme::apply_hub_visuals(ctx);

        // ── Poll Engine Manager async fetch ───────────────────────────
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

        // ── Poll Engine Manager background download ───────────────────
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
                        self.banner = Some(Banner::info(format!(
                            "Engine {version} downloaded and installed!"
                        )));
                        log::info!("Engine {} installed", version);
                    }
                    download::DownloadMessage::Error(e) => {
                        self.engine_manager.download_progress = None;
                        self.banner = Some(Banner::error(format!("Download failed: {e}")));
                        log::error!("Download failed: {}", e);
                    }
                }
            }
            if self.engine_manager.download_progress.is_none()
                && self.engine_manager.download_rx.is_some()
                && rx.try_recv().is_err()
            {
                self.engine_manager.download_rx = None;
            }
            ctx.request_repaint();
        }

        // ── Poll New Project releases fetch ───────────────────────────
        if let Some(ref rx) = self.new_project.fetch_rx
            && let Ok(result) = rx.try_recv()
        {
            match result {
                Ok(releases) => {
                    self.new_project.releases = releases;
                    self.new_project.has_fetched_once = true;
                }
                Err(e) => {
                    log::warn!("New Project release fetch failed: {e}");
                    self.new_project.has_fetched_once = true;
                }
            }
            self.new_project.fetch_rx = None;
        }

        // ── Poll New Project background download (post-Create) ────────
        let mut np_completed: Option<(String, EngineInstall)> = None;
        let mut np_failed: Option<String> = None;
        if let Some(ref rx) = self.new_project.download_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    download::DownloadMessage::Progress(done, total) => {
                        self.new_project.download_progress = Some((done, total));
                    }
                    download::DownloadMessage::Completed { version, install } => {
                        np_completed = Some((version, install));
                    }
                    download::DownloadMessage::Error(e) => {
                        np_failed = Some(e);
                    }
                }
            }
            ctx.request_repaint();
        }
        if let Some((version, install)) = np_completed {
            self.new_project.download_progress = None;
            self.new_project.download_rx = None;
            self.config.engines.push(install.clone());
            let _ = self.config.save();
            log::info!("Engine {} installed (post-NewProject)", version);

            // Now that the engine is installed, finalize project creation.
            if self.new_project.creating_after_download {
                self.finalize_new_project_creation(&install);
            }
        } else if let Some(e) = np_failed {
            self.new_project.download_progress = None;
            self.new_project.download_rx = None;
            self.new_project.creating_after_download = false;
            self.new_project.success = false;
            self.new_project.status = Some(format!("Engine download failed: {e}"));
            self.banner = Some(Banner::error(format!("Engine download failed: {e}")));
        }

        // ── Poll auth (device flow) ───────────────────────────────────
        let mut auth_done: Option<auth::AuthMessage> = None;
        if let Some(ref rx) = self.settings.auth_rx {
            while let Ok(msg) = rx.try_recv() {
                auth_done = Some(msg);
            }
            ctx.request_repaint();
        }
        if let Some(msg) = auth_done {
            match msg {
                auth::AuthMessage::DeviceCodeReady(code) => {
                    // Open the browser for the user, copy the user_code if possible.
                    let uri = code.verification_uri.clone();
                    let user_code = code.user_code.clone();
                    if let Err(e) = open::that(&uri) {
                        log::warn!("Could not open browser at {uri}: {e}");
                    }
                    self.settings.auth = AuthState::Connecting {
                        device_code: Some(code),
                        message: format!("Enter code {user_code} on {uri}"),
                    };
                }
                auth::AuthMessage::Authenticated { token, login } => {
                    self.banner = Some(Banner::info(format!("Connected to GitHub as @{login}")));
                    self.settings.auth = AuthState::Connected { token, login };
                    self.settings.auth_rx = None;
                }
                auth::AuthMessage::Failed(e) => {
                    self.banner = Some(Banner::error(format!("GitHub auth failed: {e}")));
                    self.settings.auth = AuthState::Disconnected;
                    self.settings.auth_rx = None;
                }
            }
        }

        // ── Banner auto-dismiss ───────────────────────────────────────
        if let Some(b) = self.banner.as_ref()
            && b.is_expired()
        {
            self.banner = None;
        } else if self.banner.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        // Paint global BG behind all panels (must come before the panels reserve
        // their own space, otherwise a CentralPanel here would swallow the room
        // the screens need to draw into).
        ui.painter().rect_filled(ui.max_rect(), 0.0, pal::BG);

        // ── Top bar (44 px, brand pill + slim tabs) ───────────────────
        self.show_topbar(ui);

        // ── Bottom status bar (24 px, mono metrics, mirrors editor) ───
        self.show_status_bar(ui);

        match self.screen {
            Screen::Home => screens::show_home(self, ui),
            Screen::NewProject => screens::show_new_project(self, ui),
            Screen::EngineManager => screens::show_engine_manager(self, ui),
            Screen::Settings => screens::show_settings(self, ui),
        }
    }
}

impl HubApp {
    /// Returns a short label describing the current screen, used as the
    /// project breadcrumb in the brand pill.
    fn screen_label(&self) -> &'static str {
        match self.screen {
            Screen::Home => "Projects",
            Screen::NewProject => "New Project",
            Screen::EngineManager => "Engine Manager",
            Screen::Settings => "Settings",
        }
    }

    fn show_topbar(&mut self, parent_ui: &mut egui::Ui) {
        egui::Panel::top("hub_topbar")
            .exact_size(44.0)
            .frame(egui::Frame::new())
            .show_inside(parent_ui, |ui| {
                let r = ui.max_rect();
                // Subtle vertical gradient mirrors the editor's panel headers.
                paint_vertical_gradient(ui.painter(), r, pal::SURFACE, pal::BG, 6);
                ui.painter().line_segment(
                    [r.left_bottom(), r.right_bottom()],
                    egui::Stroke::new(1.0, tint(pal::BORDER, 0.55)),
                );

                ui.horizontal_centered(|ui| {
                    ui.add_space(14.0);
                    self.paint_brand_pill(ui);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(14.0);
                        // Right cluster: auth state. Navigation lives in the sidebar.
                        match &self.settings.auth {
                            AuthState::Connected { login, .. } => {
                                status_chip(ui, &format!("@{login}"), pal::SUCCESS);
                            }
                            AuthState::Connecting { .. } => {
                                status_chip(ui, "Connecting…", pal::WARNING);
                            }
                            AuthState::Disconnected => {
                                if ghost_button(ui, "Connect GitHub", [140.0, 26.0]).clicked() {
                                    self.start_github_auth();
                                    self.screen = Screen::Settings;
                                }
                            }
                        }
                    });
                });
            });
    }

    /// Kick off the GitHub OAuth Device Flow if not already in progress. Used
    /// both by the topbar Connect button and the Settings screen.
    pub fn start_github_auth(&mut self) {
        if matches!(self.settings.auth, AuthState::Connecting { .. })
            || self.settings.auth_rx.is_some()
        {
            return;
        }
        self.settings.auth = AuthState::Connecting {
            device_code: None,
            message: "Requesting device code…".to_owned(),
        };
        self.settings.auth_rx = Some(auth::start_device_flow());
    }

    fn paint_brand_pill(&self, ui: &mut egui::Ui) {
        // Reserve the pill area inline.
        let total_w = 220.0;
        let height = 26.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(total_w, height), egui::Sense::hover());

        // Pill background.
        let pill_radius = egui::CornerRadius::same((height * 0.5) as u8);
        ui.painter()
            .rect_filled(rect, pill_radius, tint(pal::BG, 0.55));
        ui.painter().rect_stroke(
            rect,
            pill_radius,
            egui::Stroke::new(1.0, tint(pal::BORDER, 0.6)),
            egui::StrokeKind::Inside,
        );

        // Diamond + engine name on the left.
        let cy = rect.center().y;
        paint_diamond_filled(
            ui.painter(),
            egui::pos2(rect.left() + 14.0, cy),
            6.5,
            pal::PRIMARY,
        );
        ui.painter().text(
            egui::pos2(rect.left() + 26.0, cy),
            egui::Align2::LEFT_CENTER,
            "Khora",
            egui::FontId::proportional(13.0),
            pal::TEXT,
        );

        // Vertical hairline separator.
        let sep_x = rect.left() + 80.0;
        paint_v_hairline(
            ui.painter(),
            sep_x,
            rect.top() + 5.0,
            rect.bottom() - 5.0,
            tint(pal::BORDER, 0.7),
        );

        // Breadcrumb on the right.
        ui.painter().text(
            egui::pos2(sep_x + 8.0, cy),
            egui::Align2::LEFT_CENTER,
            self.screen_label(),
            egui::FontId::proportional(12.0),
            pal::TEXT_MUTED,
        );
    }

    fn show_status_bar(&self, parent_ui: &mut egui::Ui) {
        egui::Panel::bottom("hub_status_bar")
            .exact_size(24.0)
            .frame(egui::Frame::new())
            .show_inside(parent_ui, |ui| {
                let r = ui.max_rect();
                ui.painter().rect_filled(r, 0.0, pal::SURFACE);
                ui.painter().line_segment(
                    [r.left_top(), r.right_top()],
                    egui::Stroke::new(1.0, tint(pal::SEPARATOR, 0.55)),
                );

                let cy = r.center().y;
                let text_y = cy - 5.5;

                // ── Left cluster ─────────────────────────────────────
                let mut x = r.left() + 14.0;
                paint_diamond_filled(ui.painter(), egui::pos2(x, cy), 4.0, pal::PRIMARY);
                x += 14.0;

                // Ready dot + label.
                ui.painter()
                    .circle_filled(egui::pos2(x, cy), 3.5, pal::SUCCESS);
                ui.painter()
                    .circle_filled(egui::pos2(x, cy), 5.5, tint(pal::SUCCESS, 0.18));
                x += 12.0;
                ui.painter().text(
                    egui::pos2(x, text_y),
                    egui::Align2::LEFT_TOP,
                    "Ready",
                    egui::FontId::proportional(11.0),
                    pal::TEXT,
                );
                x += 50.0;

                // Project count.
                paint_v_hairline(
                    ui.painter(),
                    x,
                    r.top() + 6.0,
                    r.bottom() - 6.0,
                    tint(pal::SEPARATOR, 0.55),
                );
                x += 12.0;
                ui.painter().text(
                    egui::pos2(x, text_y),
                    egui::Align2::LEFT_TOP,
                    format!("{} projects", self.config.recent_projects.len()),
                    egui::FontId::monospace(11.0),
                    pal::TEXT_DIM,
                );
                x += 90.0;

                // Engine count (incl. dev).
                paint_v_hairline(
                    ui.painter(),
                    x,
                    r.top() + 6.0,
                    r.bottom() - 6.0,
                    tint(pal::SEPARATOR, 0.55),
                );
                x += 12.0;
                let total_engines =
                    self.config.engines.len() + usize::from(self.config.dev_engine().is_some());
                ui.painter().text(
                    egui::pos2(x, text_y),
                    egui::Align2::LEFT_TOP,
                    format!("{total_engines} engines"),
                    egui::FontId::monospace(11.0),
                    pal::TEXT_DIM,
                );
                let _ = x;

                // ── Right cluster ────────────────────────────────────
                let mut rx = r.right() - 14.0;

                // Hub version, mono.
                let version_label = format!("Hub v{}", env!("CARGO_PKG_VERSION"));
                ui.painter().text(
                    egui::pos2(rx, text_y),
                    egui::Align2::RIGHT_TOP,
                    &version_label,
                    egui::FontId::monospace(11.0),
                    pal::TEXT_MUTED,
                );
                rx -= measure_text_width(ui.ctx(), &version_label, 11.0, true) + 14.0;

                // GitHub status.
                paint_v_hairline(
                    ui.painter(),
                    rx,
                    r.top() + 6.0,
                    r.bottom() - 6.0,
                    tint(pal::SEPARATOR, 0.55),
                );
                rx -= 12.0;
                let (gh_label, gh_color) = match &self.settings.auth {
                    AuthState::Connected { login, .. } => {
                        (format!("GitHub @{login}"), pal::SUCCESS)
                    }
                    AuthState::Connecting { .. } => ("GitHub: connecting".to_owned(), pal::WARNING),
                    AuthState::Disconnected => ("GitHub: offline".to_owned(), pal::TEXT_MUTED),
                };
                ui.painter().text(
                    egui::pos2(rx, text_y),
                    egui::Align2::RIGHT_TOP,
                    &gh_label,
                    egui::FontId::monospace(11.0),
                    gh_color,
                );
            });
    }

    /// Called once a remote engine has finished downloading during a
    /// "Create Project" flow. Writes the project on disk, registers it in the
    /// config, and launches the editor.
    fn finalize_new_project_creation(&mut self, engine: &EngineInstall) {
        self.new_project.creating_after_download = false;

        let version = engine.version.clone();
        let git = self.build_git_init();

        let result = project::create_project(
            &self.new_project.name,
            &PathBuf::from(&self.new_project.path),
            &version,
            &git,
        );

        match result {
            Ok(root) => {
                self.new_project.success = true;
                self.new_project.status = Some(format!("Project created at: {}", root.display()));
                self.config
                    .push_recent(&self.new_project.name, &root, &version);
                let _ = self.config.save();

                match project::launch_editor(&engine.editor_binary, &root) {
                    Ok(()) => {
                        self.banner = Some(Banner::info("Editor launched!"));
                    }
                    Err(e) => {
                        self.banner = Some(Banner::error(format!(
                            "Project created but editor launch failed: {e}"
                        )));
                    }
                }
                self.screen = Screen::Home;
            }
            Err(e) => {
                self.new_project.success = false;
                self.new_project.status = Some(format!("Error: {e}"));
                self.banner = Some(Banner::error(format!("Project creation failed: {e}")));
            }
        }
    }

    /// Resolves the user's Git options into a [`project::GitInit`]. If the
    /// remote-creation step fails, downgrades to local-only and surfaces a
    /// banner.
    pub fn build_git_init(&mut self) -> project::GitInit {
        if !self.new_project.git_init {
            return project::GitInit::None;
        }

        if !self.new_project.git_remote {
            return project::GitInit::Local;
        }

        let Some(token) = self.settings.auth.token().map(str::to_owned) else {
            self.banner = Some(Banner::error(
                "Not connected to GitHub — falling back to local Git only.",
            ));
            return project::GitInit::Local;
        };

        let repo_name = if self.new_project.remote_repo_name.trim().is_empty() {
            self.new_project.name.clone()
        } else {
            self.new_project.remote_repo_name.clone()
        };

        match github::create_repo(&token, &repo_name, self.new_project.remote_private) {
            Ok(repo) => {
                self.banner = Some(Banner::info(format!(
                    "Created GitHub repo {}",
                    repo.full_name
                )));
                project::GitInit::LocalAndRemote {
                    remote_url: repo.clone_url,
                    push: self.new_project.remote_push,
                }
            }
            Err(e) => {
                self.banner = Some(Banner::error(format!(
                    "Could not create remote repo: {e} — falling back to local Git only."
                )));
                project::GitInit::Local
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────

/// Roughly measure the width of a string at the given font size. Uses the
/// glyph cache via `Context::fonts()` and is exact for monospace; for
/// proportional fonts it reflects the kerned width as already laid out by
/// egui.
fn measure_text_width(ctx: &egui::Context, text: &str, size: f32, monospace: bool) -> f32 {
    let font_id = if monospace {
        egui::FontId::monospace(size)
    } else {
        egui::FontId::proportional(size)
    };
    ctx.fonts_mut(|fonts| {
        let galley = fonts.layout_no_wrap(text.to_owned(), font_id, pal::TEXT);
        galley.size().x
    })
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
