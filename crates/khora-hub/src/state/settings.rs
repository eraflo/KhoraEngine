// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! State of the settings screen.

use super::auth::AuthState;
use crate::auth;
use std::sync::mpsc;

/// Settings screen state — auth flow + local repo path editing.
#[derive(Default)]
pub struct SettingsState {
    pub auth: AuthState,
    pub auth_rx: Option<mpsc::Receiver<auth::AuthMessage>>,
    pub local_repo_draft: String,
}
