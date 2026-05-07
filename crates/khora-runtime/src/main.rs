// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Khora Runtime — generic player binary stamped by the editor's
//! Build Game feature with a project's packed assets.
//!
//! This crate is intentionally tiny: all of the boot logic (auto-detect
//! pack vs loose assets, register decoders, load default scene) lives in
//! [`khora_sdk::run_default`]. The same function powers a user's own
//! `src/main.rs` when they opt into a custom-Rust project but haven't
//! customized the entry point yet — there's no behavioural divergence
//! between the pre-built runtime and a fresh native-Rust scaffold.

use anyhow::Result;
use khora_sdk::prelude::*;

#[global_allocator]
static GLOBAL: SaaTrackingAllocator = SaaTrackingAllocator::new(std::alloc::System);

fn main() -> Result<()> {
    use env_logger::{Builder, Env};
    Builder::from_env(Env::default().default_filter_or("info"))
        .filter_module("wgpu_hal::vulkan::instance", log::LevelFilter::Off)
        .init();

    khora_sdk::run_default()
}
