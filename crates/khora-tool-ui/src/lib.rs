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

//! # Khora tool UI — the first-party design system
//!
//! The look and feel of **Khora's own tools** (the editor, the hub, and any
//! future asset cooker or profiler). This crate is *cosmetics*, deliberately
//! kept out of the engine:
//!
//! | Crate | Owns | Why |
//! |---|---|---|
//! | `khora-core` | the **contracts** — [`UiTheme`](khora_core::ui::UiTheme) (semantic slots, neutral `Default`), [`UiBuilder`](khora_core::ui::UiBuilder), `FontPack`, `Icon` | the engine's UI backend consumes these; they carry no brand |
//! | **`khora-tool-ui`** (this crate) | the **values + widgets** — the "Deep Navy / Silver" palette and the shared widget vocabulary | Khora's tool identity, which a *game* built on Khora must not inherit |
//!
//! `khora-sdk` (the game-dev façade) does **not** depend on this crate, so a
//! game never compiles Khora's brand. Tools depend on `khora-sdk` for the
//! runtime and on `khora-tool-ui` for the design system.
//!
//! ## Extending it
//!
//! A new tool gets the whole look for free: install [`brand::khora_dark`] as
//! its theme, load the same fonts, and build its screens out of [`widgets`].
//! Widgets are free functions over `&mut dyn UiBuilder` + `&UiTheme` — they
//! never touch a backend and never reach into [`brand::pal`] directly, so
//! re-theming is a matter of passing a different [`UiTheme`].
//!
//! [`UiTheme`]: khora_core::ui::UiTheme

#![deny(missing_docs)]

pub mod brand;
pub mod testing;
pub mod widgets;

pub use brand::{khora_dark, pal};
