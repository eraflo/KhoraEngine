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

//! egui font definitions built from a [`FontPack`] — one recipe shared by the
//! editor shell and the eframe tool apps, so both install fonts the same way.

use std::sync::Arc;

use khora_core::ui::{FontHandle, FontPack, NamedFont};

/// The egui family name [`FontFamilyHint::Display`](khora_core::ui::editor::FontFamilyHint::Display) resolves to.
pub(crate) const DISPLAY: &str = "display";
/// The egui family name [`FontFamilyHint::Wordmark`](khora_core::ui::editor::FontFamilyHint::Wordmark) resolves to.
pub(crate) const WORDMARK: &str = "wordmark";
/// The egui family name [`FontFamilyHint::Icons`](khora_core::ui::editor::FontFamilyHint::Icons) resolves to.
pub(crate) const ICONS: &str = "icons";

/// egui's font definitions for `pack`: each family's faces in pack order —
/// the first is the family's primary face, the rest its fallbacks — ahead of
/// egui's built-in faces.
///
/// Every named family resolves to glyphs even when the pack brings no face for
/// it, because epaint panics on text asking for a `FontFamily::Name` bound to
/// no fonts: display falls back to the proportional faces, the wordmark to
/// display then proportional, the icons to proportional (tofu, not a crash).
pub(crate) fn font_definitions(pack: &FontPack) -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    install(
        &mut defs,
        egui::FontFamily::Proportional,
        &pack.proportional,
    );
    install(&mut defs, egui::FontFamily::Monospace, &pack.monospace);
    install(&mut defs, named(DISPLAY), &pack.display);
    install(&mut defs, named(WORDMARK), &pack.wordmark);
    install(&mut defs, named(ICONS), &pack.icons);

    append_fallback(&mut defs, DISPLAY, &egui::FontFamily::Proportional);
    append_fallback(&mut defs, WORDMARK, &named(DISPLAY));
    append_fallback(&mut defs, ICONS, &egui::FontFamily::Proportional);
    defs
}

fn named(name: &str) -> egui::FontFamily {
    egui::FontFamily::Name(name.into())
}

/// Puts `faces` at the head of `family`, in order.
fn install(defs: &mut egui::FontDefinitions, family: egui::FontFamily, faces: &[NamedFont]) {
    let mut keys = Vec::with_capacity(faces.len());
    for face in faces {
        let data = match &face.data {
            FontHandle::Static(bytes) => egui::FontData::from_static(bytes),
            FontHandle::Owned(bytes) => egui::FontData::from_owned(bytes.clone()),
        };
        defs.font_data.insert(face.name.clone(), Arc::new(data));
        keys.push(face.name.clone());
    }
    defs.families.entry(family).or_default().splice(0..0, keys);
}

/// Appends `fallback`'s faces after whatever `name` already holds.
fn append_fallback(defs: &mut egui::FontDefinitions, name: &str, fallback: &egui::FontFamily) {
    let fallback_keys = defs.families.get(fallback).cloned().unwrap_or_default();
    let entry = defs.families.entry(named(name)).or_default();
    for key in fallback_keys {
        if !entry.contains(&key) {
            entry.push(key);
        }
    }
}
