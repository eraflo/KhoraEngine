// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Hub UI infrastructure — theme, fonts, shared widgets.
//!
//! The hub depends on `khora-sdk` and pulls the brand tokens + fonts from
//! `khora_sdk::tool_ui` (the shared `khora-core::ui::brand` palette), so the
//! hub and editor render from one source of truth. `theme` is a thin
//! re-export shim; `widgets` is hub-local until Phase C promotes the shared
//! ones into `khora-core::ui::widgets`.

pub mod fonts;
pub mod theme;
pub mod widgets;
