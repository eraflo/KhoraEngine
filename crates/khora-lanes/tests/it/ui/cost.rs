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

//! What the UI lane quotes: a time per node and a time per text, nothing
//! for a frame with neither, and the memory its level holds.

use std::time::Duration;

use khora_core::lane::cost::Cost;

use super::fixtures::{Scene, Ui};

/// The cost the run quoted for the one level.
fn quoted(ui: &mut Ui, scene: &Scene) -> Cost {
    let recorded = ui.record(scene);
    recorded.recorded();
    assert_eq!(recorded.run.costs.len(), 1, "{:?}", recorded.run.costs);
    recorded.run.costs[0]
}

/// The time `cost` quotes, on every device.
fn time(cost: &Cost) -> Duration {
    cost.time.cpu + cost.time.gpu + cost.time.npu + cost.time.other
}

/// Asserts `costs` grow, each step adding the same time.
fn assert_equal_steps(what: &str, costs: &[Duration]) {
    assert!(
        costs[0] < costs[1] && costs[1] < costs[2],
        "more {what}, more time: {costs:?}"
    );
    let first_step = (costs[1] - costs[0]).as_secs_f64();
    let second_step = (costs[2] - costs[1]).as_secs_f64();
    assert!(
        (first_step - second_step).abs() <= first_step * 0.01,
        "each of the {what} adds the same time: {costs:?}"
    );
}

/// **The UI costs a time per node.** One, two and three hundred nodes, no
/// text, cost more each time, by the same step.
#[test]
fn the_ui_costs_a_time_per_node() {
    let mut ui = Ui::new();
    let costs: Vec<Duration> = [100, 200, 300]
        .into_iter()
        .map(|count| {
            time(&quoted(
                &mut ui,
                &Scene::new().with_text_count(0).with_node_count(count),
            ))
        })
        .collect();

    assert_equal_steps("nodes", &costs);
}

/// **The UI costs a time per text.** One, two and three hundred texts, no
/// node, cost more each time, by the same step.
#[test]
fn the_ui_costs_a_time_per_text() {
    let mut ui = Ui::new();
    let costs: Vec<Duration> = [100, 200, 300]
        .into_iter()
        .map(|count| {
            time(&quoted(
                &mut ui,
                &Scene::new().with_node_count(0).with_text_count(count),
            ))
        })
        .collect();

    assert_equal_steps("texts", &costs);
}

/// **An empty UI costs no time**, and its cost still quotes the memory its
/// level holds.
#[test]
fn an_empty_ui_costs_no_time() {
    let mut ui = Ui::new();

    let cost = quoted(&mut ui, &Scene::new().empty());

    assert_eq!(time(&cost), Duration::ZERO, "{cost:?}");
    assert_eq!(cost.memory, ui.lane().levels()[0].memory, "{cost:?}");
}
