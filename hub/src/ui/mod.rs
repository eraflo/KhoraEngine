// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Hub UI infrastructure.
//!
//! The look — brand palette, widget vocabulary — comes from the
//! `khora-tool-ui` crate, shared with the editor so the two tools cannot
//! drift. Nothing visual is defined here.
//!
//! What remains is hub-specific: loading the font files, and the handful of
//! helpers that format the hub's own data.

pub mod fonts;
pub mod widgets;
