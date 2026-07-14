// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Hub palette — re-exports the canonical Khora brand tokens.
//!
//! The palette, the `khora_dark()` producer, and the `pal` / `tint` / `rgba`
//! helpers live in the `khora-tool-ui` crate — Khora's first-party tool design
//! system, shared with the editor so the two tools cannot drift. It sits
//! outside the engine on purpose: `khora-sdk` doesn't depend on it, so a game
//! built on Khora never inherits the tool brand.
//!
//! This module is a thin alias kept so existing `crate::theme::{pal, tint,
//! khora_hub_dark, …}` call sites keep resolving. Phase C rewrites the hub
//! widgets onto the shared widget vocabulary and this shim goes away with them.

pub use khora_tool_ui::brand::{khora_dark as khora_hub_dark, pal, rgba, tint, with_alpha};
