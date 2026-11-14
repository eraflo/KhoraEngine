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

//! The inspector's decision cards: what the DCC's last decision lent the
//! selected agent, why, and what the DCC learnt of the agent's rows.

use std::time::Duration;

use khora_sdk::editor_ui::*;
use khora_sdk::khora_core::agent::gorna::protocol::{
    BindingConstraint, DecidedBy, Envelope, Trigger,
};
use khora_sdk::khora_core::agent::gorna::DecisionView;
use khora_sdk::khora_core::lane::cost::Device;
use khora_sdk::StrategyId;
use khora_tool_ui::widgets::paint;
use khora_tool_ui::widgets::with_alpha;

use super::paint::{paint_card_box, wrapped};
use super::AgentSnapshot;

/// The height of a key/value line.
const LINE: f32 = 22.0;

/// What the Decision card says before any round.
const NOT_LENT_YET: &str = "The DCC has not lent this agent anything yet.";
const WHAT_A_ROUND_IS: &str = "It runs its default row until the first round. A round opens when \
                               the frame governor sees pressure, when a device or the agent \
                               itself asks, or every few seconds to re-plan.";
const READ_BACK: &str = "Read back from a decision journal, not decided by this run.";
const KEPT: &str = "Kept the envelope it held: no row planned.";
const NO_ROW_MEASURED: &str = "No row measured yet.";
const SWITCH_FOOTNOTE: &str =
    "Switch: what a move to that row is charged — its preparation and the swap.";

/// One card's place in the inspector: its left edge and its width.
#[derive(Clone, Copy)]
pub(super) struct Card {
    pub(super) x: f32,
    pub(super) w: f32,
}

impl Card {
    fn inner_x(self) -> f32 {
        self.x + 10.0
    }

    fn inner_w(self) -> f32 {
        self.w - 20.0
    }

    /// Paints the card's header titled `title`, with `tag` — its text and
    /// colour — right-aligned on it. Returns where the card's lines start.
    fn header(
        self,
        ui: &mut dyn UiBuilder,
        y: f32,
        title: &str,
        icon: Icon,
        (tag, tag_color): (&str, [f32; 4]),
        theme: &UiTheme,
    ) -> f32 {
        let below = paint_card_box(ui, self.x, y, self.w, title, icon, theme);
        ui.paint_text_styled(
            [self.x + self.w - 8.0, y + 8.0],
            tag,
            10.0,
            tag_color,
            FontFamilyHint::Monospace,
            TextAlign::Right,
        );
        below
    }

    /// Paints a key on the left and its value, in monospace, on the right.
    fn line(
        self,
        ui: &mut dyn UiBuilder,
        y: f32,
        key: &str,
        value: &str,
        color: [f32; 4],
        theme: &UiTheme,
    ) -> f32 {
        paint::text(ui, [self.inner_x(), y + 6.0], key, 11.0, theme.text_dim);
        ui.paint_text_styled(
            [self.inner_x() + self.inner_w() - 4.0, y + 6.0],
            value,
            11.0,
            color,
            FontFamilyHint::Monospace,
            TextAlign::Right,
        );
        y + LINE
    }

    /// Paints a muted sub-label over a group of lines.
    fn sub_label(self, ui: &mut dyn UiBuilder, y: f32, label: &str, theme: &UiTheme) -> f32 {
        paint::text(ui, [self.inner_x(), y + 6.0], label, 10.5, theme.text_muted);
        y + 18.0
    }

    /// Paints a sentence across the card, word-wrapped.
    fn sentence(
        self,
        ui: &mut dyn UiBuilder,
        y: f32,
        text: &str,
        size: f32,
        color: [f32; 4],
    ) -> f32 {
        wrapped(
            ui,
            [self.inner_x(), y + 6.0],
            text,
            size,
            color,
            FontFamilyHint::Proportional,
            self.inner_w(),
        ) + 2.0
    }
}

/// Paints the Decision card for `agent` from `y`, and returns where the next
/// card goes.
///
/// When `view` lends the agent an envelope, the card of what the DCC learnt
/// of its rows follows. See the book: concepts/gorna.md, "How an agent chooses".
pub(super) fn paint_decision_cards(
    ui: &mut dyn UiBuilder,
    card: Card,
    y: f32,
    agent: &AgentSnapshot,
    view: Option<&DecisionView>,
    theme: &UiTheme,
) -> f32 {
    let (tag, tag_color) = match view {
        None => ("none yet", theme.text_muted),
        Some(view) if view.replayed => ("replayed · journal", theme.primary),
        Some(_) => ("live", theme.text_muted),
    };
    let mut cy = card.header(ui, y, "Decision", Icon::Scale, (tag, tag_color), theme);

    let Some(view) = view else {
        cy = card.sentence(ui, cy, NOT_LENT_YET, 11.5, theme.text);
        cy = card.sentence(ui, cy, WHAT_A_ROUND_IS, 11.0, theme.text_dim);
        return cy + 10.0;
    };
    if view.replayed {
        cy = card.sentence(ui, cy, READ_BACK, 10.5, theme.text_muted);
    }
    let lent = (view.envelopes.iter())
        .find(|(named, _)| *named == agent.id)
        .map(|(_, envelope)| *envelope);
    let Some(lent) = lent else {
        let line = format!("Not lent in epoch {}.", view.epoch.0);
        cy = card.sentence(ui, cy, &line, 11.0, theme.text_muted);
        return cy + 10.0;
    };

    cy = paint_lent(ui, card, cy, &lent, theme);
    cy = paint_rows(ui, card, cy, agent, view, theme);
    cy = paint_why(ui, card, cy, agent, view, theme);
    cy = paint_learnt(ui, card, cy + 10.0, agent, view, theme);
    cy + 10.0
}

/// Paints what the agent was lent: its time on the host and the GPU, and its
/// memory over every device; a device lent nothing is not named.
fn paint_lent(ui: &mut dyn UiBuilder, card: Card, y: f32, lent: &Envelope, theme: &UiTheme) -> f32 {
    let mut cy = card.sub_label(ui, y + 2.0, "Lent", theme);
    cy = card.line(
        ui,
        cy,
        "Host time",
        &millis(lent.time.cpu),
        theme.text,
        theme,
    );
    if !lent.time.gpu.is_zero() {
        cy = card.line(
            ui,
            cy,
            "GPU time",
            &millis(lent.time.gpu),
            theme.text,
            theme,
        );
    }
    // A device whose memory the round did not know is lent `u64::MAX`: the
    // envelope bounds nothing there.
    let devices = [
        lent.memory.cpu,
        lent.memory.gpu,
        lent.memory.npu,
        lent.memory.other,
    ];
    if devices.contains(&u64::MAX) {
        return card.line(ui, cy, "Memory", "not limited", theme.text_dim, theme);
    }
    let memory = (devices.iter()).fold(0u64, |sum, bytes| sum.saturating_add(*bytes));
    if memory > 0 {
        cy = card.line(ui, cy, "Memory", &bytes(memory), theme.text, theme);
    }
    cy
}

/// Paints the row planned for the agent and the row it runs, the running one
/// in gold.
fn paint_rows(
    ui: &mut dyn UiBuilder,
    card: Card,
    y: f32,
    agent: &AgentSnapshot,
    view: &DecisionView,
    theme: &UiTheme,
) -> f32 {
    let running = agent.report.row;
    let planned = (view.planned.iter())
        .find(|(named, _)| *named == agent.id)
        .map(|(_, row)| *row);
    match planned {
        Some(planned) if planned == running => card.line(
            ui,
            y,
            "Planned row",
            &row_name(planned),
            theme.accent_c,
            theme,
        ),
        Some(planned) => {
            let cy = card.line(ui, y, "Planned row", &row_name(planned), theme.text, theme);
            let cy = card.line(
                ui,
                cy,
                "Running row",
                &row_name(running),
                theme.accent_c,
                theme,
            );
            let note = format!(
                "Still on {}: the swap to {} lands at the next frame start.",
                row_name(running),
                row_name(planned)
            );
            card.sentence(ui, cy, &note, 11.0, theme.warning)
        }
        None => {
            let cy = card.line(
                ui,
                y,
                "Running row",
                &row_name(running),
                theme.accent_c,
                theme,
            );
            card.sentence(ui, cy, KEPT, 10.5, theme.text_muted)
        }
    }
}

/// Paints why the round decided as it did, and the upgrades the agent was
/// refused, each with what it broke.
fn paint_why(
    ui: &mut dyn UiBuilder,
    card: Card,
    y: f32,
    agent: &AgentSnapshot,
    view: &DecisionView,
    theme: &UiTheme,
) -> f32 {
    let mut cy = card.sub_label(ui, y + 8.0, "Why", theme);
    cy = card.line(
        ui,
        cy,
        "Opened by",
        trigger_name(view.trigger),
        theme.text,
        theme,
    );
    cy = card.line(
        ui,
        cy,
        "Decided by",
        decider_name(view.why.by),
        theme.text,
        theme,
    );
    if let Some(binding) = view.why.binding {
        cy = card.line(
            ui,
            cy,
            "Frame bound by",
            &binding_name(binding),
            theme.text,
            theme,
        );
    }
    if !view.planning.margin.is_zero() {
        cy = card.line(
            ui,
            cy,
            "Risk margin kept",
            &millis(view.planning.margin),
            theme.text,
            theme,
        );
    }
    for refused in (view.why.rejected.iter()).filter(|refused| refused.agent == agent.id) {
        cy = paint_refusal(ui, card, cy + 6.0, refused.row, refused.over, theme);
    }
    cy
}

/// Paints one refused upgrade on an amber tint: the row, and what it broke.
fn paint_refusal(
    ui: &mut dyn UiBuilder,
    card: Card,
    y: f32,
    row: StrategyId,
    over: BindingConstraint,
    theme: &UiTheme,
) -> f32 {
    let (x, w) = (card.inner_x(), card.inner_w());
    ui.paint_rect_filled(
        [x - 4.0, y],
        [w + 4.0, 42.0],
        with_alpha(theme.warning, 0.08),
        theme.radius_sm,
    );
    paint::icon(ui, [x + 2.0, y + 7.0], Icon::Warn, 11.0, theme.warning);
    paint::text(
        ui,
        [x + 18.0, y + 6.0],
        "Upgrade refused",
        11.0,
        theme.warning,
    );
    ui.paint_text_styled(
        [x + 4.0, y + 24.0],
        &row_name(row),
        11.0,
        theme.text,
        FontFamilyHint::Monospace,
        TextAlign::Left,
    );
    ui.paint_text_styled(
        [x + w - 4.0, y + 24.0],
        &broke(over),
        10.5,
        theme.text_dim,
        FontFamilyHint::Proportional,
        TextAlign::Right,
    );
    y + 46.0
}

/// Paints what the DCC learnt of each of the agent's rows: its measured time
/// over its quote, the spread, and what a switch to it is charged.
fn paint_learnt(
    ui: &mut dyn UiBuilder,
    card: Card,
    y: f32,
    agent: &AgentSnapshot,
    view: &DecisionView,
    theme: &UiTheme,
) -> f32 {
    let mut cy = card.header(
        ui,
        y,
        "What the DCC learnt",
        Icon::Database,
        ("measured ÷ quoted", theme.text_muted),
        theme,
    );
    let rows: Vec<_> = (view.corrections.iter())
        .filter(|(named, _, _)| *named == agent.id)
        .collect();
    if rows.is_empty() {
        return card.sentence(ui, cy, NO_ROW_MEASURED, 10.5, theme.text_muted);
    }

    // Columns, right-aligned from the card's inner right edge.
    let right = card.inner_x() + card.inner_w() - 4.0;
    let columns = [right - 120.0, right - 62.0, right];
    let header = ["×k", "±σ", "Switch"];
    paint::text(
        ui,
        [card.inner_x(), cy + 4.0],
        "Row",
        10.0,
        theme.text_muted,
    );
    for (column, title) in columns.iter().zip(header) {
        ui.paint_text_styled(
            [*column, cy + 4.0],
            title,
            10.0,
            theme.text_muted,
            FontFamilyHint::Proportional,
            TextAlign::Right,
        );
    }
    cy += 20.0;

    let running = agent.report.row;
    for (_, row, correction) in rows {
        let runs = *row == running;
        if runs {
            ui.paint_rect_filled(
                [card.x + 2.0, cy],
                [card.w - 4.0, LINE],
                with_alpha(theme.accent_c, 0.08),
                theme.radius_sm,
            );
        }
        let name_color = if runs { theme.accent_c } else { theme.text_dim };
        let charge = if runs {
            "running".to_owned()
        } else {
            (view.planning.switch.iter())
                .find(|(named, switched, _)| *named == agent.id && switched == row)
                .map_or_else(|| "—".to_owned(), |(_, _, charge)| millis(*charge))
        };
        let cells = [
            (format!("{:.2}", correction.factor), theme.text),
            (format!("{:.2}", correction.spread), theme.text_dim),
            (
                charge,
                if runs {
                    theme.text_muted
                } else {
                    theme.text_dim
                },
            ),
        ];
        ui.paint_text_styled(
            [card.inner_x(), cy + 5.0],
            &row_name(*row),
            11.0,
            name_color,
            FontFamilyHint::Monospace,
            TextAlign::Left,
        );
        for (column, (cell, color)) in columns.iter().zip(cells) {
            ui.paint_text_styled(
                [*column, cy + 5.0],
                &cell,
                11.0,
                color,
                FontFamilyHint::Monospace,
                TextAlign::Right,
            );
        }
        cy += LINE;
    }
    card.sentence(ui, cy + 2.0, SWITCH_FOOTNOTE, 10.5, theme.text_muted)
}

/// Returns `time` in milliseconds, to two places.
fn millis(time: Duration) -> String {
    format!("{:.2} ms", time.as_secs_f64() * 1_000.0)
}

/// Returns `amount` bytes in the largest unit it holds a whole one of, so a
/// few hundred bytes never read as `0 MiB`.
fn bytes(amount: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    if amount < KIB {
        format!("{amount} B")
    } else if amount < MIB {
        format!("{:.1} KiB", amount as f64 / KIB as f64)
    } else {
        format!("{:.0} MiB", amount as f64 / MIB as f64)
    }
}

/// Returns a row as tools name it.
fn row_name(row: StrategyId) -> String {
    match row {
        StrategyId::LowPower => "LowPower".to_owned(),
        StrategyId::Balanced => "Balanced".to_owned(),
        StrategyId::HighPerformance => "HighPerformance".to_owned(),
        StrategyId::Custom(n) => format!("Custom({n})"),
    }
}

/// Returns what opened a round, in words.
fn trigger_name(trigger: Trigger) -> &'static str {
    match trigger {
        Trigger::Initial => "Initial",
        Trigger::Pressure => "Pressure",
        Trigger::ForecastBreach => "Forecast breach",
        Trigger::FrameGovernor => "Frame governor",
        Trigger::AgentsChanged => "Agents changed",
        Trigger::PolicyChanged => "Policy changed",
        Trigger::AgentStalled => "Agent stalled",
        Trigger::Overruns => "Overruns",
        Trigger::Refresh => "Refresh",
        Trigger::Safety => "Safety",
    }
}

/// Returns what decided a round, in words.
fn decider_name(by: DecidedBy) -> &'static str {
    match by {
        DecidedBy::Floors => "Floors only",
        DecidedBy::Greedy => "Greedy",
        DecidedBy::Exact => "Exact search",
        DecidedBy::Planner => "Planner",
    }
}

/// Returns a device as the constraints name it.
fn device_name(device: Device) -> &'static str {
    match device {
        Device::Cpu => "host",
        Device::Gpu => "GPU",
        Device::Npu => "NPU",
        Device::Other => "other device",
    }
}

/// Returns the constraint that bound the frame, in words.
fn binding_name(binding: BindingConstraint) -> String {
    match binding {
        BindingConstraint::Time(Device::Cpu) => "Host time".to_owned(),
        BindingConstraint::Memory(Device::Cpu) => "Host memory".to_owned(),
        BindingConstraint::Time(device) => format!("{} time", device_name(device)),
        BindingConstraint::Memory(device) => format!("{} memory", device_name(device)),
        BindingConstraint::Ceiling(device) => format!("Stable ceiling, {}", device_name(device)),
    }
}

/// Returns what a refused upgrade broke, in words.
fn broke(over: BindingConstraint) -> String {
    match over {
        BindingConstraint::Time(device) => format!("over the frame, {}", device_name(device)),
        BindingConstraint::Memory(device) => format!("over {} memory", device_name(device)),
        BindingConstraint::Ceiling(device) => {
            format!("over its Stable ceiling, {}", device_name(device))
        }
    }
}
