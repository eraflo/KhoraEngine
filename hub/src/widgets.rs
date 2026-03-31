// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Reusable UI widgets for the Khora Hub.

use crate::theme::pal;
use eframe::egui;

/// Draws a 4-pointed compass/diamond star (Khora logo shape) using the painter.
pub fn paint_khora_star(
    painter: &egui::Painter,
    center: egui::Pos2,
    size: f32,
    color: egui::Color32,
) {
    use egui::Pos2;
    use egui::epaint::{PathShape, PathStroke};
    let s = size;
    let t = s * 0.28;
    let points = vec![
        Pos2::new(center.x, center.y - s),
        Pos2::new(center.x + t, center.y - t),
        Pos2::new(center.x + s, center.y),
        Pos2::new(center.x + t, center.y + t),
        Pos2::new(center.x, center.y + s),
        Pos2::new(center.x - t, center.y + t),
        Pos2::new(center.x - s, center.y),
        Pos2::new(center.x - t, center.y - t),
    ];
    painter.add(egui::Shape::Path(PathShape {
        points,
        closed: true,
        fill: color,
        stroke: PathStroke::NONE,
    }));
}

/// Draws a thin horizontal separator line.
pub fn paint_separator(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, color);
}

/// A small colored badge/chip (e.g. version tag).
pub fn badge(ui: &mut egui::Ui, text: &str, bg: egui::Color32, fg: egui::Color32) {
    egui::Frame::none()
        .fill(bg)
        .rounding(egui::Rounding::same(4_f32))
        .inner_margin(egui::Margin {
            left: 6.0,
            right: 6.0,
            top: 2.0,
            bottom: 2.0,
        })
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(10.5).color(fg));
        });
}

/// A status chip with icon-like coloring.
pub fn status_chip(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    egui::Frame::none()
        .fill(color.gamma_multiply(0.15))
        .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.4)))
        .rounding(egui::Rounding::same(4_f32))
        .inner_margin(egui::Margin {
            left: 8.0,
            right: 8.0,
            top: 4.0,
            bottom: 4.0,
        })
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(11.0).color(color));
        });
}

/// A section header label styled like Linear/Vercel section titles.
pub fn section_header(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .strong()
            .size(12.0)
            .color(pal::TEXT_DIM),
    );
}

/// A form field label.
pub fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.0).color(pal::TEXT_DIM));
}

/// A sidebar nav button.
pub fn sidebar_nav_btn(ui: &mut egui::Ui, label: &str, active: bool) {
    let fill = if active {
        pal::PRIMARY_DIM
    } else {
        egui::Color32::TRANSPARENT
    };
    let text_color = if active { pal::PRIMARY } else { pal::TEXT_DIM };
    ui.add_sized(
        [180.0, 30.0],
        egui::Button::new(egui::RichText::new(label).size(12.5).color(text_color))
            .fill(fill)
            .stroke(egui::Stroke::NONE),
    );
}

/// A top-bar tab pill.
pub fn tab_pill(ui: &mut egui::Ui, label: &str, active: bool) {
    let fill = if active {
        pal::PRIMARY_DIM
    } else {
        egui::Color32::TRANSPARENT
    };
    let text_color = if active { pal::PRIMARY } else { pal::TEXT_DIM };
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(12.5).color(text_color))
            .fill(fill)
            .stroke(egui::Stroke::NONE),
    );
}

/// Format a unix timestamp into a human-readable relative string.
pub fn format_ts(ts: u64) -> String {
    if ts == 0 {
        return "Never".to_owned();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let diff = now.saturating_sub(ts);
    if diff < 60 {
        "Just now".to_owned()
    } else if diff < 3600 {
        format!("{} min ago", diff / 60)
    } else if diff < 86400 {
        format!("{} h ago", diff / 3600)
    } else {
        format!("{} days ago", diff / 86400)
    }
}
