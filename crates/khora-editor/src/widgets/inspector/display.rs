// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! How the inspector names and pictures things: an entity's icon and type
//! tag, a domain's icon and label.

use khora_sdk::editor_ui::{Icon, InspectedEntity};

/// Picks an Inspector header icon based on the most "interesting"
/// component on the entity. The fallback is a cube (mesh-like).
pub fn pick_icon(i: &InspectedEntity) -> Icon {
    let names: std::collections::HashSet<&str> = i
        .components_json
        .iter()
        .map(|c| c.type_name.as_str())
        .collect();
    if names.contains("Camera") {
        Icon::Camera
    } else if names.contains("Light") {
        Icon::Light
    } else if names.contains("AudioSource") || names.contains("AudioListener") {
        Icon::Music
    } else {
        Icon::Cube
    }
}

/// Picks a short type tag for the Inspector meta row.
pub fn pick_type_tag(i: &InspectedEntity) -> &'static str {
    let names: std::collections::HashSet<&str> = i
        .components_json
        .iter()
        .map(|c| c.type_name.as_str())
        .collect();
    if names.contains("Camera") {
        "Camera"
    } else if names.contains("Light") {
        "Light"
    } else if names.contains("AudioSource") || names.contains("AudioListener") {
        "Audio"
    } else {
        "Mesh"
    }
}

/// Returns a component card's icon for its domain tag.
pub fn icon_for_domain_tag(tag: Option<u8>) -> Icon {
    match tag {
        Some(0) => Icon::Axes,   // Spatial
        Some(1) => Icon::Image,  // Render
        Some(2) => Icon::Music,  // Audio
        Some(3) => Icon::Zap,    // Physics
        Some(4) => Icon::Layers, // UI
        Some(5) => Icon::Code,   // Script
        _ => Icon::More,
    }
}

/// Returns the "+ Add Component" submenu label for a domain tag.
pub fn category_label_for_tag(tag: Option<u8>) -> &'static str {
    match tag {
        Some(0) => "Spatial",
        Some(1) => "Render",
        Some(2) => "Audio",
        Some(3) => "Physics",
        Some(4) => "UI",
        Some(5) => "Script",
        _ => "Other",
    }
}
