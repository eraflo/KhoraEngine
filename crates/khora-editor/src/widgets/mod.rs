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

//! Editor widget library — composite widgets used by panels.
//!
//! Each widget is a free function that takes `&mut dyn UiBuilder` plus
//! whatever parameters it needs (theme, data, callbacks). Widgets paint to
//! absolute screen coordinates via the new [`UiBuilder`] primitives —
//! `paint_*`, `interact_rect`, etc. — so they compose cleanly without
//! relying on egui's auto-layout.

pub mod brand;
pub mod chrome;
pub mod controls;
pub mod enum_variants;
pub mod inspector;
pub mod paint;
pub mod tile;
