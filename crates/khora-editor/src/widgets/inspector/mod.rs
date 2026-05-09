// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Inspector widget library — header, cards, JSON property walker, tabs.
//!
//! Layout:
//!   - [`header`]        — panel-top entity strip (icon + name + tags)
//!   - [`card`]          — collapsible component card frame
//!   - [`walker`]        — JSON `Value` → widget walker (single source)
//!   - [`renderers`]     — per-shape leaf renderers (Vec3, Quat, Color, …)
//!   - [`add_component`] — "+ Add Component" menu, bucketed by domain
//!   - [`display`]       — icon / type-tag / category labels
//!   - [`tabs`]          — `InspectorTab` trait + `Properties` / `Debug`

pub mod add_component;
pub mod asset_pane;
pub mod card;
pub mod display;
pub mod header;
pub mod renderers;
pub mod tabs;
pub mod tag_chips;
pub mod walker;

