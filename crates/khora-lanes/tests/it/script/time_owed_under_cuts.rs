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

//! **No game time is lost:**
//! an instance's next turn is handed all the time since its last: every
//! countdown of an instance — the
//! wait of its pending body, each of its timers — and its `Update` receive
//! every elapsed second exactly once, possibly late, never dropped, never
//! twice. A turn that stops before reaching one of them — a resumed body cut
//! again, a hook cut in front of a restored body — keeps that consumer's time
//! owed; a save and a hot reload carry it.
//!
//! The frames below are binary fractions of a second (`k/256`, `1/64`), so
//! every sum is exact in `f32` and a single dropped frame shows as an
//! inequality, not as a rounding error.

use khora_core::script::{EventQueue, ScriptEvent};
use khora_data::flow::ScriptView;
use khora_lanes::script::{Body, ScriptRunReport, ScriptRuntime};

use super::saves::{loaded, through_record};
use super::scheduled_scene::{
    build, entity, float_of, frame, frame_with, int_of, runtime_of, view, PLENTY,
};

const MODULE: &str = "owed.erg";

/// A frame of 1/64 s.
const DELTA: f32 = 1.0 / 64.0;

/// The `n`th frame's delta: seven different binary fractions, so the frame a
/// turn drops is a frame whose delta can be told from its neighbours'.
fn delta_of(n: usize) -> f32 {
    (1 + n % 7) as f32 / 256.0
}

/// A slice of one or two units: every `Update` it reaches is cut.
fn trickle(n: usize) -> u64 {
    1 + (n % 2) as u64
}

/// `Clock`: an `Update` long enough for a slice of one or two units to cut it
/// many times, summing the `dt` each call that completes was handed.
const CLOCK: &str = "behavior Clock {
    float total = 0.0;
    int calls = 0;
    void Update(float dt) {
        int i = 0;
        while (i < 20) { i += 1; }
        total += dt;
        calls += 1;
    }
}";

/// One frame of `delta` with `fuel`.
fn frame_of(
    view: &mut ScriptView,
    runtime: &mut ScriptRuntime,
    delta: f32,
    fuel: u64,
) -> ScriptRunReport {
    view.delta_seconds = delta;
    frame(view, runtime, fuel)
}

/// Whether `behavior` on the entity at `index` ended the frame part-way
/// through `body`.
fn part_way(runtime: &ScriptRuntime, index: u32, behavior: &str, body: &Body) -> bool {
    runtime
        .peek(entity(index), behavior)
        .and_then(|held| held.pending.as_ref())
        .is_some_and(|pending| &pending.body == body)
}

/// Runs `Clock` one frame with plenty, then `cut` frames of a trickle; the
/// reports and the time that passed, frame by frame.
fn clock_under_a_trickle(
    view: &mut ScriptView,
    runtime: &mut ScriptRuntime,
    cut: usize,
) -> (Vec<ScriptRunReport>, Vec<f32>) {
    let mut reports = vec![frame_of(view, runtime, delta_of(0), PLENTY)];
    let mut deltas = vec![delta_of(0)];
    let mut cut_turns = 0;
    for n in 1..=cut {
        let report = frame_of(view, runtime, delta_of(n), trickle(n));
        // A turn that ran and stopped part-way: the path that used to drop
        // the frame's delta.
        if report.spent > 0 && part_way(runtime, 0, "Clock", &Body::Update) {
            cut_turns += 1;
        }
        reports.push(report);
        deltas.push(delta_of(n));
    }
    assert!(
        cut_turns >= 5,
        "set-up: the trickle cut `Update` in {cut_turns} turns"
    );
    (reports, deltas)
}

/// **An `Update` cut every frame is handed every second once the fuel
/// returns.** `Clock`'s `Update` is cut by a slice of one or two units for
/// sixty-four frames of different lengths, then runs with plenty: the `dt`
/// its completed calls received add up to the time that passed — not one
/// frame short.
#[test]
fn an_update_cut_every_frame_is_handed_every_second_once_the_fuel_returns() {
    let mut view = view(MODULE, &[("Clock", &[0])], 0.0);
    let mut runtime = runtime_of(MODULE, CLOCK);
    let (_, mut deltas) = clock_under_a_trickle(&mut view, &mut runtime, 64);
    for n in 65..69 {
        frame_of(&mut view, &mut runtime, delta_of(n), PLENTY);
        deltas.push(delta_of(n));
    }

    let elapsed: f32 = deltas.iter().sum();
    assert!(
        !part_way(&runtime, 0, "Clock", &Body::Update),
        "set-up: with plenty, the cut `Update` finished"
    );
    assert_eq!(
        float_of(&runtime, MODULE, 0, "Clock", "total"),
        elapsed,
        "{} completed `Update`s were handed every second of the {} frames",
        int_of(&runtime, MODULE, 0, "Clock", "calls"),
        deltas.len()
    );
}

/// `Ticker`: a timer counting its firings, and a handler long enough for a
/// slice of thirty units to cut it for many frames.
const TICKER: &str = "behavior Ticker {
    int fired = 0;
    int worked = 0;
    every 0.125s { fired += 1; }
    on Work() { int i = 0; while (i < 2000) { i += 1; } worked += 1; }
}
event Work;";

/// `Ticker` over 129 frames of 1/64 s, hearing `Work` from the second: the
/// first sixty-four after it with `fuel(n)`, the rest with plenty. Its
/// firings, and how many frames ended with the handler part-way.
fn ticker_with(fuel: impl Fn(usize) -> u64) -> (i64, usize) {
    let view = view(MODULE, &[("Ticker", &[0])], DELTA);
    let mut runtime = runtime_of(MODULE, TICKER);
    frame(&view, &mut runtime, PLENTY);
    let mut mail = EventQueue::new();
    mail.push(ScriptEvent::new(entity(0), "Work"));
    let mut cut = 0;
    for n in 1..=128 {
        let fuel = if n <= 64 { fuel(n) } else { PLENTY };
        let report = frame_with(&view, &mail, &mut runtime, fuel);
        mail = report.undelivered;
        if n <= 64 && part_way(&runtime, 0, "Ticker", &Body::Sequence) {
            cut += 1;
        }
    }
    assert_eq!(
        int_of(&runtime, MODULE, 0, "Ticker", "worked"),
        1,
        "set-up: `Work` was handled, once"
    );
    (int_of(&runtime, MODULE, 0, "Ticker", "fired"), cut)
}

/// **A timer starved behind a cut body catches up.** `Ticker`'s handler is
/// cut by a thin slice for sixty-four frames — every turn of them only
/// resumes it, so its `every 0.125s` never runs — then the fuel returns: by
/// the end, the timer has fired as often as in a run that was never cut over
/// the same two seconds.
#[test]
fn a_timer_starved_behind_a_cut_body_catches_up_once_the_fuel_returns() {
    let (uncut, _) = ticker_with(|_| PLENTY);
    let (caught_up, cut) = ticker_with(|_| 30);

    assert!(
        cut >= 30,
        "set-up: the handler stayed part-way {cut} frames"
    );
    assert!(uncut >= 15, "set-up: two seconds fired {uncut} times");
    assert_eq!(
        caught_up, uncut,
        "the starved timer was handed every second it missed"
    );
}

/// `Striker`: an `Update` summing its `dt`, and a handler that works long
/// enough to be cut, then awaits half a second.
const STRIKER: &str = "behavior Striker {
    int struck = 0;
    float lived = 0.0;
    void Update(float dt) { lived += dt; }
    async void Strike() { await 0.5s; struck += 1; }
    on Go() { int i = 0; while (i < 200) { i += 1; } Strike(); }
}
event Go;";

/// **An `await` reached under cuts waits its half second from where it
/// stopped, and no longer.** `Striker`'s handler is cut by a thin slice on
/// its way to `await 0.5s`: the wait starts at the frame it parks, fed none
/// of the time its body spent cut, and the strike lands exactly thirty-two
/// 1/64-s frames later. Its `Update`, held off the whole time, is handed all
/// of it once the fuel returns.
#[test]
fn an_await_reached_under_cuts_waits_its_time_from_where_it_parked() {
    let view = view(MODULE, &[("Striker", &[0])], DELTA);
    let mut runtime = runtime_of(MODULE, STRIKER);
    frame(&view, &mut runtime, PLENTY);
    let mut frames = 1;

    let mut mail = EventQueue::new();
    mail.push(ScriptEvent::new(entity(0), "Go"));
    let waiting = |runtime: &ScriptRuntime| {
        runtime
            .peek(entity(0), "Striker")
            .and_then(|held| held.pending.as_ref())
            .map(|pending| pending.remaining)
            .filter(|remaining| *remaining > 0.0)
    };
    let mut cut = 0;
    let parked = loop {
        assert!(frames < 400, "the handler never reached its `await`");
        let report = frame_with(&view, &mail, &mut runtime, 20);
        mail = report.undelivered;
        frames += 1;
        if let Some(remaining) = waiting(&runtime) {
            break remaining;
        }
        cut += 1;
    };
    assert!(
        cut >= 5,
        "set-up: the handler was cut {cut} times on its way"
    );
    assert_eq!(
        parked, 0.5,
        "the wait starts where it parked, fed nothing from before"
    );

    let mut waited = 0;
    while int_of(&runtime, MODULE, 0, "Striker", "struck") == 0 {
        assert!(waited < 64, "the strike never landed");
        let report = frame_with(&view, &mail, &mut runtime, 20);
        mail = report.undelivered;
        frames += 1;
        waited += 1;
    }
    assert_eq!(
        waited, 32,
        "half a second of 1/64-s frames after it parked, not more"
    );

    for _ in 0..8 {
        frame(&view, &mut runtime, PLENTY);
        frames += 1;
    }
    assert_eq!(
        float_of(&runtime, MODULE, 0, "Striker", "lived"),
        frames as f32 * DELTA,
        "`Update` was handed every second of the {frames} frames, the cut and the wait included"
    );
}

/// The latest record the lane wrote of `Clock` among `reports`, and the
/// frame it was written in.
fn latest_clock_record(reports: &[ScriptRunReport]) -> (usize, khora_core::script::ScriptSnapshot) {
    reports
        .iter()
        .enumerate()
        .rev()
        .find_map(|(at, report)| {
            report
                .state
                .iter()
                .find(|update| update.behavior == "Clock")
                .map(|update| (at, update.snapshot.clone()))
        })
        .expect("the lane wrote the instance's state")
}

/// **The time a cut turn owes survives a save.** `Clock`'s `Update` is cut
/// by a trickle for forty frames; the game is saved at its latest record,
/// through the scene's record, and loaded into a fresh runtime that runs it
/// with plenty. Its total is every second up to the save plus every second
/// since the load.
#[test]
fn the_time_a_cut_turn_owes_survives_a_save() {
    let mut view = view(MODULE, &[("Clock", &[0])], 0.0);
    let mut runtime = runtime_of(MODULE, CLOCK);
    let (reports, deltas) = clock_under_a_trickle(&mut view, &mut runtime, 40);
    let (at, record) = latest_clock_record(&reports);
    let saved = through_record(&record);
    let before: f32 = deltas[..=at].iter().sum();

    // Closed and opened again.
    let mut runtime = runtime_of(MODULE, CLOCK);
    let mut arriving = view.clone();
    arriving.instances[0].arrival = Some(loaded(saved));
    let mut after = 0.0;
    frame_of(&mut arriving, &mut runtime, delta_of(41), PLENTY);
    after += delta_of(41);
    for n in 42..46 {
        frame_of(&mut view, &mut runtime, delta_of(n), PLENTY);
        after += delta_of(n);
    }

    assert_eq!(
        float_of(&runtime, MODULE, 0, "Clock", "total"),
        before + after,
        "every second up to the save (frame {at}) and every second since the load"
    );
}

/// **The time a cut turn owes survives a hot reload.** `Clock`'s `Update` is
/// cut by a trickle for forty frames, then its module is edited — a field
/// added above the others — and it runs with plenty: its total is every
/// second that passed, before the edit and after it.
#[test]
fn the_time_a_cut_turn_owes_survives_a_hot_reload() {
    let mut view = view(MODULE, &[("Clock", &[0])], 0.0);
    let mut runtime = runtime_of(MODULE, CLOCK);
    let (_, mut deltas) = clock_under_a_trickle(&mut view, &mut runtime, 40);

    let edited = CLOCK.replacen(
        "float total = 0.0;",
        "int extra = 0;\n    float total = 0.0;",
        1,
    );
    runtime.reload(MODULE, build(&edited));
    for n in 41..45 {
        frame_of(&mut view, &mut runtime, delta_of(n), PLENTY);
        deltas.push(delta_of(n));
    }

    let elapsed: f32 = deltas.iter().sum();
    assert_eq!(
        float_of(&runtime, MODULE, 0, "Clock", "total"),
        elapsed,
        "every second of the {} frames, across the edit",
        deltas.len()
    );
}

/// `Loader`: a strike two seconds after `Go`, and an `OnLoad` long enough
/// for a thin slice to cut it for many frames.
const LOADER: &str = "behavior Loader {
    int struck = 0;
    int loads = 0;
    async void Strike() { await 2.0s; struck += 1; }
    on Go() { Strike(); }
    void OnLoad() { int i = 0; while (i < 300) { i += 1; } loads += 1; }
}
event Go;";

/// **A wait restored behind a cut `OnLoad` is handed the time `OnLoad`
/// took.** `Loader` hears `Go` and parks on `await 2.0s`; the game is saved
/// that frame and loaded into a fresh runtime whose first sixteen frames cut
/// its `OnLoad` — the restored wait sits behind it. The strike lands exactly
/// two seconds of 1/64-s frames after the load, the frames `OnLoad` was cut
/// in counted.
#[test]
fn a_wait_restored_behind_a_cut_on_load_is_handed_the_time_on_load_took() {
    let view = view(MODULE, &[("Loader", &[0])], DELTA);
    let mut runtime = runtime_of(MODULE, LOADER);
    frame(&view, &mut runtime, PLENTY);
    let mut mail = EventQueue::new();
    mail.push(ScriptEvent::new(entity(0), "Go"));
    let report = frame_with(&view, &mail, &mut runtime, PLENTY);
    let record = report
        .state
        .iter()
        .find(|update| update.behavior == "Loader")
        .map(|update| update.snapshot.clone())
        .expect("the lane wrote the instance's state");
    let saved = through_record(&record);
    assert!(
        saved.pending.is_some(),
        "set-up: the save holds the parked strike"
    );

    // Closed and opened again.
    let mut runtime = runtime_of(MODULE, LOADER);
    let mut arriving = view.clone();
    arriving.instances[0].arrival = Some(loaded(saved));
    frame(&arriving, &mut runtime, 20);
    let mut frames = 1;
    while frames < 16 {
        frame(&view, &mut runtime, 20);
        frames += 1;
    }
    assert_eq!(
        int_of(&runtime, MODULE, 0, "Loader", "loads"),
        0,
        "set-up: `OnLoad` is still cut after sixteen frames"
    );
    while int_of(&runtime, MODULE, 0, "Loader", "struck") == 0 {
        assert!(frames < 400, "the strike never landed");
        frame(&view, &mut runtime, PLENTY);
        frames += 1;
    }

    assert_eq!(
        int_of(&runtime, MODULE, 0, "Loader", "loads"),
        1,
        "set-up: `OnLoad` finished"
    );
    assert_eq!(
        frames, 128,
        "two seconds of 1/64-s frames after the load, the cut `OnLoad`'s included"
    );
}

/// `Pulse`: a timer whose own body is long enough for a thin slice to cut it
/// over many frames, counting its firings.
const PULSE: &str = "behavior Pulse {
    int fired = 0;
    every 0.25s { int i = 0; while (i < 300) { i += 1; } fired += 1; }
}";

/// `Pulse` over 256 frames of 1/64 s — four seconds — the first 128 with
/// `fuel(n)`, the rest with plenty. Its firings, and how many frames ended
/// with its timer's body part-way.
fn pulse_with(fuel: impl Fn(usize) -> u64) -> (i64, usize) {
    let view = view(MODULE, &[("Pulse", &[0])], DELTA);
    let mut runtime = runtime_of(MODULE, PULSE);
    let mut cut = 0;
    for n in 0..256 {
        let fuel = if n < 128 { fuel(n) } else { PLENTY };
        frame(&view, &mut runtime, fuel);
        let timer_part_way = runtime
            .peek(entity(0), "Pulse")
            .and_then(|held| held.pending.as_ref())
            .is_some_and(|pending| matches!(pending.body, Body::Timer { .. }));
        if n < 128 && timer_part_way {
            cut += 1;
        }
    }
    (int_of(&runtime, MODULE, 0, "Pulse", "fired"), cut)
}

/// **A timer whose own body is cut keeps its period.** `Pulse`'s
/// `every 0.25s` body is cut by a thin slice for two seconds, then the fuel
/// returns: the timer was counting the whole time its body ran in pieces, so
/// by the end of four seconds it has fired as often as in a run that was
/// never cut — the seconds its body spent cut are not dropped from its
/// schedule.
#[test]
fn a_timer_whose_own_body_is_cut_keeps_its_period() {
    let (uncut, _) = pulse_with(|_| PLENTY);
    let (cut_run, cut) = pulse_with(|_| 40);

    assert!(
        cut >= 30,
        "set-up: the timer's body stayed part-way {cut} frames"
    );
    assert_eq!(uncut, 16, "set-up: four seconds of `every 0.25s`");
    assert_eq!(
        cut_run, uncut,
        "every second its body spent cut was counted towards its next firing"
    );
}

/// **The time an instance sits out for fuel survives a save.** `Clock` runs
/// a frame with plenty, then sixteen frames with no fuel at all — what a
/// budget at or under the run's overhead buys. The game is saved then, from
/// the latest record the lane wrote, and loaded into a fresh runtime that
/// runs it with plenty: its total is every second up to the save plus every
/// second since the load, the sixteen frames it sat out included.
#[test]
fn the_time_an_instance_sits_out_for_fuel_survives_a_save() {
    let mut view = view(MODULE, &[("Clock", &[0])], 0.0);
    let mut runtime = runtime_of(MODULE, CLOCK);
    let mut reports = vec![frame_of(&mut view, &mut runtime, delta_of(0), PLENTY)];
    let mut before = delta_of(0);
    for n in 1..=16 {
        let report = frame_of(&mut view, &mut runtime, delta_of(n), 0);
        assert_eq!(report.spent, 0, "set-up: frame {n} had no fuel");
        reports.push(report);
        before += delta_of(n);
    }
    let (_, record) = latest_clock_record(&reports);
    let saved = through_record(&record);

    // Closed and opened again.
    let mut runtime = runtime_of(MODULE, CLOCK);
    let mut arriving = view.clone();
    arriving.instances[0].arrival = Some(loaded(saved));
    let mut after = 0.0;
    frame_of(&mut arriving, &mut runtime, delta_of(17), PLENTY);
    after += delta_of(17);
    for n in 18..22 {
        frame_of(&mut view, &mut runtime, delta_of(n), PLENTY);
        after += delta_of(n);
    }

    assert_eq!(
        float_of(&runtime, MODULE, 0, "Clock", "total"),
        before + after,
        "every second up to the save, the frames sat out for fuel included, and every second since the load"
    );
}
