// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! The inspector's widgets: the entity header, component cards drawn from
//! JSON or from a script's declaration, the tabs and the asset pane.

pub mod add_component;
pub mod asset_pane;
pub mod card;
mod debug_tab;
pub mod declared;
pub mod display;
pub mod header;
mod instance_band;
pub mod renderers;
pub mod tabs;
pub mod tag_chips;
pub mod walker;

#[cfg(test)]
mod tests;
