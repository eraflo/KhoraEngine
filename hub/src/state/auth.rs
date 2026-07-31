// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! GitHub authentication state + Settings screen state.

use crate::auth;
use std::sync::mpsc;

/// GitHub auth state — disconnected → connecting → connected.
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

/// Settings screen state — auth flow + local repo path editing.
#[derive(Default)]
pub struct SettingsState {
    pub auth: AuthState,
    pub auth_rx: Option<mpsc::Receiver<auth::AuthMessage>>,
    pub local_repo_draft: String,
}
