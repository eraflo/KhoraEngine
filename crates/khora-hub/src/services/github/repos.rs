// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! The repositories created on the signed-in user's behalf.

use anyhow::{Context, Result};
use serde::Deserialize;

pub use super::authenticated_user::get_authenticated_user;

/// A GitHub repository — subset of the `POST /user/repos` response we care about.
#[derive(Debug, Clone, Deserialize)]
pub struct CreatedRepo {
    pub full_name: String,
    pub clone_url: String,
}

/// An HTTP client that sends `token` as a bearer to the GitHub API.
pub(super) fn authed_client(token: &str) -> Result<reqwest::blocking::Client> {
    use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue};

    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}"))
            .context("Token contains invalid header characters")?,
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github+json"),
    );

    reqwest::blocking::Client::builder()
        .user_agent("khora-hub/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .default_headers(headers)
        .build()
        .context("Failed to build authenticated HTTP client")
}

/// Creates a new repository under the authenticated user via `POST /user/repos`.
pub fn create_repo(token: &str, name: &str, private: bool) -> Result<CreatedRepo> {
    let client = authed_client(token)?;
    let body = serde_json::json!({
        "name": name,
        "private": private,
        "auto_init": false,
    });

    let response = client
        .post("https://api.github.com/user/repos")
        .json(&body)
        .send()
        .context("Failed to call GitHub /user/repos")?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().unwrap_or_default();
        anyhow::bail!("GitHub /user/repos returned {}: {}", status, text);
    }

    response.json().context("Failed to parse created-repo JSON")
}
