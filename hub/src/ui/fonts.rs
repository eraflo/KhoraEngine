// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Brand fonts loader — Geist / Geist Mono / Fraunces / Lucide.
//!
//! Produces a [`FontPack`] backend-neutrally. The engine's
//! `AppContext::set_fonts` does the lifting onto egui. Shares the exact
//! font set with the editor so the two apps render identically.
//!
//! If a .ttf file is missing that family is simply skipped — the backend
//! falls back (Fraunces → Geist, no icons → tofu). Drop the files into
//! `hub/assets/fonts/` to enable the brand typography (licenses: SIL OFL —
//! Geist <https://github.com/vercel/geist-font>, Fraunces, and Lucide).

use khora_sdk::tool_ui::{FontHandle, FontPack, NamedFont};
use std::path::{Path, PathBuf};

/// Builds a [`FontPack`] populated with Geist / Geist Mono if the
/// files can be found, or returns an empty pack otherwise.
///
/// The list is searched in order:
/// 1. `<exe-dir>/assets/fonts/` (deployed layout)
/// 2. `hub/assets/fonts/` (dev layout via `CARGO_MANIFEST_DIR`)
pub fn build_pack() -> FontPack {
    let mut pack = FontPack::default();
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
    // Regular-then-semibold so the heavier face is the primary display face;
    // optional, absent files leave display aliased to proportional.
    let display: &[(&str, &str)] = &[
        ("fraunces-regular", "Fraunces-Regular.ttf"),
        ("fraunces-semibold", "Fraunces-SemiBold.ttf"),
    ];
    let icons: &[(&str, &str)] = &[("lucide", "Lucide.ttf")];

    let prop_count = install_family(&mut pack.proportional, &candidates, proportional);
    let mono_count = install_family(&mut pack.monospace, &candidates, monospace);
    let disp_count = install_family(&mut pack.display, &candidates, display);
    let icon_count = install_family(&mut pack.icons, &candidates, icons);

    if prop_count + mono_count + disp_count + icon_count == 0 {
        log::info!(
            "Hub fonts: no font files found under {:?} \u{2014} keeping default fonts. \
             Drop the .ttf files into 'hub/assets/fonts/' to enable the brand typography.",
            candidates
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
        );
    } else {
        log::info!(
            "Hub fonts: loaded {} proportional + {} monospace + {} display + {} icon face(s).",
            prop_count,
            mono_count,
            disp_count,
            icon_count
        );
    }

    pack
}

fn install_family(family: &mut Vec<NamedFont>, roots: &[PathBuf], files: &[(&str, &str)]) -> usize {
    let mut installed = 0;
    for (name, file) in files {
        let Some(found) = find_in_roots(roots, file) else {
            continue;
        };
        match std::fs::read(&found) {
            Ok(bytes) => {
                family.push(NamedFont {
                    name: (*name).to_owned(),
                    data: FontHandle::Owned(bytes),
                });
                installed += 1;
            }
            Err(e) => log::warn!("Hub fonts: failed to read {}: {}", found.display(), e),
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
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        out.push(parent.join("assets/fonts"));
    }
    out.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts"));
    out
}
