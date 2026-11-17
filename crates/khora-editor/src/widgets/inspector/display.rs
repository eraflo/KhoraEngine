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
    domain_label(tag).unwrap_or("Other")
}

/// Returns a domain tag's name, `None` for no tag or one no domain has.
pub fn domain_label(tag: Option<u8>) -> Option<&'static str> {
    match tag {
        Some(0) => Some("Spatial"),
        Some(1) => Some("Render"),
        Some(2) => Some("Audio"),
        Some(3) => Some("Physics"),
        Some(4) => Some("UI"),
        Some(5) => Some("Script"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{category_label_for_tag, domain_label};

    /// Each domain tag has its name, and the "+ Add Component" menu files the rest under `Other`.
    #[test]
    fn a_domain_tag_names_its_domain_and_the_menu_files_the_rest_under_other() {
        let names: Vec<_> = (0..6).map(|tag| domain_label(Some(tag))).collect();
        assert_eq!(
            names,
            ["Spatial", "Render", "Audio", "Physics", "UI", "Script"].map(Some)
        );
        assert_eq!(domain_label(None), None);
        assert_eq!(domain_label(Some(6)), None);
        assert_eq!(category_label_for_tag(Some(4)), "UI");
        assert_eq!(category_label_for_tag(None), "Other");
    }
}
