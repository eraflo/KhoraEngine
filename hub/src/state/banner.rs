// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Top-of-screen info / error banner. Auto-expires.

use std::time::{Duration, Instant};

/// A short, dismissable notification shown above the active screen.
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
