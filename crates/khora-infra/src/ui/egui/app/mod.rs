// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! egui implementation of [`khora_core::ui::AppContext`] + the
//! `run_native` boot helper.
//!
//! Standalone tools (the hub) consume this through `khora-sdk` —
//! they never import `eframe` or `egui` directly.

mod context;
mod runtime;

pub use context::EguiAppContext;
pub use runtime::{run_native, WindowConfigInput, WindowIconInput};
