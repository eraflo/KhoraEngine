// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Per-frame async-message drain. Run once at the top of
//! `App::update` so the rest of the frame paints against a coherent
//! state.

use crate::HubApp;
use crate::auth;
use crate::config::EngineInstall;
use crate::download;
use crate::state::{AuthState, Banner};
use khora_sdk::tool_ui::AppContext;

/// Drain every background-channel queued since the last frame.
/// Triggers banner updates, progress recordings, and post-download
/// callbacks for the New Project flow.
pub fn pump_async_messages(app: &mut HubApp, ctx: &mut dyn AppContext) {
    pump_engine_manager_fetch(app);
    pump_engine_manager_download(app, ctx);
    pump_new_project_fetch(app);
    pump_new_project_download(app, ctx);
    pump_auth(app, ctx);
    expire_banner(app);
}

fn pump_engine_manager_fetch(app: &mut HubApp) {
    if let Some(ref rx) = app.engine_manager.fetch_rx
        && let Ok(result) = rx.try_recv()
    {
        match result {
            Ok(releases) => {
                app.engine_manager.releases = releases;
                app.engine_manager.fetch_error = None;
            }
            Err(e) => app.engine_manager.fetch_error = Some(e),
        }
        app.engine_manager.fetching = false;
        app.engine_manager.fetch_rx = None;
    }
}

fn pump_engine_manager_download(app: &mut HubApp, ctx: &mut dyn AppContext) {
    if let Some(ref rx) = app.engine_manager.download_rx {
        while let Ok(msg) = rx.try_recv() {
            match msg {
                download::DownloadMessage::Progress(done, total) => {
                    app.engine_manager.download_progress = Some((done, total));
                }
                download::DownloadMessage::Completed { version, install } => {
                    app.engine_manager.download_progress = None;
                    app.config.engines.push(install);
                    let _ = app.config.save();
                    app.banner = Some(Banner::info(format!(
                        "Engine {version} downloaded and installed!"
                    )));
                }
                download::DownloadMessage::Error(e) => {
                    app.engine_manager.download_progress = None;
                    app.banner = Some(Banner::error(format!("Download failed: {e}")));
                }
            }
        }
        ctx.request_repaint();
    }
}

fn pump_new_project_fetch(app: &mut HubApp) {
    if let Some(ref rx) = app.new_project.fetch_rx
        && let Ok(result) = rx.try_recv()
    {
        match result {
            Ok(releases) => {
                app.new_project.releases = releases;
                app.new_project.has_fetched_once = true;
            }
            Err(e) => {
                log::warn!("New Project release fetch failed: {e}");
                app.new_project.has_fetched_once = true;
            }
        }
        app.new_project.fetch_rx = None;
    }
}

fn pump_new_project_download(app: &mut HubApp, ctx: &mut dyn AppContext) {
    let mut np_completed: Option<(String, EngineInstall)> = None;
    let mut np_failed: Option<String> = None;
    if let Some(ref rx) = app.new_project.download_rx {
        while let Ok(msg) = rx.try_recv() {
            match msg {
                download::DownloadMessage::Progress(done, total) => {
                    app.new_project.download_progress = Some((done, total));
                }
                download::DownloadMessage::Completed { version, install } => {
                    np_completed = Some((version, install));
                }
                download::DownloadMessage::Error(e) => np_failed = Some(e),
            }
        }
        ctx.request_repaint();
    }
    if let Some((version, install)) = np_completed {
        app.new_project.download_progress = None;
        app.new_project.download_rx = None;
        app.config.engines.push(install.clone());
        let _ = app.config.save();
        log::info!("Engine {} installed (post-NewProject)", version);
        if app.new_project.creating_after_download {
            app.finalize_new_project_creation(&install);
        }
    } else if let Some(e) = np_failed {
        app.new_project.download_progress = None;
        app.new_project.download_rx = None;
        app.new_project.creating_after_download = false;
        app.new_project.success = false;
        app.new_project.status = Some(format!("Engine download failed: {e}"));
        app.banner = Some(Banner::error(format!("Engine download failed: {e}")));
    }
}

fn pump_auth(app: &mut HubApp, ctx: &mut dyn AppContext) {
    let mut auth_done: Option<auth::AuthMessage> = None;
    if let Some(ref rx) = app.settings.auth_rx {
        while let Ok(msg) = rx.try_recv() {
            auth_done = Some(msg);
        }
        ctx.request_repaint();
    }
    if let Some(msg) = auth_done {
        match msg {
            auth::AuthMessage::DeviceCodeReady(code) => {
                let uri = code.verification_uri.clone();
                let user_code = code.user_code.clone();
                if let Err(e) = open::that(&uri) {
                    log::warn!("Could not open browser at {uri}: {e}");
                }
                app.settings.auth = AuthState::Connecting {
                    device_code: Some(code),
                    message: format!("Enter code {user_code} on {uri}"),
                };
            }
            auth::AuthMessage::Authenticated { token, login } => {
                app.banner = Some(Banner::info(format!("Connected to GitHub as @{login}")));
                app.settings.auth = AuthState::Connected { token, login };
                app.settings.auth_rx = None;
            }
            auth::AuthMessage::Failed(e) => {
                app.banner = Some(Banner::error(format!("GitHub auth failed: {e}")));
                app.settings.auth = AuthState::Disconnected;
                app.settings.auth_rx = None;
            }
        }
    }
}

fn expire_banner(app: &mut HubApp) {
    if let Some(b) = app.banner.as_ref()
        && b.is_expired()
    {
        app.banner = None;
    }
}
