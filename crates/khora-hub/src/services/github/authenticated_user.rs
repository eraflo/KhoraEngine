// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! The signed-in GitHub user.

use anyhow::{Context, Result};
use serde::Deserialize;

use super::repos::authed_client;

/// The authenticated GitHub user — subset of `GET /user`.
#[derive(Debug, Clone, Deserialize)]
pub struct AuthenticatedUser {
    pub login: String,
}

/// Fetches the authenticated user via `GET /user`.
pub fn get_authenticated_user(token: &str) -> Result<AuthenticatedUser> {
    let client = authed_client(token)?;
    let response = client
        .get("https://api.github.com/user")
        .send()
        .context("Failed to call GitHub /user")?;

    if !response.status().is_success() {
        anyhow::bail!("GitHub /user returned status {}", response.status());
    }

    response.json().context("Failed to parse GitHub user JSON")
}
