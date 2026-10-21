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

//! The `"wordmark"` font family, through both egui font installers: the
//! editor shell's and the standalone tool app context's.
//!
//! A pack's wordmark faces become the family's primary fonts; a pack without
//! any still binds the family — to the display faces first, then to the
//! proportional ones — because epaint panics on text asking for a
//! `FontFamily::Name` bound to no fonts.

use egui::{FontFamily, Pos2};
use khora_core::ui::editor::shell::EditorShell;
use khora_core::ui::fonts::{FontHandle, FontPack, NamedFont};
use khora_core::ui::{AppContext, UiTheme};
use khora_infra::ui::egui::EguiAppContext;
use khora_infra::EguiEditorShell;

/// The two places a [`FontPack`] enters egui.
#[derive(Debug, Clone, Copy)]
enum Installer {
    /// `EguiEditorShell::set_fonts`, the editor's path.
    EditorShell,
    /// `EguiAppContext::set_fonts`, the standalone tools' (hub) path.
    AppContext,
}

fn wordmark() -> FontFamily {
    FontFamily::Name("wordmark".into())
}

fn input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(400.0, 300.0),
        )),
        ..Default::default()
    }
}

/// A real font file, borrowed from egui's built-in set so the face parses
/// when egui loads it.
fn real_font_bytes() -> Vec<u8> {
    egui::FontDefinitions::default().font_data["Ubuntu-Light"]
        .font
        .to_vec()
}

fn face(name: &str) -> NamedFont {
    NamedFont {
        name: name.to_owned(),
        data: FontHandle::Owned(real_font_bytes()),
    }
}

/// A context whose fonts are `pack`, installed through `installer` and live
/// (egui swaps fonts in at the start of the next pass).
fn installed(installer: Installer, pack: FontPack) -> egui::Context {
    let ctx = egui::Context::default();
    match installer {
        Installer::EditorShell => {
            let mut shell = EguiEditorShell::new(ctx.clone(), UiTheme::default());
            shell.set_fonts(pack);
        }
        Installer::AppContext => {
            let _ = ctx.run_ui(input(), |ui| {
                let mut frame = eframe::Frame::_new_kittest();
                let mut app = EguiAppContext::new(ui, &mut frame);
                app.set_fonts(&pack);
            });
        }
    }
    let _ = ctx.run_ui(input(), |_| {});
    ctx
}

/// The font keys bound to the `"wordmark"` family, in fallback order.
fn wordmark_fonts(ctx: &egui::Context) -> Option<Vec<String>> {
    ctx.fonts(|fonts| fonts.definitions().families.get(&wordmark()).cloned())
}

fn assert_wordmark_face_is_primary(installer: Installer) {
    let ctx = installed(
        installer,
        FontPack {
            proportional: vec![face("body-face")],
            display: vec![face("display-face")],
            wordmark: vec![face("wordmark-face")],
            ..FontPack::default()
        },
    );
    let fonts = wordmark_fonts(&ctx)
        .unwrap_or_else(|| panic!("{installer:?}: no \"wordmark\" family was installed"));
    assert_eq!(
        fonts.first().map(String::as_str),
        Some("wordmark-face"),
        "{installer:?}: the pack's wordmark face leads the \"wordmark\" family, got {fonts:?}"
    );
    assert!(
        ctx.fonts(|f| f.definitions().font_data.contains_key("wordmark-face")),
        "{installer:?}: the wordmark face's bytes are registered"
    );
}

fn assert_missing_wordmark_falls_back_to_display_then_proportional(installer: Installer) {
    let ctx = installed(
        installer,
        FontPack {
            proportional: vec![face("body-face")],
            display: vec![face("display-face")],
            ..FontPack::default()
        },
    );
    let fonts = wordmark_fonts(&ctx).unwrap_or_else(|| {
        panic!("{installer:?}: a pack without wordmark faces still gets a \"wordmark\" family")
    });
    let display = fonts.iter().position(|k| k == "display-face");
    let body = fonts.iter().position(|k| k == "body-face");
    assert!(
        display.is_some() && body.is_some() && display < body,
        "{installer:?}: the \"wordmark\" family falls back to the display face, then the \
         proportional one, got {fonts:?}"
    );
}

fn assert_missing_wordmark_and_display_fall_back_to_proportional(installer: Installer) {
    let ctx = installed(
        installer,
        FontPack {
            proportional: vec![face("body-face")],
            ..FontPack::default()
        },
    );
    let fonts = wordmark_fonts(&ctx).unwrap_or_else(|| {
        panic!("{installer:?}: a pack without wordmark faces still gets a \"wordmark\" family")
    });
    assert!(
        fonts.iter().any(|k| k == "body-face"),
        "{installer:?}: the \"wordmark\" family falls back to the proportional face, got {fonts:?}"
    );
}

fn assert_wordmark_text_lays_out_without_a_wordmark_face(installer: Installer) {
    let ctx = installed(
        installer,
        FontPack {
            proportional: vec![face("body-face")],
            ..FontPack::default()
        },
    );
    // Laying text out in a family bound to no fonts panics inside epaint.
    let _ = ctx.run_ui(input(), |ui| {
        ui.label(egui::RichText::new("KHORA").family(wordmark()));
    });
}

#[test]
fn editor_shell_installs_the_wordmark_face_as_the_primary_wordmark_font() {
    assert_wordmark_face_is_primary(Installer::EditorShell);
}

#[test]
fn app_context_installs_the_wordmark_face_as_the_primary_wordmark_font() {
    assert_wordmark_face_is_primary(Installer::AppContext);
}

#[test]
fn editor_shell_falls_back_from_a_missing_wordmark_to_display_then_proportional() {
    assert_missing_wordmark_falls_back_to_display_then_proportional(Installer::EditorShell);
}

#[test]
fn app_context_falls_back_from_a_missing_wordmark_to_display_then_proportional() {
    assert_missing_wordmark_falls_back_to_display_then_proportional(Installer::AppContext);
}

#[test]
fn editor_shell_falls_back_from_a_missing_wordmark_and_display_to_proportional() {
    assert_missing_wordmark_and_display_fall_back_to_proportional(Installer::EditorShell);
}

#[test]
fn app_context_falls_back_from_a_missing_wordmark_and_display_to_proportional() {
    assert_missing_wordmark_and_display_fall_back_to_proportional(Installer::AppContext);
}

#[test]
fn editor_shell_lays_out_wordmark_text_when_the_pack_has_no_wordmark_face() {
    assert_wordmark_text_lays_out_without_a_wordmark_face(Installer::EditorShell);
}

#[test]
fn app_context_lays_out_wordmark_text_when_the_pack_has_no_wordmark_face() {
    assert_wordmark_text_lays_out_without_a_wordmark_face(Installer::AppContext);
}
