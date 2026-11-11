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

//! **The inspector shows the DCC's last decision for the selected agent**:
//! what it was lent, the row planned for it, why the round opened and who
//! decided, the upgrades it was refused, and what the DCC learnt of its rows —
//! or, before any round, what a round is. Its cards scroll under a fixed
//! header.
//!
//! Every frame is painted through [`EditorPanel::ui`] on a
//! [`RecordingUiBuilder`], over one registered agent, `probe`, and a
//! [`DecisionWatch`] a hand-built [`DecisionView`] is published into.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_sdk::editor_ui::{EditorPanel, EditorState, UiTheme};
use khora_sdk::khora_core::agent::gorna::protocol::{
    Binding, DecidedBy, Envelope, Epoch, Offer, OfferRequest, Rejected, Report, Trigger, Why,
};
use khora_sdk::khora_core::agent::gorna::{Correction, DecisionView, DecisionWatch, Planning};
use khora_sdk::khora_core::agent::{Agent, Place};
use khora_sdk::khora_core::lane::cost::{Device, PerDevice};
use khora_sdk::khora_core::platform::KeyCode;
use khora_sdk::khora_core::ui::editor::viewport_texture::ViewportTextureHandle;
use khora_sdk::khora_core::ui::editor::{
    FontFamilyHint, Interaction, LinearGradient, RadialGradient, TextAlign, UiBuilder,
};
use khora_sdk::{AgentId, AgentRegistry, StrategyId};
use khora_tool_ui::testing::{PaintCall, RecordingUiBuilder};

use super::super::{ControlPlanePanel, INSPECTOR_PANEL_WIDTH};

// ── The scene ─────────────────────────────────────────────────────────

/// Wide enough for the three columns; tall enough for the cards to be read.
const WIDE: f32 = 1400.0;
const TALL: f32 = 900.0;
/// Tall enough that every card fits: nothing to scroll.
const VERY_TALL: f32 = 4000.0;
/// Short enough that the cards overflow the inspector.
const SHORT: f32 = 360.0;

/// What the probe's last run said, so the Message card is painted.
const SAID: &str = "probe ran its row";

const MIB: u64 = 1024 * 1024;

fn probe() -> AgentId {
    AgentId::new("probe")
}

fn other() -> AgentId {
    AgentId::new("elsewhere")
}

fn ms(micros: u64) -> Duration {
    Duration::from_micros(micros)
}

/// An agent whose report says it runs `row`, healthy, saying [`SAID`]. Its
/// place runs nothing: the panel reads the agent only.
struct Running {
    row: StrategyId,
    place: Arc<Mutex<Place>>,
}

impl Agent for Running {
    fn id(&self) -> AgentId {
        probe()
    }

    fn offer(&mut self, request: &OfferRequest) -> Offer {
        Offer {
            agent: self.id(),
            epoch: request.epoch,
            rows: Vec::new(),
            floor: StrategyId::LowPower,
            current: self.row,
            health: 1.0,
        }
    }

    fn apply(
        &mut self,
        _epoch: Epoch,
        _envelope: &Envelope,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
    }

    fn report(&self) -> Report {
        Report {
            agent: self.id(),
            epoch: Epoch(7),
            row: self.row,
            measured: PerDevice::default(),
            done: 1.0,
            health: 1.0,
            over_budget: false,
            stalled: false,
            workload: None,
            switch: None,
            message: Some(SAID.to_owned()),
        }
    }

    fn place(&self) -> Arc<Mutex<Place>> {
        Arc::clone(&self.place)
    }
}

/// The Control Plane over one agent, `probe`, running `running`, reading
/// `decision`.
fn panel(running: StrategyId, decision: Option<DecisionWatch>) -> ControlPlanePanel {
    let mut registry = AgentRegistry::new();
    registry.register(
        Arc::new(Mutex::new(Running {
            row: running,
            place: Arc::new(Mutex::new(Place::new(probe()))),
        })),
        1.0,
    );
    ControlPlanePanel::new(
        Arc::new(Mutex::new(EditorState::default())),
        khora_tool_ui::khora_dark(),
        Some(Arc::new(Mutex::new(registry))),
        None,
        decision,
    )
}

/// A watch holding `view`.
fn watching(view: DecisionView) -> DecisionWatch {
    let watch = DecisionWatch::default();
    watch.publish(view);
    watch
}

/// One frame of `panel` over a `width` × `height` panel.
fn frame(panel: &mut ControlPlanePanel, width: f32, height: f32) -> RecordingUiBuilder {
    let mut ui = RecordingUiBuilder::new(width, height);
    panel.ui(&mut ui);
    ui
}

/// One wide, tall frame of the probe running `running`, reading `view`.
fn shown(running: StrategyId, view: DecisionView) -> RecordingUiBuilder {
    frame(&mut panel(running, Some(watching(view))), WIDE, TALL)
}

fn theme() -> UiTheme {
    khora_tool_ui::khora_dark()
}

/// The live decision of epoch 7 the probe was lent in: an exact search,
/// opened by the frame governor, bound by the GPU's time; the probe planned
/// on `Balanced`, refused `HighPerformance` (over the frame, GPU) and
/// `Custom(7)` (over its Stable ceiling, host); its four rows corrected and
/// three of them charged a switch. Another agent, `elsewhere`, is lent,
/// refused and corrected too — none of it is the probe's.
fn decision() -> DecisionView {
    DecisionView {
        epoch: Epoch(7),
        trigger: Trigger::FrameGovernor,
        target: ms(16_670),
        why: Why {
            by: DecidedBy::Exact,
            trigger: Trigger::FrameGovernor,
            binding: Some(Binding::Time(Device::Gpu)),
            rejected: vec![
                Rejected {
                    agent: probe(),
                    row: StrategyId::HighPerformance,
                    over: Binding::Time(Device::Gpu),
                },
                Rejected {
                    agent: other(),
                    row: StrategyId::Balanced,
                    over: Binding::Memory(Device::Cpu),
                },
                Rejected {
                    agent: probe(),
                    row: StrategyId::Custom(7),
                    over: Binding::Ceiling(Device::Cpu),
                },
            ],
        },
        envelopes: vec![
            (
                other(),
                Envelope {
                    time: PerDevice {
                        cpu: ms(9_750),
                        ..PerDevice::default()
                    },
                    memory: PerDevice::default(),
                },
            ),
            (
                probe(),
                Envelope {
                    time: PerDevice {
                        cpu: ms(4_250),
                        gpu: ms(3_100),
                        ..PerDevice::default()
                    },
                    memory: PerDevice {
                        cpu: 64 * MIB,
                        gpu: 192 * MIB,
                        ..PerDevice::default()
                    },
                },
            ),
        ],
        planned: vec![
            (other(), StrategyId::LowPower),
            (probe(), StrategyId::Balanced),
        ],
        corrections: vec![
            (
                probe(),
                StrategyId::LowPower,
                Correction {
                    factor: 0.91,
                    spread: 0.04,
                },
            ),
            (
                other(),
                StrategyId::Balanced,
                Correction {
                    factor: 9.99,
                    spread: 8.88,
                },
            ),
            (
                probe(),
                StrategyId::Balanced,
                Correction {
                    factor: 2.47,
                    spread: 0.19,
                },
            ),
            (
                probe(),
                StrategyId::HighPerformance,
                Correction {
                    factor: 1.12,
                    spread: 0.33,
                },
            ),
            (
                probe(),
                StrategyId::Custom(7),
                Correction {
                    factor: 3.05,
                    spread: 0.52,
                },
            ),
        ],
        planning: Planning {
            margin: ms(850),
            switch: vec![
                (probe(), StrategyId::LowPower, ms(1_750)),
                (probe(), StrategyId::Balanced, ms(660)),
                (probe(), StrategyId::Custom(7), ms(2_200)),
                (other(), StrategyId::LowPower, ms(5_550)),
            ],
            ..Planning::default()
        },
        replayed: false,
    }
}

/// A decision that names the probe and nothing more: lent host time only,
/// planned on `Balanced`, decided greedily with nothing binding, no margin,
/// no refusal, no correction.
fn bare_decision() -> DecisionView {
    DecisionView {
        epoch: Epoch(4),
        trigger: Trigger::Refresh,
        target: ms(16_670),
        why: Why {
            by: DecidedBy::Greedy,
            trigger: Trigger::Refresh,
            binding: None,
            rejected: Vec::new(),
        },
        envelopes: vec![(
            probe(),
            Envelope {
                time: PerDevice {
                    cpu: ms(2_500),
                    ..PerDevice::default()
                },
                memory: PerDevice::default(),
            },
        )],
        planned: vec![(probe(), StrategyId::Balanced)],
        corrections: Vec::new(),
        planning: Planning::default(),
        replayed: false,
    }
}

// ── Reading what was painted ──────────────────────────────────────────

/// One run of text painted in the inspector column.
#[derive(Debug, Clone, Copy)]
struct Painted<'a> {
    /// Its position in the recorded calls.
    index: usize,
    text: &'a str,
    pos: [f32; 2],
    color: [f32; 4],
    family: FontFamilyHint,
}

/// Where the inspector column starts, on a panel `width` wide.
fn inspector_x(width: f32) -> f32 {
    width - 8.0 - INSPECTOR_PANEL_WIDTH
}

/// Where the body's columns start, below the summary bar.
const BODY_TOP: f32 = 8.0 + 88.0 + 8.0;

/// Every run of text painted in the inspector column, in paint order — the
/// summary bar above it left out.
fn inspector_texts(calls: &[PaintCall], width: f32) -> Vec<Painted<'_>> {
    let left = inspector_x(width) - 0.5;
    calls
        .iter()
        .enumerate()
        .filter_map(|(index, call)| match call {
            PaintCall::Text {
                pos,
                text,
                color,
                family,
                ..
            } if pos[0] >= left && pos[1] >= BODY_TOP - 0.5 => Some(Painted {
                index,
                text: text.as_str(),
                pos: *pos,
                color: *color,
                family: *family,
            }),
            _ => None,
        })
        .collect()
}

/// The runs whose text is exactly `text`.
fn exactly<'a>(texts: &[Painted<'a>], text: &str) -> Vec<Painted<'a>> {
    texts.iter().filter(|t| t.text == text).copied().collect()
}

/// The first run whose text is exactly `text`.
fn the<'a>(texts: &[Painted<'a>], text: &str) -> Option<Painted<'a>> {
    exactly(texts, text).first().copied()
}

/// Whether `prefix` is `sentence`'s start, ending between two words.
fn starts_at_word(sentence: &str, prefix: &str) -> bool {
    !prefix.is_empty()
        && sentence.starts_with(prefix)
        && (sentence.len() == prefix.len() || sentence[prefix.len()..].starts_with(' '))
}

/// The first run of `sentence` as the inspector paints it, word-wrapped into
/// lines: a run that starts the sentence, then the runs after it in paint
/// order at the same x (within 0.5), each lower than the last, in the same
/// colour and family, whose texts joined by single spaces give exactly the
/// sentence. A sentence that fits is one run.
fn sentence<'a>(texts: &[Painted<'a>], sentence: &str) -> Option<Painted<'a>> {
    for (at, first) in texts.iter().enumerate() {
        if !starts_at_word(sentence, first.text) {
            continue;
        }
        let mut joined = first.text.to_owned();
        let mut last_y = first.pos[1];
        if joined == sentence {
            return Some(*first);
        }
        for next in &texts[at + 1..] {
            let same_column = (next.pos[0] - first.pos[0]).abs() <= 0.5
                && next.color == first.color
                && next.family == first.family
                && next.pos[1] > last_y;
            if !same_column {
                continue;
            }
            let longer = format!("{joined} {}", next.text);
            if !starts_at_word(sentence, &longer) {
                break;
            }
            joined = longer;
            last_y = next.pos[1];
            if joined == sentence {
                return Some(*first);
            }
        }
    }
    None
}

/// The first run whose text contains `needle`.
fn containing<'a>(texts: &[Painted<'a>], needle: &str) -> Option<Painted<'a>> {
    texts.iter().find(|t| t.text.contains(needle)).copied()
}

/// Every run on the same line as some run matching `anchor` — within 4 pt of
/// its height — the anchors themselves left out.
fn on_line_of<'a>(
    texts: &[Painted<'a>],
    anchor: impl Fn(&Painted<'a>) -> bool,
) -> Vec<Painted<'a>> {
    let anchors: Vec<Painted<'a>> = texts.iter().filter(|t| anchor(t)).copied().collect();
    texts
        .iter()
        .filter(|t| {
            anchors
                .iter()
                .any(|a| a.index != t.index && (a.pos[1] - t.pos[1]).abs() <= 4.0)
        })
        .copied()
        .collect()
}

/// The values on the line of the label exactly `label`.
fn values_of<'a>(texts: &[Painted<'a>], label: &str) -> Vec<Painted<'a>> {
    on_line_of(texts, |t| t.text == label)
}

/// The run exactly `value` on the line of the label `label`.
fn value_of<'a>(texts: &[Painted<'a>], label: &str, value: &str) -> Option<Painted<'a>> {
    values_of(texts, label)
        .into_iter()
        .find(|t| t.text == value)
}

/// The fills painted in the inspector column.
fn inspector_fills(calls: &[PaintCall], width: f32) -> Vec<[f32; 4]> {
    let left = inspector_x(width) - 0.5;
    calls
        .iter()
        .filter_map(|call| match call {
            PaintCall::RectFilled { min, color, .. }
                if min[0] >= left && min[1] >= BODY_TOP - 0.5 =>
            {
                Some(*color)
            }
            _ => None,
        })
        .collect()
}

/// Whether `color` is `of` made translucent: the same hue, a lower alpha.
fn tinted(color: [f32; 4], of: [f32; 4]) -> bool {
    color[..3] == of[..3] && color[3] < of[3]
}

fn listing(texts: &[Painted<'_>]) -> Vec<String> {
    texts
        .iter()
        .map(|t| format!("{:?} @ {:?}", t.text, t.pos))
        .collect()
}

// ── A live decision ───────────────────────────────────────────────────

/// **The inspector shows the selected agent's decision.** The Decision card,
/// tagged `live`, says what the probe was lent — its host and GPU time and
/// its memory over every device —, the row planned for it in gold (it is the
/// row it runs), the trigger that opened the round, that an exact search
/// decided it, what bound the frame and the risk margin kept. Every value is
/// in monospace.
#[test]
fn the_inspector_shows_the_selected_agents_decision() {
    let ui = shown(StrategyId::Balanced, decision());
    let texts = inspector_texts(ui.calls(), WIDE);
    let theme = theme();
    let all = listing(&texts);

    assert!(
        the(&texts, "Decision").is_some(),
        "the Decision card: {all:#?}"
    );
    let tag = the(&texts, "live").unwrap_or_else(|| panic!("the `live` tag: {all:#?}"));
    assert_eq!(
        tag.family,
        FontFamilyHint::Monospace,
        "the tag is monospace"
    );

    assert!(
        the(&texts, "Lent").is_some(),
        "the Lent sub-label: {all:#?}"
    );
    for (label, value) in [
        ("Host time", "4.25 ms"),
        ("GPU time", "3.10 ms"),
        ("Memory", "256 MiB"),
        ("Opened by", "Frame governor"),
        ("Decided by", "Exact search"),
        ("Frame bound by", "GPU time"),
        ("Risk margin kept", "0.85 ms"),
    ] {
        let shown = value_of(&texts, label, value)
            .unwrap_or_else(|| panic!("`{label}` shows `{value}`: {all:#?}"));
        if label != "Opened by" && label != "Decided by" && label != "Frame bound by" {
            assert_eq!(
                shown.family,
                FontFamilyHint::Monospace,
                "`{label}`'s value is monospace"
            );
        }
    }
    assert!(the(&texts, "Why").is_some(), "the Why sub-label: {all:#?}");

    let planned = value_of(&texts, "Planned row", "Balanced")
        .unwrap_or_else(|| panic!("`Planned row` shows `Balanced`: {all:#?}"));
    assert_eq!(
        planned.color, theme.accent_c,
        "the planned row is the running one: gold"
    );
    assert!(
        the(&texts, "Running row").is_none(),
        "one row only when the planned row is the one running"
    );
    assert!(
        !texts.iter().any(|t| t.text.starts_with("Still on")),
        "no swap pending"
    );
    assert!(
        !texts.iter().any(|t| t.text.contains("9.75 ms")),
        "another agent's envelope is not the probe's"
    );
}

/// **A refused upgrade is shown with what it broke.** Each upgrade the probe
/// was refused is an amber-tinted block: `Upgrade refused` in amber, the row
/// in monospace, and the constraint in words. Another agent's refusal is not
/// shown.
#[test]
fn the_inspector_shows_the_upgrades_the_agent_was_refused() {
    let ui = shown(StrategyId::Balanced, decision());
    let texts = inspector_texts(ui.calls(), WIDE);
    let theme = theme();
    let all = listing(&texts);

    let refused = exactly(&texts, "Upgrade refused");
    assert_eq!(refused.len(), 2, "the probe's two refusals: {all:#?}");
    for block in &refused {
        assert_eq!(block.color, theme.warning, "`Upgrade refused` is amber");
    }

    let mut found = Vec::new();
    for block in &refused {
        let at = texts
            .iter()
            .position(|t| t.index == block.index)
            .expect("the block is among the texts");
        let near = &texts[at + 1..(at + 4).min(texts.len())];
        for (row, broke) in [
            ("HighPerformance", "over the frame, GPU"),
            ("Custom(7)", "over its Stable ceiling, host"),
        ] {
            let names_row = near
                .iter()
                .any(|t| t.text == row && t.family == FontFamilyHint::Monospace);
            let says_why = near.iter().any(|t| t.text.contains(broke));
            if names_row && says_why {
                found.push(row);
            }
        }
    }
    found.sort_unstable();
    assert_eq!(
        found,
        vec!["Custom(7)", "HighPerformance"],
        "each refusal names its row (monospace) and what it broke: {all:#?}"
    );

    assert!(
        !texts.iter().any(|t| t.text.contains("over host memory")),
        "another agent's refusal is not the probe's"
    );
    assert!(
        inspector_fills(ui.calls(), WIDE)
            .iter()
            .any(|c| tinted(*c, theme.warning)),
        "a refusal sits on an amber tint"
    );
}

/// **The inspector shows what the DCC learnt of the agent's rows.** The card
/// `What the DCC learnt`, tagged `measured ÷ quoted`, lists each of the
/// probe's corrections in row order — the row's name, `×k` and `±σ` to two
/// places, and what a switch to it is charged (`—` when nothing). The running
/// row's line is gold, on a faint gold band, and says `running` in place of
/// its charge. Another agent's corrections and charges are not shown.
#[test]
fn the_inspector_shows_what_the_dcc_learnt_of_the_agents_rows() {
    let ui = shown(StrategyId::Balanced, decision());
    let texts = inspector_texts(ui.calls(), WIDE);
    let theme = theme();
    let all = listing(&texts);

    assert!(
        the(&texts, "What the DCC learnt").is_some(),
        "the learnt card: {all:#?}"
    );
    assert!(
        the(&texts, "measured ÷ quoted").is_some(),
        "its tag: {all:#?}"
    );
    assert!(
        texts.iter().any(|t| t.text.contains("×k")) && texts.iter().any(|t| t.text.contains("±σ")),
        "the header names the columns: {all:#?}"
    );

    // One line per correction, in the order the view gives them.
    let lines: Vec<(&str, &str, &str, Option<&str>)> = vec![
        ("LowPower", "0.91", "0.04", Some("1.75 ms")),
        ("Balanced", "2.47", "0.19", None),
        ("HighPerformance", "1.12", "0.33", Some("—")),
        ("Custom(7)", "3.05", "0.52", Some("2.20 ms")),
    ];
    let mut heights = Vec::new();
    for (row, factor, spread, charge) in &lines {
        let k = containing(&texts, factor)
            .unwrap_or_else(|| panic!("{row}'s factor {factor}: {all:#?}"));
        heights.push(k.pos[1]);
        let line = on_line_of(&texts, |t| t.index == k.index);
        assert!(
            line.iter().any(|t| t.text == *row),
            "{row}'s line names it: {line:#?}"
        );
        assert!(
            line.iter().any(|t| t.text.contains(spread)),
            "{row}'s line shows ±{spread}: {line:#?}"
        );
        match charge {
            Some(charge) => assert!(
                line.iter().any(|t| t.text == *charge),
                "{row}'s line shows its switch charge {charge}: {line:#?}"
            ),
            None => {
                assert!(
                    line.iter().any(|t| t.text == "running"),
                    "the running row's line says `running`: {line:#?}"
                );
                assert!(
                    line.iter()
                        .any(|t| t.text == *row && t.color == theme.accent_c),
                    "the running row's name is gold: {line:#?}"
                );
            }
        }
    }
    assert!(
        heights.windows(2).all(|w| w[0] < w[1]),
        "the corrections are listed in row order: {heights:?}"
    );
    assert!(
        !texts.iter().any(|t| t.text.contains("0.66 ms")),
        "the running row says `running` in place of its charge: {all:#?}"
    );
    for foreign in ["9.99", "8.88", "5.55 ms"] {
        assert!(
            !texts.iter().any(|t| t.text.contains(foreign)),
            "another agent's {foreign} is not the probe's: {all:#?}"
        );
    }
    assert!(
        sentence(
            &texts,
            "Switch: what a move to that row is charged — its preparation and the swap."
        )
        .is_some(),
        "the footnote: {all:#?}"
    );
    assert!(
        inspector_fills(ui.calls(), WIDE)
            .iter()
            .any(|c| tinted(*c, theme.accent_c)),
        "the running row sits on a faint gold band"
    );
}

/// **Gold means the running row, and nothing else.** Every gold text in the
/// inspector is on the line of the planned row (the running one here) or of
/// the learnt card's running row.
#[test]
fn only_the_running_row_is_gold() {
    let ui = shown(StrategyId::Balanced, decision());
    let texts = inspector_texts(ui.calls(), WIDE);
    let gold = theme().accent_c;

    let planned = values_of(&texts, "Planned row");
    let running = on_line_of(&texts, |t| t.text == "running");
    assert!(
        !running.is_empty(),
        "the learnt card's running row: {:#?}",
        listing(&texts)
    );
    for t in texts.iter().filter(|t| t.color == gold) {
        assert!(
            planned.iter().chain(&running).any(|r| r.index == t.index) || t.text == "running",
            "{:?} is gold but is not the running row",
            t.text
        );
    }
}

/// **The decision cards sit between Active Strategy and Health.** The cards
/// read, top to bottom: Execution Timing, Active Strategy, Decision, What the
/// DCC learnt, Health, Message.
#[test]
fn the_decision_cards_sit_between_active_strategy_and_health() {
    let ui = shown(StrategyId::Balanced, decision());
    let texts = inspector_texts(ui.calls(), WIDE);
    let all = listing(&texts);

    let order = [
        "Execution Timing",
        "Active Strategy",
        "Decision",
        "What the DCC learnt",
        "Health",
        "Message",
    ];
    let heights: Vec<f32> = order
        .iter()
        .map(|title| {
            the(&texts, title)
                .unwrap_or_else(|| panic!("the `{title}` card: {all:#?}"))
                .pos[1]
        })
        .collect();
    assert!(
        heights.windows(2).all(|w| w[0] < w[1]),
        "cards in the order {order:?}, got heights {heights:?}"
    );
}

// ── A swap pending, a replay ──────────────────────────────────────────

/// **A planned row not yet running is told apart from the one that runs.**
/// The probe still runs `LowPower` while the round planned
/// `HighPerformance`: `Planned row` is plain, `Running row` is gold, and an
/// amber note says the swap lands at the next frame start.
#[test]
fn a_planned_row_not_yet_running_is_shown_beside_the_running_one() {
    let mut view = decision();
    view.planned = vec![(probe(), StrategyId::HighPerformance)];
    let ui = shown(StrategyId::LowPower, view);
    let texts = inspector_texts(ui.calls(), WIDE);
    let theme = theme();
    let all = listing(&texts);

    let planned = value_of(&texts, "Planned row", "HighPerformance")
        .unwrap_or_else(|| panic!("`Planned row` shows `HighPerformance`: {all:#?}"));
    assert_ne!(
        planned.color, theme.accent_c,
        "a row planned but not running is not gold"
    );
    let running = value_of(&texts, "Running row", "LowPower")
        .unwrap_or_else(|| panic!("`Running row` shows `LowPower`: {all:#?}"));
    assert_eq!(running.color, theme.accent_c, "the running row is gold");

    let note = sentence(
        &texts,
        "Still on LowPower: the swap to HighPerformance lands at the next frame start.",
    )
    .unwrap_or_else(|| panic!("the swap note: {all:#?}"));
    assert_eq!(note.color, theme.warning, "the swap note is amber");
}

/// **An agent lent an envelope but planned no row says it kept what it
/// held.** The view lends the probe its envelope (it was late, or `Stable`)
/// but plans no row for it: `Running row` names the running row in gold, a
/// muted line says the envelope was kept — no `Planned row`, no swap note.
#[test]
fn an_agent_planned_no_row_says_it_kept_its_envelope() {
    let mut view = decision();
    view.planned.retain(|(agent, _)| *agent != probe());
    let ui = shown(StrategyId::LowPower, view);
    let texts = inspector_texts(ui.calls(), WIDE);
    let theme = theme();
    let all = listing(&texts);

    let running = value_of(&texts, "Running row", "LowPower")
        .unwrap_or_else(|| panic!("`Running row` shows `LowPower`: {all:#?}"));
    assert_eq!(running.color, theme.accent_c, "the running row is gold");
    let kept = sentence(&texts, "Kept the envelope it held: no row planned.")
        .unwrap_or_else(|| panic!("the kept-envelope line: {all:#?}"));
    assert_eq!(
        kept.color, theme.text_muted,
        "the kept-envelope line is muted"
    );
    assert!(
        the(&texts, "Planned row").is_none(),
        "no row was planned: {all:#?}"
    );
    assert!(
        !texts.iter().any(|t| t.text.starts_with("Still on")),
        "no swap pending: {all:#?}"
    );
    assert!(
        value_of(&texts, "Host time", "4.25 ms").is_some(),
        "the envelope it holds is still shown: {all:#?}"
    );
}

/// **A replayed decision says it was read back.** Its tag is
/// `replayed · journal` in silver, not `live`, and a line under it says it
/// was not decided by this run.
#[test]
fn a_replayed_decision_says_it_was_read_back_from_a_journal() {
    let mut view = decision();
    view.replayed = true;
    let ui = shown(StrategyId::Balanced, view);
    let texts = inspector_texts(ui.calls(), WIDE);
    let theme = theme();
    let all = listing(&texts);

    let tag =
        the(&texts, "replayed · journal").unwrap_or_else(|| panic!("the replayed tag: {all:#?}"));
    assert_eq!(tag.color, theme.primary, "the replayed tag is silver");
    assert!(the(&texts, "live").is_none(), "a replay is not live");
    assert!(
        sentence(
            &texts,
            "Read back from a decision journal, not decided by this run."
        )
        .is_some(),
        "the replay line: {all:#?}"
    );
}

// ── No decision for the agent ─────────────────────────────────────────

/// What the Decision card says before any round.
const NOT_LENT_YET: &str = "The DCC has not lent this agent anything yet.";
const WHAT_A_ROUND_IS: &str = "It runs its default row until the first round. A round opens when \
                               the frame governor sees pressure, when a device or the agent \
                               itself asks, or every few seconds to re-plan.";

/// Asserts the frame shows the empty Decision card that teaches.
fn teaches(ui: &RecordingUiBuilder, case: &str) {
    let texts = inspector_texts(ui.calls(), WIDE);
    let theme = theme();
    let all = listing(&texts);

    let decision = the(&texts, "Decision")
        .unwrap_or_else(|| panic!("{case}: the Decision card: {all:#?}"))
        .pos[1];
    let strategy = the(&texts, "Active Strategy")
        .unwrap_or_else(|| panic!("{case}: Active Strategy: {all:#?}"))
        .pos[1];
    let health = the(&texts, "Health")
        .unwrap_or_else(|| panic!("{case}: Health: {all:#?}"))
        .pos[1];
    assert!(
        strategy < decision && decision < health,
        "{case}: the Decision card sits between Active Strategy and Health"
    );

    let tag = the(&texts, "none yet").unwrap_or_else(|| panic!("{case}: the tag: {all:#?}"));
    assert_eq!(
        tag.family,
        FontFamilyHint::Monospace,
        "{case}: the tag is monospace"
    );
    assert!(
        sentence(&texts, NOT_LENT_YET).is_some(),
        "{case}: says nothing was lent: {all:#?}"
    );
    let round = sentence(&texts, WHAT_A_ROUND_IS)
        .unwrap_or_else(|| panic!("{case}: says what a round is: {all:#?}"));
    assert_eq!(round.color, theme.text_dim, "{case}: the paragraph is dim");

    assert!(
        the(&texts, "What the DCC learnt").is_none(),
        "{case}: nothing learnt without a decision"
    );
    for absent in ["live", "Host time", "Planned row", "Opened by"] {
        assert!(
            the(&texts, absent).is_none(),
            "{case}: no `{absent}` without a decision"
        );
    }
}

/// **The inspector teaches when no decision ran.** Before the DCC has
/// published a round — and in an editor given no watch at all — the
/// Decision card is tagged `none yet`, says nothing was lent, and says when a
/// round opens. No learnt card.
#[test]
fn the_inspector_teaches_when_no_decision_ran() {
    let ui = frame(
        &mut panel(StrategyId::Balanced, Some(DecisionWatch::default())),
        WIDE,
        TALL,
    );
    teaches(&ui, "an empty watch");

    let ui = frame(&mut panel(StrategyId::Balanced, None), WIDE, TALL);
    teaches(&ui, "no watch");
}

/// **A decision that lent the agent nothing says so.** The view names only
/// another agent: the card keeps its tag and says `Not lent in epoch 12.` in
/// muted text; no row, no learnt card.
#[test]
fn a_decision_that_did_not_lend_the_agent_says_so() {
    let mut view = decision();
    view.epoch = Epoch(12);
    view.envelopes.retain(|(agent, _)| *agent != probe());
    view.planned.retain(|(agent, _)| *agent != probe());
    let ui = shown(StrategyId::Balanced, view);
    let texts = inspector_texts(ui.calls(), WIDE);
    let theme = theme();
    let all = listing(&texts);

    assert!(the(&texts, "live").is_some(), "the tag stays: {all:#?}");
    let line = sentence(&texts, "Not lent in epoch 12.")
        .unwrap_or_else(|| panic!("the not-lent line: {all:#?}"));
    assert_eq!(line.color, theme.text_muted, "the not-lent line is muted");
    assert!(
        the(&texts, "What the DCC learnt").is_none(),
        "no learnt card for an agent not lent"
    );
    assert!(
        the(&texts, "Planned row").is_none() && the(&texts, "Host time").is_none(),
        "nothing lent, nothing planned: {all:#?}"
    );
}

// ── What exists, and only that ────────────────────────────────────────

/// **A line is shown only when it has something to say.** Nothing bound the
/// frame, no margin was kept, no GPU time and no memory were lent, no
/// upgrade was refused: no `Frame bound by`, `Risk margin kept`, `GPU time`,
/// `Memory` nor `Upgrade refused` — while the host time and who decided are.
#[test]
fn absent_reasons_and_zero_amounts_are_not_shown() {
    let ui = shown(StrategyId::Balanced, bare_decision());
    let texts = inspector_texts(ui.calls(), WIDE);
    let all = listing(&texts);

    assert!(
        value_of(&texts, "Host time", "2.50 ms").is_some(),
        "the host time lent: {all:#?}"
    );
    assert!(
        value_of(&texts, "Decided by", "Greedy").is_some(),
        "who decided: {all:#?}"
    );
    assert!(
        value_of(&texts, "Opened by", "Refresh").is_some(),
        "what opened the round: {all:#?}"
    );
    for absent in [
        "Frame bound by",
        "Risk margin kept",
        "GPU time",
        "Memory",
        "Upgrade refused",
    ] {
        assert!(
            the(&texts, absent).is_none(),
            "`{absent}` has nothing to say here: {all:#?}"
        );
    }
}

/// **Memory lent under a MiB is not shown as nothing.** The grid overlay
/// quotes its camera block — a few hundred bytes — and is lent it: the
/// `Memory` line is shown (something was lent) and does not read `0 MiB`,
/// which says the opposite.
#[test]
fn memory_lent_under_a_mib_is_not_shown_as_zero() {
    let mut view = bare_decision();
    view.envelopes[0].1.memory.gpu = 368;
    let ui = shown(StrategyId::Balanced, view);
    let texts = inspector_texts(ui.calls(), WIDE);
    let all = listing(&texts);

    let values = values_of(&texts, "Memory");
    let value = values
        .iter()
        .find(|t| t.text.contains("iB") || t.text.ends_with(" B"))
        .unwrap_or_else(|| panic!("a `Memory` line for 368 bytes lent: {all:#?}"));
    let digits: String = value.text.chars().filter(|c| c.is_ascii_digit()).collect();
    assert!(
        value.text.contains('<') || digits.chars().any(|c| c != '0'),
        "368 bytes lent reads as a non-zero amount, got {:?}",
        value.text
    );
}

/// **An agent with no row measured says so.** The learnt card is there — the
/// decision names the probe — and says `No row measured yet.` in muted text.
#[test]
fn the_learnt_card_says_when_no_row_was_measured() {
    let ui = shown(StrategyId::Balanced, bare_decision());
    let texts = inspector_texts(ui.calls(), WIDE);
    let all = listing(&texts);

    assert!(
        the(&texts, "What the DCC learnt").is_some(),
        "the learnt card: {all:#?}"
    );
    let line = sentence(&texts, "No row measured yet.")
        .unwrap_or_else(|| panic!("the unmeasured line: {all:#?}"));
    assert_eq!(
        line.color,
        theme().text_muted,
        "the unmeasured line is muted"
    );
}

/// **Every trigger, decider and constraint is named in words.** Each
/// `Trigger` opens the round under its name, each `DecidedBy` decides it
/// under its name, each `Binding` bounds the frame under its name and, on a
/// refused upgrade, says what it broke.
#[test]
fn every_trigger_decider_and_constraint_is_named_in_words() {
    for (trigger, name) in [
        (Trigger::Initial, "Initial"),
        (Trigger::Pressure, "Pressure"),
        (Trigger::ForecastBreach, "Forecast breach"),
        (Trigger::FrameGovernor, "Frame governor"),
        (Trigger::AgentsChanged, "Agents changed"),
        (Trigger::PolicyChanged, "Policy changed"),
        (Trigger::AgentStalled, "Agent stalled"),
        (Trigger::Overruns, "Overruns"),
        (Trigger::Refresh, "Refresh"),
        (Trigger::Safety, "Safety"),
    ] {
        let mut view = bare_decision();
        view.trigger = trigger;
        view.why.trigger = trigger;
        let ui = shown(StrategyId::Balanced, view);
        let texts = inspector_texts(ui.calls(), WIDE);
        assert!(
            value_of(&texts, "Opened by", name).is_some(),
            "{trigger:?} opens as `{name}`: {:#?}",
            listing(&texts)
        );
    }

    for (by, name) in [
        (DecidedBy::Floors, "Floors only"),
        (DecidedBy::Greedy, "Greedy"),
        (DecidedBy::Exact, "Exact search"),
        (DecidedBy::Planner, "Planner"),
    ] {
        let mut view = bare_decision();
        view.why.by = by;
        let ui = shown(StrategyId::Balanced, view);
        let texts = inspector_texts(ui.calls(), WIDE);
        assert!(
            value_of(&texts, "Decided by", name).is_some(),
            "{by:?} decides as `{name}`: {:#?}",
            listing(&texts)
        );
    }

    for (binding, bound, broke) in [
        (
            Binding::Time(Device::Cpu),
            "Host time",
            "over the frame, host",
        ),
        (
            Binding::Time(Device::Gpu),
            "GPU time",
            "over the frame, GPU",
        ),
        (
            Binding::Memory(Device::Cpu),
            "Host memory",
            "over host memory",
        ),
        (
            Binding::Memory(Device::Gpu),
            "GPU memory",
            "over GPU memory",
        ),
        (
            Binding::Ceiling(Device::Cpu),
            "Stable ceiling, host",
            "over its Stable ceiling, host",
        ),
        (
            Binding::Ceiling(Device::Gpu),
            "Stable ceiling, GPU",
            "over its Stable ceiling, GPU",
        ),
    ] {
        let mut view = bare_decision();
        view.why.binding = Some(binding);
        view.why.rejected = vec![Rejected {
            agent: probe(),
            row: StrategyId::HighPerformance,
            over: binding,
        }];
        let ui = shown(StrategyId::Balanced, view);
        let texts = inspector_texts(ui.calls(), WIDE);
        let all = listing(&texts);
        assert!(
            value_of(&texts, "Frame bound by", bound).is_some(),
            "{binding:?} bounds the frame as `{bound}`: {all:#?}"
        );
        assert!(
            texts.iter().any(|t| t.text.contains(broke)),
            "a refusal over {binding:?} says `{broke}`: {all:#?}"
        );
    }
}

// ── Scrolling ─────────────────────────────────────────────────────────

/// A clip pushed or popped, at a position in the recorded calls.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Clip {
    /// `push_clip_rect(rect)`, made before the call at `at`.
    Push { at: usize, rect: [f32; 4] },
    /// `pop_clip_rect()`, made before the call at `at`.
    Pop { at: usize },
}

/// A [`RecordingUiBuilder`] that also records the clips pushed and popped,
/// which the recorder itself lets through.
struct ClipProbe {
    inner: RecordingUiBuilder,
    clips: Vec<Clip>,
}

impl ClipProbe {
    fn new(inner: RecordingUiBuilder) -> Self {
        Self {
            inner,
            clips: Vec::new(),
        }
    }
}

impl UiBuilder for ClipProbe {
    fn push_clip_rect(&mut self, rect: [f32; 4]) {
        self.clips.push(Clip::Push {
            at: self.inner.calls().len(),
            rect,
        });
    }

    fn pop_clip_rect(&mut self) {
        self.clips.push(Clip::Pop {
            at: self.inner.calls().len(),
        });
    }

    fn heading(&mut self, text: &str) {
        self.inner.heading(text);
    }
    fn label(&mut self, text: &str) {
        self.inner.label(text);
    }
    fn colored_label(&mut self, color: [f32; 4], text: &str) {
        self.inner.colored_label(color, text);
    }
    fn small_label(&mut self, text: &str) {
        self.inner.small_label(text);
    }
    fn monospace(&mut self, text: &str) {
        self.inner.monospace(text);
    }
    fn button(&mut self, text: &str) -> bool {
        self.inner.button(text)
    }
    fn small_button(&mut self, text: &str) -> bool {
        self.inner.small_button(text)
    }
    fn selectable_label(&mut self, active: bool, text: &str) -> bool {
        self.inner.selectable_label(active, text)
    }
    fn selectable_label_double_clicked(&mut self, active: bool, text: &str) -> bool {
        self.inner.selectable_label_double_clicked(active, text)
    }
    fn checkbox(&mut self, checked: &mut bool, text: &str) -> bool {
        self.inner.checkbox(checked, text)
    }
    fn drag_value_f32(&mut self, label: &str, value: &mut f32, speed: f32) -> bool {
        self.inner.drag_value_f32(label, value, speed)
    }
    fn slider_f32(&mut self, label: &str, value: &mut f32, min: f32, max: f32) -> bool {
        self.inner.slider_f32(label, value, min, max)
    }
    fn text_edit_singleline(&mut self, text: &mut String) -> bool {
        self.inner.text_edit_singleline(text)
    }
    fn vec3_editor(&mut self, label: &str, value: &mut [f32; 3], speed: f32) -> bool {
        self.inner.vec3_editor(label, value, speed)
    }
    fn color_edit(&mut self, label: &str, color: &mut [f32; 4]) -> bool {
        self.inner.color_edit(label, color)
    }
    fn combo_box(
        &mut self,
        id_salt: &str,
        label: &str,
        current: &mut usize,
        options: &[&str],
    ) -> bool {
        self.inner.combo_box(id_salt, label, current, options)
    }
    fn horizontal(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn vertical(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn collapsing(
        &mut self,
        _header: &str,
        _default_open: bool,
        f: &mut dyn FnMut(&mut dyn UiBuilder),
    ) {
        f(self);
    }
    fn indent(&mut self, _id: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn scroll_area(&mut self, _id: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        f(self);
    }
    fn region_at(
        &mut self,
        _id_salt: &str,
        _rect: [f32; 4],
        f: &mut dyn FnMut(&mut dyn UiBuilder),
    ) {
        f(self);
    }
    fn separator(&mut self) {
        self.inner.separator();
    }
    fn spacing(&mut self, points: f32) {
        self.inner.spacing(points);
    }
    fn is_last_item_double_clicked(&self) -> bool {
        self.inner.is_last_item_double_clicked()
    }
    fn is_last_item_hovered(&self) -> bool {
        self.inner.is_last_item_hovered()
    }
    fn is_last_item_enter_pressed(&self) -> bool {
        self.inner.is_last_item_enter_pressed()
    }
    fn is_last_item_escape_pressed(&self) -> bool {
        self.inner.is_last_item_escape_pressed()
    }
    fn key_pressed(&self, key: KeyCode) -> bool {
        self.inner.key_pressed(key)
    }
    fn raw_key_pressed(&self, key: KeyCode) -> bool {
        self.inner.raw_key_pressed(key)
    }
    fn scroll_delta_in(&self, rect: [f32; 4]) -> f32 {
        self.inner.scroll_delta_in(rect)
    }
    fn pointer_position(&self) -> Option<[f32; 2]> {
        self.inner.pointer_position()
    }
    fn context_menu_last(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        self.inner.context_menu_last(f);
    }
    fn paint_line(&mut self, from: [f32; 2], to: [f32; 2], color: [f32; 4], thickness: f32) {
        self.inner.paint_line(from, to, color, thickness);
    }
    fn paint_rect_filled(&mut self, min: [f32; 2], size: [f32; 2], color: [f32; 4], rounding: f32) {
        self.inner.paint_rect_filled(min, size, color, rounding);
    }
    fn paint_text(&mut self, pos: [f32; 2], color: [f32; 4], text: &str) {
        self.inner.paint_text(pos, color, text);
    }
    fn paint_rect_stroke(
        &mut self,
        min: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
        rounding: f32,
        thickness: f32,
    ) {
        self.inner
            .paint_rect_stroke(min, size, color, rounding, thickness);
    }
    fn paint_circle_filled(&mut self, center: [f32; 2], radius: f32, color: [f32; 4]) {
        self.inner.paint_circle_filled(center, radius, color);
    }
    fn paint_circle_stroke(
        &mut self,
        center: [f32; 2],
        radius: f32,
        color: [f32; 4],
        thickness: f32,
    ) {
        self.inner
            .paint_circle_stroke(center, radius, color, thickness);
    }
    fn paint_text_styled(
        &mut self,
        pos: [f32; 2],
        text: &str,
        size: f32,
        color: [f32; 4],
        family: FontFamilyHint,
        align: TextAlign,
    ) {
        self.inner
            .paint_text_styled(pos, text, size, color, family, align);
    }
    fn paint_path_filled(&mut self, points: &[[f32; 2]], color: [f32; 4]) {
        self.inner.paint_path_filled(points, color);
    }
    fn paint_path_filled_gradient(&mut self, points: &[[f32; 2]], gradient: LinearGradient<'_>) {
        self.inner.paint_path_filled_gradient(points, gradient);
    }
    fn paint_path_filled_radial(&mut self, points: &[[f32; 2]], gradient: RadialGradient<'_>) {
        self.inner.paint_path_filled_radial(points, gradient);
    }
    fn interact_rect(&mut self, id_salt: &str, rect: [f32; 4]) -> Interaction {
        self.inner.interact_rect(id_salt, rect)
    }
    fn cursor_pos(&self) -> [f32; 2] {
        self.inner.cursor_pos()
    }
    fn allocate_size(&mut self, size: [f32; 2]) -> [f32; 4] {
        self.inner.allocate_size(size)
    }
    fn measure_text(&self, text: &str, size: f32, family: FontFamilyHint) -> [f32; 2] {
        self.inner.measure_text(text, size, family)
    }
    fn available_width(&self) -> f32 {
        self.inner.available_width()
    }
    fn available_height(&self) -> f32 {
        self.inner.available_height()
    }
    fn panel_rect(&self) -> [f32; 4] {
        self.inner.panel_rect()
    }
    fn screen_rect(&self) -> [f32; 4] {
        self.inner.screen_rect()
    }
    fn pixels_per_point(&self) -> f32 {
        self.inner.pixels_per_point()
    }
    fn viewport_image(
        &mut self,
        handle: ViewportTextureHandle,
        size: [f32; 2],
    ) -> Option<[f32; 2]> {
        self.inner.viewport_image(handle, size)
    }
}

/// One frame of `panel` over a `width` × `height` panel, the wheel at
/// `scroll`, its clips recorded.
fn clipped_frame(panel: &mut ControlPlanePanel, width: f32, height: f32, scroll: f32) -> ClipProbe {
    let mut ui = ClipProbe::new(RecordingUiBuilder::new(width, height).with_scroll(scroll));
    panel.ui(&mut ui);
    ui
}

/// The clip the inspector pushed — the one inside its column — and the
/// range of calls `[push, pop)` it covers.
fn inspector_clip(probe: &ClipProbe, width: f32) -> Option<([f32; 4], usize, usize)> {
    let left = inspector_x(width) - 0.5;
    let right = inspector_x(width) + INSPECTOR_PANEL_WIDTH + 0.5;
    let mut depth_at_push = None;
    let mut depth = 0usize;
    let mut found = None;
    for clip in &probe.clips {
        match *clip {
            Clip::Push { at, rect } => {
                depth += 1;
                if found.is_none()
                    && depth_at_push.is_none()
                    && rect[0] >= left
                    && rect[0] + rect[2] <= right
                {
                    depth_at_push = Some(depth);
                    found = Some((rect, at, usize::MAX));
                }
            }
            Clip::Pop { at } => {
                if depth_at_push == Some(depth) {
                    if let Some((rect, push, _)) = found {
                        found = Some((rect, push, at));
                    }
                    depth_at_push = None;
                }
                depth = depth.saturating_sub(1);
            }
        }
    }
    found
}

/// **The inspector scrolls when its cards overflow.** On a short panel the
/// cards below the header's divider are painted inside a clip that starts
/// below the header and ends within the panel, the scroll bar
/// `cp-inspector-scroll` is laid out, and the wheel moves the cards up while
/// the header — the agent's name and its status pill — stays where it was.
/// On a panel tall enough for every card, there is no bar.
#[test]
fn the_inspector_scrolls_when_its_cards_overflow() {
    let mut short = panel(StrategyId::Balanced, Some(watching(decision())));

    let still = clipped_frame(&mut short, WIDE, SHORT, 0.0);
    let texts = inspector_texts(still.inner.calls(), WIDE);
    let all = listing(&texts);
    let name = the(&texts, "probe").unwrap_or_else(|| panic!("the agent's name: {all:#?}"));
    let pill = the(&texts, "healthy").unwrap_or_else(|| panic!("the status pill: {all:#?}"));

    let (clip, push, pop) = inspector_clip(&still, WIDE)
        .unwrap_or_else(|| panic!("a clip in the inspector column: {:?}", still.clips));
    assert!(
        clip[1] > pill.pos[1],
        "the clip starts below the header: clip {clip:?}, pill at {:?}",
        pill.pos
    );
    assert!(
        clip[1] + clip[3] <= SHORT + 0.5,
        "the clip ends within the panel: {clip:?}"
    );
    assert!(
        name.index < push && pill.index < push,
        "the header is painted outside the clip"
    );
    for card in ["Execution Timing", "Active Strategy", "Decision"] {
        let title = the(&texts, card).unwrap_or_else(|| panic!("the `{card}` card: {all:#?}"));
        assert!(
            push <= title.index && title.index < pop,
            "the `{card}` card is painted inside the clip"
        );
    }
    assert!(
        still.inner.interact_ids().contains(&"cp-inspector-scroll"),
        "the scroll bar is laid out: {:?}",
        still.inner.interact_ids()
    );

    let scrolled = clipped_frame(&mut short, WIDE, SHORT, -80.0);
    let moved = inspector_texts(scrolled.inner.calls(), WIDE);
    let all_moved = listing(&moved);
    let timing = |texts: &[Painted<'_>], all: &[String]| {
        the(texts, "Execution Timing")
            .unwrap_or_else(|| panic!("Execution Timing: {all:#?}"))
            .pos[1]
    };
    assert!(
        timing(&moved, &all_moved) < timing(&texts, &all) - 1.0,
        "the wheel moves the cards up"
    );
    let name_after = the(&moved, "probe").unwrap_or_else(|| panic!("the name: {all_moved:#?}"));
    let pill_after = the(&moved, "healthy").unwrap_or_else(|| panic!("the pill: {all_moved:#?}"));
    assert_eq!(name_after.pos, name.pos, "the agent's name stays");
    assert_eq!(pill_after.pos, pill.pos, "the status pill stays");

    let mut tall = panel(StrategyId::Balanced, Some(watching(decision())));
    let roomy = clipped_frame(&mut tall, WIDE, VERY_TALL, 0.0);
    assert!(
        !roomy.inner.interact_ids().contains(&"cp-inspector-scroll"),
        "no bar when every card fits"
    );
}

/// **Cards that shrink leave no blank inspector.** Scrolled to the end of a
/// long decision, the next round no longer lends the probe anything: its
/// cards are much shorter. On the very first frame that shows them the last
/// card — the Message — is inside the clip, not scrolled away above it with
/// blank space below.
#[test]
fn cards_that_shrink_stay_in_view_on_the_frame_they_shrink() {
    let watch = watching(decision());
    let mut short = panel(StrategyId::Balanced, Some(watch.clone()));
    let _ = clipped_frame(&mut short, WIDE, SHORT, 0.0);
    let _ = clipped_frame(&mut short, WIDE, SHORT, -10_000.0);

    let mut not_lent = decision();
    not_lent.envelopes.retain(|(agent, _)| *agent != probe());
    not_lent.planned.retain(|(agent, _)| *agent != probe());
    watch.publish(not_lent);

    let shrunk = clipped_frame(&mut short, WIDE, SHORT, 0.0);
    let (clip, _, _) = inspector_clip(&shrunk, WIDE)
        .unwrap_or_else(|| panic!("a clip in the inspector column: {:?}", shrunk.clips));
    let message_y = shrunk
        .inner
        .calls()
        .iter()
        .find_map(|call| match call {
            PaintCall::Text { pos, text, .. } if text == SAID => Some(pos[1]),
            _ => None,
        })
        .expect("the Message card's text is painted");
    assert!(
        message_y >= clip[1] && message_y <= clip[1] + clip[3],
        "the last card is in view on the frame the cards shrink: \
         message at y {message_y}, clip {clip:?}"
    );
}

/// **Memory lent while a device's memory was not known reads as not
/// limited.** A round that did not know a device's memory lends `u64::MAX`
/// there: the envelope bounds nothing on that device. The `Memory` line says
/// `not limited`, never a sum of `u64::MAX` bytes.
#[test]
fn memory_lent_while_unknown_reads_not_limited() {
    let mut view = bare_decision();
    view.envelopes[0].1.memory.cpu = 64 * MIB;
    view.envelopes[0].1.memory.gpu = u64::MAX;
    let ui = shown(StrategyId::Balanced, view);
    let texts = inspector_texts(ui.calls(), WIDE);
    let all = listing(&texts);

    assert!(
        value_of(&texts, "Memory", "not limited").is_some(),
        "an unknown device's memory bounds nothing: {all:#?}"
    );
    assert!(
        !texts.iter().any(|t| t.text.contains("17592186044416")),
        "no sum of u64::MAX bytes: {all:#?}"
    );
}
