// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Brand fonts loader — Geist / Geist Mono.
//!
//! Mirrors the editor's font loader (`crates/khora-editor/src/fonts.rs`) but
//! stays self-contained: the hub has zero engine dependencies by design, so
//! we hand-roll the same logic against `egui::FontDefinitions`.
//!
//! If the .ttf files are missing, this is a no-op — the hub falls back to
//! egui's built-in fonts. Drop the files into `hub/assets/fonts/` to enable
//! the brand typography (license: SIL OFL, see
//! <https://github.com/vercel/geist-font>).

use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Builds a [`FontDefinitions`] populated with Geist / Geist Mono if the
/// files can be found, or returns the egui defaults otherwise.
///
/// The list is searched in order:
/// 1. `<exe-dir>/assets/fonts/` (deployed layout)
/// 2. `hub/assets/fonts/` (dev layout via `CARGO_MANIFEST_DIR`)
pub fn build_definitions() -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    let candidates = candidate_roots();

    let proportional: &[(&str, &str)] = &[
        ("geist-regular", "Geist-Regular.ttf"),
        ("geist-medium", "Geist-Medium.ttf"),
        ("geist-semibold", "Geist-SemiBold.ttf"),
    ];
    let monospace: &[(&str, &str)] = &[
        ("geist-mono-regular", "GeistMono-Regular.ttf"),
        ("geist-mono-medium", "GeistMono-Medium.ttf"),
    ];

    let prop_count = install_into(&mut defs, &candidates, proportional, egui::FontFamily::Proportional);
    let mono_count = install_into(&mut defs, &candidates, monospace, egui::FontFamily::Monospace);

    if prop_count + mono_count == 0 {
        log::info!(
            "Hub fonts: no Geist files found under {:?} \u{2014} keeping default fonts. \
             Drop the .ttf files into 'hub/assets/fonts/' to enable the brand typography.",
            candidates
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
        );
    } else {
        log::info!(
            "Hub fonts: loaded {} proportional + {} monospace face(s).",
            prop_count, mono_count
        );
    }

    defs
}

fn install_into(
    defs: &mut egui::FontDefinitions,
    roots: &[PathBuf],
    files: &[(&str, &str)],
    family: egui::FontFamily,
) -> usize {
    let mut installed = 0;
    for (name, file) in files {
        let Some(found) = find_in_roots(roots, file) else {
            continue;
        };
        match std::fs::read(&found) {
            Ok(bytes) => {
                let key = (*name).to_owned();
                defs.font_data
                    .insert(key.clone(), Arc::new(egui::FontData::from_owned(bytes)));
                defs.families
                    .entry(family.clone())
                    .or_default()
                    .insert(0, key);
                installed += 1;
            }
            Err(e) => log::warn!(
                "Hub fonts: failed to read {}: {}",
                found.display(),
                e
            ),
        }
    }
    installed
}

fn find_in_roots(roots: &[PathBuf], filename: &str) -> Option<PathBuf> {
    for root in roots {
        let candidate = root.join(filename);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn candidate_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            out.push(parent.join("assets/fonts"));
        }
    }
    out.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts"));
    out
}
