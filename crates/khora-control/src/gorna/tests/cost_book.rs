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

//! The cost book: what the DCC learns of each (agent, row) — measured host
//! time ≈ factor × quoted, older runs forgotten, with the spread of the runs
//! around it — and what a round makes of it: each row's host time planned
//! at its quote corrected, a caution above it while the correction is
//! uncertain, every other device as quoted, and each envelope lent in the
//! agent's own quoted units, so the row it chooses on its quotes is the row
//! planned.

use super::*;
use crate::gorna::arbiter::Arbiter;
use crate::gorna::cost_book::{CostBook, CostBookConfig};
use crate::gorna::planning::{z_of, PlanningConfig};
use khora_core::agent::gorna::protocol::Trigger;
use khora_core::agent::gorna::Correction;
use khora_core::lane::cost::PerLink;

use StrategyId::{Balanced, HighPerformance, LowPower};

const ASH: AgentId = AgentId::new("ash");
const ELM: AgentId = AgentId::new("elm");

/// A book of the default settings.
fn book() -> CostBook {
    CostBook::new(CostBookConfig::default())
}

/// The next draw of `state` (xorshift64), uniform in [-1, 1].
fn draw(state: &mut u64) -> f64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    (x >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

/// A generator's first state.
const SEED: u64 = 0x2545_F491_4F6C_DD1D;

/// `runs` runs of `agent`'s `row` into `book`: each quoting between 0.5 and
/// 1.5 ms on the host, as a lane's quote follows its workload, and taking
/// `factor` times its quote, within ±`noise` of it, relative.
fn run(
    book: &mut CostBook,
    agent: AgentId,
    row: StrategyId,
    factor: f64,
    noise: f64,
    runs: usize,
    state: &mut u64,
) {
    for _ in 0..runs {
        let quoted_us = 1_000.0 + 500.0 * draw(state);
        let measured_us = quoted_us * factor * (1.0 + noise * draw(state));
        book.observe(
            agent,
            row,
            Duration::from_secs_f64(quoted_us / 1e6),
            Duration::from_secs_f64(measured_us / 1e6),
        );
    }
}

// ── What the book learns ──────────────────────────────────────────

/// **The book converges to a linear cost.** A row whose runs take 1.5 times
/// what they quote, within ±5 %, is learned at 1.5 within 0.04, and its
/// spread narrows as runs come: below the prior after five runs, narrower
/// still after two hundred, never zero while the runs vary. The default
/// book forgets each older run by 0.95 a run, starts from a spread of 0.5,
/// and halves a row's own estimate every 120 runs of its agent it misses.
#[test]
fn rls_converges_to_a_linear_cost() {
    assert_eq!(
        CostBookConfig::default(),
        CostBookConfig {
            forgetting: 0.95,
            prior_spread: 0.5,
            stale_half_life: 120,
        }
    );
    let mut book = book();
    assert_eq!(book.config(), CostBookConfig::default());
    let mut state = SEED;

    run(&mut book, ASH, Balanced, 1.5, 0.05, 5, &mut state);
    let early = book.correction(ASH, Balanced);
    run(&mut book, ASH, Balanced, 1.5, 0.05, 195, &mut state);
    let late = book.correction(ASH, Balanced);

    assert!(
        (late.factor - 1.5).abs() < 0.04,
        "runs of 1.5 × their quote: {late:?}"
    );
    assert!(early.spread < 0.5, "five runs narrow the prior: {early:?}");
    assert!(
        late.spread < early.spread,
        "more runs, a narrower spread: {early:?} then {late:?}"
    );
    assert!(late.spread > 0.0, "the runs still vary: {late:?}");
}

/// **The book follows a change of cost.** A row learned at 1 over a hundred
/// runs then costs twice its quote: ten runs later its factor has moved
/// past 1.2, sixty runs later it is within 0.1 of 2. A book that forgets
/// nothing (λ = 1), fed the same runs, is still more than 0.3 from 2: the
/// forgetting is what follows the change.
#[test]
fn the_book_follows_a_change_of_cost() {
    let mut forgetting = book();
    let mut state = SEED;
    run(&mut forgetting, ASH, Balanced, 1.0, 0.0, 100, &mut state);
    run(&mut forgetting, ASH, Balanced, 2.0, 0.0, 10, &mut state);
    let moving = forgetting.correction(ASH, Balanced);
    run(&mut forgetting, ASH, Balanced, 2.0, 0.0, 50, &mut state);
    let followed = forgetting.correction(ASH, Balanced);

    assert!(
        moving.factor > 1.2,
        "ten runs at twice the quote move it: {moving:?}"
    );
    assert!(
        (followed.factor - 2.0).abs() < 0.1,
        "sixty runs at twice the quote: {followed:?}"
    );

    let mut never = CostBook::new(CostBookConfig {
        forgetting: 1.0,
        ..CostBookConfig::default()
    });
    let mut state = SEED;
    run(&mut never, ASH, Balanced, 1.0, 0.0, 100, &mut state);
    run(&mut never, ASH, Balanced, 2.0, 0.0, 60, &mut state);
    let remembered = never.correction(ASH, Balanced);
    assert!(
        (remembered.factor - 2.0).abs() > 0.3,
        "a book that forgets nothing still weighs the hundred runs at 1: {remembered:?}"
    );
}

/// **A row never run plans at its agent's mean correction.** `ash`'s floor
/// is learned at 2 from steady runs, its `Balanced` at 3 from runs spread
/// ±20 %: its `HighPerformance`, never run, is corrected by their mean
/// factor and their mean spread — it can only be learned by running it, so
/// it is not held back by a prior of its own. An agent with no learned row
/// plans at its quotes: factor 1, no spread, whatever book settings name it.
/// (The book relaxes no estimate here, so the floor, unrun while `Balanced`
/// runs, keeps what it learned.)
#[test]
fn a_row_never_run_plans_at_its_agents_mean_correction() {
    let mut book = CostBook::new(CostBookConfig {
        forgetting: 0.95,
        prior_spread: 0.8,
        stale_half_life: 0,
    });
    let mut state = SEED;
    run(&mut book, ASH, LowPower, 2.0, 0.0, 60, &mut state);
    run(&mut book, ASH, Balanced, 3.0, 0.2, 60, &mut state);
    let (low, mid) = (
        book.correction(ASH, LowPower),
        book.correction(ASH, Balanced),
    );
    assert!((low.factor - 2.0).abs() < 1e-3, "{low:?}");
    assert!((mid.factor - 3.0).abs() < 0.1, "{mid:?}");
    assert!(
        mid.spread > low.spread,
        "noisy runs spread wider: {low:?}, {mid:?}"
    );

    let never = book.correction(ASH, HighPerformance);

    assert!(
        (never.factor - (low.factor + mid.factor) / 2.0).abs() < 1e-12,
        "the mean factor of {low:?} and {mid:?}: {never:?}"
    );
    assert!(
        (never.spread - (low.spread + mid.spread) / 2.0).abs() < 1e-12,
        "the mean spread of {low:?} and {mid:?}: {never:?}"
    );
    assert_eq!(
        book.correction(ELM, Balanced),
        Correction::default(),
        "an agent with nothing learned plans at its quotes"
    );
    assert_eq!(
        self::book().correction(ELM, LowPower),
        Correction {
            factor: 1.0,
            spread: 0.0,
        },
        "the default book, before any run"
    );
}

/// **Each row learns its own correction.** `ash`'s floor costs its quote
/// and its `HighPerformance` twice its quote:
/// - forty runs of the floor, then forty of the upgrade, learn 1 and 2
///   apart; the upgrade's runs leave the floor's correction as it was;
/// - runs alternating between the two, one switch a run, still learn them
///   apart;
/// - `elm`'s floor, costing three times its quote, leaves `ash`'s alone.
///
/// The book here relaxes no estimate, so what is checked is what each row
/// learned from its own runs: how an estimate unrun drifts back to its
/// agent's mean is `a_row_long_unrun_returns_to_its_agents_mean`.
#[test]
fn each_row_learns_its_own_correction() {
    let mut book = CostBook::new(CostBookConfig {
        stale_half_life: 0,
        ..CostBookConfig::default()
    });
    let mut state = SEED;
    run(&mut book, ASH, LowPower, 1.0, 0.02, 40, &mut state);
    let floor = book.correction(ASH, LowPower);
    run(&mut book, ASH, HighPerformance, 2.0, 0.02, 40, &mut state);
    let upgrade = book.correction(ASH, HighPerformance);

    assert!((floor.factor - 1.0).abs() < 0.03, "{floor:?}");
    assert!((upgrade.factor - 2.0).abs() < 0.05, "{upgrade:?}");
    assert_eq!(
        book.correction(ASH, LowPower),
        floor,
        "the upgrade's runs leave the floor's correction"
    );

    for _ in 0..60 {
        run(&mut book, ASH, LowPower, 1.0, 0.02, 1, &mut state);
        run(&mut book, ASH, HighPerformance, 2.0, 0.02, 1, &mut state);
    }
    let (floor, upgrade) = (
        book.correction(ASH, LowPower),
        book.correction(ASH, HighPerformance),
    );
    assert!(
        (floor.factor - 1.0).abs() < 0.03,
        "a switch every run: {floor:?}"
    );
    assert!(
        (upgrade.factor - 2.0).abs() < 0.05,
        "a switch every run: {upgrade:?}"
    );

    run(&mut book, ELM, LowPower, 3.0, 0.0, 40, &mut state);
    assert_eq!(
        book.correction(ASH, LowPower),
        floor,
        "another agent's row is its own"
    );
    assert!((book.correction(ELM, LowPower).factor - 3.0).abs() < 1e-3);
}

/// `ash`'s `row` reported once, quoting 1 ms on the host and taking
/// `factor` times that, on the first run of a layer just installed: it
/// carries the time its preparation took.
fn warm_up(row: StrategyId, factor: f64) -> Report {
    Report {
        switch: Some(ms(4)),
        ..run_quoting("ash", row, 1_000, factor)
    }
}

/// **A layer's first run teaches nothing.** The first run of a layer just
/// installed runs cold — pipelines and caches still to fill — so the
/// report carrying its preparation time is no measure of the row:
/// - `ash`'s floor learned at 2 and its `HighPerformance` at 3; two hundred
///   and forty first runs of its `Balanced` at ×50 leave `Balanced` never
///   run (the mean of the two), and age neither learned row: each reads
///   exactly as before;
/// - a first run of the learned `HighPerformance` at ×300 leaves it as it
///   was;
/// - the same report without its preparation time teaches as any run.
#[test]
fn a_layers_first_run_teaches_nothing() {
    let mut book = book();
    let mut state = SEED;
    run(&mut book, ASH, LowPower, 2.0, 0.0, 60, &mut state);
    run(&mut book, ASH, HighPerformance, 3.0, 0.0, 60, &mut state);
    let (floor, best) = (
        book.correction(ASH, LowPower),
        book.correction(ASH, HighPerformance),
    );
    let never = book.correction(ASH, Balanced);

    for _ in 0..240 {
        book.observe_report(&warm_up(Balanced, 50.0));
    }
    book.observe_report(&warm_up(HighPerformance, 300.0));

    assert_eq!(
        book.correction(ASH, Balanced),
        never,
        "first runs leave a row never run"
    );
    assert_eq!(
        book.correction(ASH, LowPower),
        floor,
        "first runs are no runs of the agent: they age no row"
    );
    assert_eq!(
        book.correction(ASH, HighPerformance),
        best,
        "a first run teaches a learned row nothing"
    );

    let warm = Report {
        switch: None,
        ..warm_up(Balanced, 50.0)
    };
    for _ in 0..30 {
        book.observe_report(&warm);
    }
    let learned = book.correction(ASH, Balanced);
    assert!(
        (learned.factor - 50.0).abs() < 0.5,
        "runs past the first teach: {learned:?}"
    );
}

/// **A row long unrun returns to its agent's mean.** `ash`'s `Balanced` is
/// learned at 300 over ten runs, then only its floor runs, at 20. Each run
/// of the agent it misses weighs `Balanced`'s own estimate down — half every
/// 120 — the rest going to its agent's other learned rows, here the floor:
/// - another agent's runs are not `ash`'s: they age none of its rows;
/// - after 120 runs of the floor, `Balanced` is halfway: about 160;
/// - after 1 200, within 3 % of 20, its spread the floor's within 0.005;
/// - one run of `Balanced` again, at 300, sets it back on its own estimate;
/// - a book whose half-life is 0 never relaxes: after 1 200 runs of the
///   floor, `Balanced` still reads 300.
#[test]
fn a_row_long_unrun_returns_to_its_agents_mean() {
    let teach = |book: &mut CostBook, state: &mut u64| {
        run(book, ASH, Balanced, 300.0, 0.0, 10, state);
    };
    let mut book = book();
    let mut state = SEED;
    teach(&mut book, &mut state);
    assert!((book.correction(ASH, Balanced).factor - 300.0).abs() < 1e-3);

    run(&mut book, ELM, LowPower, 5.0, 0.0, 1_200, &mut state);
    assert!(
        (book.correction(ASH, Balanced).factor - 300.0).abs() < 1e-3,
        "`elm`'s runs are not `ash`'s: {:?}",
        book.correction(ASH, Balanced)
    );

    run(&mut book, ASH, LowPower, 20.0, 0.0, 120, &mut state);
    let halfway = book.correction(ASH, Balanced);
    assert!(
        (halfway.factor - 160.0).abs() < 1.0,
        "half of 300 and half of 20 after 120 runs: {halfway:?}"
    );

    run(&mut book, ASH, LowPower, 20.0, 0.0, 1_080, &mut state);
    let (relaxed, floor) = (
        book.correction(ASH, Balanced),
        book.correction(ASH, LowPower),
    );
    assert!(
        (relaxed.factor - 20.0).abs() / 20.0 < 0.03,
        "after 1 200 runs of the floor, the floor's 20: {relaxed:?}"
    );
    assert!(
        (relaxed.spread - floor.spread).abs() < 0.005,
        "and its spread: {relaxed:?}, {floor:?}"
    );

    run(&mut book, ASH, Balanced, 300.0, 0.0, 1, &mut state);
    let rerun = book.correction(ASH, Balanced);
    assert!(
        (rerun.factor - 300.0).abs() < 1.0,
        "run again, its own estimate: {rerun:?}"
    );

    let mut never = CostBook::new(CostBookConfig {
        stale_half_life: 0,
        ..CostBookConfig::default()
    });
    let mut state = SEED;
    teach(&mut never, &mut state);
    run(&mut never, ASH, LowPower, 20.0, 0.0, 1_200, &mut state);
    assert!(
        (never.correction(ASH, Balanced).factor - 300.0).abs() < 1e-3,
        "a half-life of 0 never relaxes: {:?}",
        never.correction(ASH, Balanced)
    );
}

/// **A run that quoted no host time teaches nothing.** Runs of a row quoting
/// no time leave it as never run; so do reports with no workload, or a
/// workload of zero. A report's workload is the host time its row quoted,
/// in µs, and only its host time is read: thirty reports quoting 1 ms that
/// took 2 ms on the host and 50 ms on the graphics processor learn 2.
#[test]
fn a_run_that_quoted_no_host_time_teaches_nothing() {
    let mut book = book();
    // Never run, by an agent with nothing learned: planned at its quote.
    let never = Correction::default();
    for _ in 0..20 {
        book.observe(ASH, LowPower, Duration::ZERO, ms(3));
    }
    assert_eq!(book.correction(ASH, LowPower), never, "no quote, no run");

    let mut report = run_quoting("ash", LowPower, 1_000, 2.0);
    report.measured.gpu = ms(50);
    for workload in [None, Some(0.0)] {
        let unquoted = Report {
            workload,
            ..report.clone()
        };
        for _ in 0..20 {
            book.observe_report(&unquoted);
        }
    }
    assert_eq!(
        book.correction(ASH, LowPower),
        never,
        "no workload, or none quoted"
    );

    for _ in 0..30 {
        book.observe_report(&report);
    }
    let learned = book.correction(ASH, LowPower);
    assert!(
        (learned.factor - 2.0).abs() < 1e-3,
        "2 ms on the host for 1 ms quoted, whatever the graphics processor took: {learned:?}"
    );
}

/// **The book corrects every row it is offered.** Offers of `ash` (its floor
/// learned at 2, its `Balanced` never run) and `elm` (nothing learned): one
/// correction per offered row, each the book's own for that row.
#[test]
fn the_book_corrects_every_row_it_is_offered() {
    let mut book = book();
    let mut state = SEED;
    run(&mut book, ASH, LowPower, 2.0, 0.0, 60, &mut state);
    let offers = [
        offer(
            "ash",
            vec![row(LowPower, 1_000, 0.4), row(Balanced, 3_000, 0.7)],
            LowPower,
        ),
        offer("elm", vec![row(LowPower, 500, 0.5)], LowPower),
    ];

    let corrections = book.corrections(&offers);

    assert_eq!(corrections.len(), 3, "{corrections:?}");
    for (agent, row) in [(ASH, LowPower), (ASH, Balanced), (ELM, LowPower)] {
        let named = (corrections.iter())
            .find(|(named, id, _)| *named == agent && *id == row)
            .map(|(_, _, correction)| *correction);
        assert_eq!(
            named,
            Some(book.correction(agent, row)),
            "{agent}'s {row:?} in {corrections:?}"
        );
    }
}

// ── What a round makes of it ──────────────────────────────────────

/// The row an agent offering `rows` runs within `envelope`: the best
/// quality whose own quote fits it, else its floor (the first row).
fn chosen_on_quotes(rows: &[OfferRow], envelope: &Envelope) -> StrategyId {
    rows.iter()
        .filter(|row| fits(&row.cost, envelope))
        .max_by(|a, b| a.quality.total_cmp(&b.quality))
        .map_or(rows[0].id, |row| row.id)
}

/// A row of `cpu_us` on the host and `gpu_us` on the graphics processor.
fn row_on_both(id: StrategyId, cpu_us: u64, gpu_us: u64, quality: f32) -> OfferRow {
    let mut row = row(id, cpu_us, quality);
    row.cost.time.gpu = us(gpu_us);
    row
}

/// **Only the host time is corrected.** `ash`'s floor runs twice the host
/// time it quotes:
/// - quoting 2 ms on the host and 6 ms on the graphics processor, it is
///   planned at about 4.4 ms and 6 ms in a 10 ms frame: the round is not
///   `Safety` (corrected on the graphics processor too, it would be 13 ms),
///   and the envelope lends it at least its 6 ms there;
/// - quoting 6 ms on the host and 2 ms on the graphics processor, its host
///   time is over 12 ms: the round is `Safety`, where its quote alone fits.
#[test]
fn only_the_host_time_is_corrected() {
    let frame = |rows: Vec<OfferRow>, taught: bool| {
        let (_, agent) = shared(Bidder::new("ash", rows, LowPower));
        let mut arbitrator = arbitrator();
        if taught {
            let quoted_us = 2_000;
            teach(&mut arbitrator, "ash", LowPower, quoted_us, 2.0);
        }
        arbitrator.allocate(
            &playing(),
            &frame_of(10.0),
            Trigger::Refresh,
            &[agent],
            &nothing(),
        )
    };
    let busy_host = || vec![row_on_both(LowPower, 6_000, 2_000, 0.4)];
    let busy_gpu = || vec![row_on_both(LowPower, 2_000, 6_000, 0.4)];
    assert_eq!(
        frame(busy_host(), false).trigger,
        Trigger::Refresh,
        "quoted, 6 ms fit 10 ms"
    );

    let gpu_bound = frame(busy_gpu(), true);
    assert_eq!(
        gpu_bound.trigger,
        Trigger::Refresh,
        "the graphics processor's 6 ms stay as quoted: {gpu_bound:?}"
    );
    let lent = lent_to(&gpu_bound, "ash");
    assert!(
        lent.time.gpu >= ms(6),
        "the graphics processor's time is lent as quoted: {lent:?}"
    );

    let host_bound = frame(busy_host(), true);
    assert_eq!(
        host_bound.trigger,
        Trigger::Safety,
        "the host's 6 ms run as 12: {host_bound:?}"
    );
}

/// Teaches `arbitrator`'s cost book that `name`'s `row`, quoting
/// `quoted_us` µs on the host, runs alternately 0.4 and 1.6 times its quote:
/// a factor near 1, its runs spread widely around it.
fn teach_widely_spread(
    arbitrator: &mut GornaArbitrator,
    name: &'static str,
    row: StrategyId,
    quoted_us: u64,
) {
    for run in 0..TAUGHT_RUNS {
        let factor = if run % 2 == 0 { 0.4 } else { 1.6 };
        arbitrator
            .cost_book_mut()
            .observe_report(&run_quoting(name, row, quoted_us, factor));
    }
}

/// **An uncertain row is planned with caution.** `ash` offers its floor,
/// 2 ms at 0.4, and an upgrade, 6 ms at 1.0, in a 9 ms frame, both running
/// about what they quote. A round plans a row above the floor at
/// (factor + z × spread) × quote, z the planner's for its risk (about 1.645
/// at 5 %), and the floor at its factor alone.
/// - The upgrade's runs spread widely (0.4 then 1.6 times its quote): its
///   spread is over 0.4, so it is planned at about 2 × 6 = 12 ms and held
///   back — the envelope lent fits no upgrade on the agent's quotes.
/// - The same upgrade with steady runs has a narrow spread: planned at
///   about 1.1 × 6 ≈ 6.6 ms, it is taken.
/// - A floor of 8 ms whose runs spread as widely is planned at its factor,
///   about 8 ms: the round is not `Safety` — what the book does not know
///   never takes away the floor.
#[test]
fn an_uncertain_row_is_planned_with_caution() {
    let rows = vec![row(LowPower, 2_000, 0.4), row(HighPerformance, 6_000, 1.0)];
    let round = |arbitrator: &mut GornaArbitrator, rows: &[OfferRow]| {
        let (_, agent) = shared(Bidder::new("ash", rows.to_vec(), LowPower));
        arbitrator.allocate(
            &playing(),
            &frame_of(9.0),
            Trigger::Refresh,
            &[agent],
            &nothing(),
        )
    };
    let spread_of = |arbitrator: &GornaArbitrator, row: StrategyId| {
        (arbitrator.corrections().iter())
            .find(|(agent, id, _)| *agent == ASH && *id == row)
            .map(|(_, _, correction)| correction.spread)
            .unwrap_or_else(|| panic!("{row:?} in {:?}", arbitrator.corrections()))
    };

    let mut uncertain = arbitrator();
    teach(&mut uncertain, "ash", LowPower, 2_000, 1.0);
    teach_widely_spread(&mut uncertain, "ash", HighPerformance, 6_000);
    let held = round(&mut uncertain, &rows);

    let caution = uncertain.planning().caution;
    assert!(
        (caution - z_of(PlanningConfig::default().risk)).abs() < 1e-12,
        "the round plans z of its risk: {caution}"
    );
    assert!(
        spread_of(&uncertain, HighPerformance) > 0.4,
        "runs of 0.4 and 1.6 times the quote spread widely: {:?}",
        uncertain.corrections()
    );
    assert_eq!(
        chosen_on_quotes(&rows, &lent_to(&held, "ash")),
        LowPower,
        "the upgrade is held back while its spread is wide: {held:?}"
    );

    let mut steady = arbitrator();
    teach(&mut steady, "ash", LowPower, 2_000, 1.0);
    teach(&mut steady, "ash", HighPerformance, 6_000, 1.0);
    let taken = round(&mut steady, &rows);

    assert!(
        spread_of(&steady, HighPerformance) < 0.1,
        "sixty runs alike narrow it: {:?}",
        steady.corrections()
    );
    assert_eq!(
        chosen_on_quotes(&rows, &lent_to(&taken, "ash")),
        HighPerformance,
        "the upgrade is taken once its runs are steady: {taken:?}"
    );

    let mut floor_only = arbitrator();
    teach_widely_spread(&mut floor_only, "ash", LowPower, 8_000);
    let floor = round(&mut floor_only, &[row(LowPower, 8_000, 0.4)]);
    assert!(
        spread_of(&floor_only, LowPower) > 0.4,
        "{:?}",
        floor_only.corrections()
    );
    assert_ne!(
        floor.trigger,
        Trigger::Safety,
        "the floor is planned at its factor, not its caution: {floor:?}"
    );
}

/// **An envelope is lent in the agent's quoted units.** `ash` offers 2 ms at
/// 0.4, 4 ms at 0.7 and 7 ms at 1.0, and every run of each took twice what
/// it quoted. In a 12 ms frame, the round plans `Balanced` (about 8.7 ms
/// corrected; the best row, about 15 ms, does not fit), and lends its
/// envelope — decided in corrected time — divided by `Balanced`'s planned
/// factor: within the frame in real time, it fits `Balanced`'s 4 ms quote
/// and not the best row's 7 ms, so the agent, choosing on its own quotes,
/// runs the row planned.
#[test]
fn an_envelope_is_lent_in_the_agents_quoted_units() {
    let rows = vec![
        row(LowPower, 2_000, 0.4),
        row(Balanced, 4_000, 0.7),
        row(HighPerformance, 7_000, 1.0),
    ];
    let (_, agent) = shared(Bidder::new("ash", rows.clone(), LowPower));
    let mut arbitrator = arbitrator();
    for quoted in &rows {
        teach(
            &mut arbitrator,
            "ash",
            quoted.id,
            micros(quoted.cost.time.cpu),
            2.0,
        );
    }

    let allocation = arbitrator.allocate(
        &playing(),
        &frame_of(12.0),
        Trigger::Refresh,
        &[agent],
        &nothing(),
    );

    let caution = arbitrator.planning().caution;
    let corrections = arbitrator.corrections().to_vec();
    assert_eq!(corrections.len(), 3, "every row offered: {corrections:?}");
    let planned_factor = |row: StrategyId| {
        (corrections.iter())
            .find(|(agent, id, _)| *agent == ASH && *id == row)
            .map(|(_, _, correction)| correction.planned(caution))
            .unwrap_or_else(|| panic!("{row:?} in {corrections:?}"))
    };
    for quoted in &rows {
        assert!(
            planned_factor(quoted.id) >= 2.0,
            "{:?} runs twice its quote: {corrections:?}",
            quoted.id
        );
    }
    // The round as the DCC decides it, in corrected time.
    let corrected_rows: Vec<OfferRow> = (rows.iter())
        .map(|quoted| {
            let mut corrected = *quoted;
            corrected.cost.time.cpu = quoted.cost.time.cpu.mul_f64(planned_factor(quoted.id));
            corrected
        })
        .collect();
    let bids = [Bid {
        offer: Offer {
            agent: ASH,
            epoch: allocation.epoch,
            rows: corrected_rows,
            floor: LowPower,
            current: LowPower,
            health: 1.0,
        },
        importance: AgentImportance::Important,
        mode: AdaptationMode::Learning,
        hints: AgentHints::default(),
        kept: None,
        late: false,
    }];
    let decision = Arbiter::default().decide(&Round {
        epoch: allocation.epoch,
        trigger: Trigger::Refresh,
        target: arbitrator.target(),
        memory: arbitrator.memory(),
        link_rate: PerLink::default(),
        waves: &[],
        background: &[],
        bids: &bids,
        planning: arbitrator.planning().clone(),
    });
    assert_eq!(decision.rows, vec![(ASH, Balanced)], "{:?}", decision.why);
    let in_real_time = lent_to(&decision.allocation, "ash").time.cpu;

    let lent = lent_to(&allocation, "ash");

    let expected = in_real_time.as_secs_f64() / planned_factor(Balanced);
    assert!(
        (lent.time.cpu.as_secs_f64() - expected).abs() <= 2e-6,
        "the corrected plan's {in_real_time:?} ÷ {}: {expected} s, lent {:?}",
        planned_factor(Balanced),
        lent.time.cpu
    );
    assert!(
        lent.time.cpu.as_secs_f64() * planned_factor(Balanced) <= 0.012 + 1e-6,
        "within the 12 ms frame in real time: {lent:?}"
    );
    assert_eq!(
        chosen_on_quotes(&rows, &lent),
        Balanced,
        "the agent runs the row planned: {lent:?}"
    );
}

/// **A `Safety` round lends the floor at its quote, whatever row is
/// pinned.** `ash`'s floor quotes 2 ms and runs as quoted; its
/// `HighPerformance`, pinned by the developer, runs twice its 3 ms quote. A
/// round opened as `Safety` lends every agent its floor: `ash`'s envelope
/// is its floor's 2 ms, in its quoted units — not the floor's corrected time
/// divided by the pinned row's correction, about 0.95 ms, under the floor it
/// runs whatever is lent.
#[test]
fn a_safety_round_lends_the_floor_at_its_quote_whatever_row_is_pinned() {
    let rows = vec![row(LowPower, 2_000, 0.4), row(HighPerformance, 3_000, 1.0)];
    let (_, agent) = shared(Bidder::new("ash", rows.clone(), LowPower));
    let mut arbitrator = arbitrator();
    teach(&mut arbitrator, "ash", LowPower, 2_000, 1.0);
    teach(&mut arbitrator, "ash", HighPerformance, 3_000, 2.0);
    arbitrator.set_adaptation_mode(ASH, AdaptationMode::Manual(HighPerformance));

    let allocation = arbitrator.allocate(
        &playing(),
        &frame_of(16.0),
        Trigger::Safety,
        &[agent],
        &nothing(),
    );

    assert_eq!(allocation.trigger, Trigger::Safety, "{allocation:?}");
    let lent = lent_to(&allocation, "ash");
    assert!(
        fits(&rows[0].cost, &lent),
        "the floor's 2 ms quote fits what is lent: {lent:?}"
    );
}

/// **The row planned fits the envelope lent for it when it fills the
/// frame.** `ash`'s rows run 1.25 times their quotes, learned without
/// spread, and the round plans at the factor alone: its `HighPerformance`,
/// quoting 1999 µs, is planned at 2498.75 µs in a 2498 µs frame (times are
/// counted in whole microseconds), and taken. The envelope lent for it, in
/// `ash`'s quoted units, still fits its 1999 µs quote, so `ash` runs the row
/// planned.
#[test]
fn the_row_planned_fits_the_envelope_lent_for_it_when_it_fills_the_frame() {
    let rows = vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 1_999, 1.0)];
    let learned = Correction {
        factor: 1.25,
        spread: 0.0,
    };
    let corrections = vec![(ASH, LowPower, learned), (ASH, HighPerformance, learned)];
    let offer = Offer {
        agent: ASH,
        epoch: Epoch(1),
        rows: rows.clone(),
        floor: LowPower,
        current: LowPower,
        health: 1.0,
    };
    let mut bids = vec![Bid {
        offer: offer.clone(),
        importance: AgentImportance::Important,
        mode: AdaptationMode::Learning,
        hints: AgentHints::default(),
        kept: None,
        late: false,
    }];
    // What the kept agents hold, in their quoted units, before the bids are
    // corrected: none here.
    let held: Vec<(AgentId, Envelope)> = (bids.iter())
        .filter_map(|bid| bid.kept.map(|envelope| (bid.offer.agent, envelope)))
        .collect();
    for bid in &mut bids {
        crate::gorna::cost_book::correct_bid(bid, &corrections, 0.0);
    }
    let mut decision = Arbiter::default().decide(&Round {
        epoch: Epoch(1),
        trigger: Trigger::Refresh,
        target: us(2_498),
        memory: PerDevice::default(),
        link_rate: PerLink::default(),
        waves: &[],
        background: &[],
        bids: &bids,
        planning: Planning::default(),
    });
    assert_eq!(
        decision.rows,
        vec![(ASH, HighPerformance)],
        "{:?}",
        decision.why
    );

    crate::gorna::cost_book::lend_in_quotes(
        &mut decision.allocation,
        &[offer],
        &bids,
        &decision.rows,
        &decision.kept,
        &held,
        &corrections,
        0.0,
    );

    let lent = lent_to(&decision.allocation, "ash");
    assert_eq!(
        chosen_on_quotes(&rows, &lent),
        HighPerformance,
        "the row planned fits what is lent: {lent:?}"
    );
}

/// **A bad first measure does not hold a row back for ever.** `ash` offers
/// its floor, 0.1 ms at 0.4, and `Balanced`, 0.4 ms at 0.9, in a 16 ms frame.
/// `Balanced` ran once at 300 times its quote — a cold start —, then only the
/// floor runs, at 20 times its quote:
/// - sixty runs on, `Balanced` is still planned far over 100 × 0.4 ms, over
///   the frame: the round plans the floor;
/// - six hundred runs on, its estimate has relaxed toward the floor's 20:
///   at about 0.4 ms × 29 it fits, and the round plans it — the agent,
///   choosing on its quotes, runs it, and can learn it again.
#[test]
fn a_bad_first_measure_does_not_hold_a_row_back_for_ever() {
    let rows = vec![row(LowPower, 100, 0.4), row(Balanced, 400, 0.9)];
    let round = |arbitrator: &mut GornaArbitrator| {
        let (_, agent) = shared(Bidder::new("ash", rows.clone(), LowPower));
        arbitrator.allocate(
            &playing(),
            &frame_of(16.0),
            Trigger::Refresh,
            &[agent],
            &nothing(),
        )
    };
    let mut arbitrator = arbitrator();
    (arbitrator.cost_book_mut()).observe_report(&run_quoting("ash", Balanced, 400, 300.0));
    teach(&mut arbitrator, "ash", LowPower, 100, 20.0);

    let first = round(&mut arbitrator);
    assert_eq!(
        chosen_on_quotes(&rows, &lent_to(&first, "ash")),
        LowPower,
        "`Balanced` still reads far above ×20: {:?}",
        arbitrator.corrections()
    );

    for _ in 0..9 {
        teach(&mut arbitrator, "ash", LowPower, 100, 20.0);
    }
    let later = round(&mut arbitrator);
    assert_eq!(
        chosen_on_quotes(&rows, &lent_to(&later, "ash")),
        Balanced,
        "`Balanced` relaxed toward the floor's ×20: {:?}",
        arbitrator.corrections()
    );
}

/// `time` in whole microseconds.
fn micros(time: Duration) -> u64 {
    u64::try_from(time.as_micros()).unwrap_or(u64::MAX)
}

/// **A `Stable` agent keeps its envelope as it holds it, under any
/// correction.** `ash`'s rows run 0.77 times their quotes (a lane quoting
/// above what it takes). The first round lends `ash` an envelope in its quoted
/// units; `Stable` from then on, the second round keeps it — every
/// nanosecond of it, though the round plans it in measured time and lends it
/// back in quoted time.
#[test]
fn a_stable_agent_keeps_its_envelope_as_it_holds_it_under_a_correction() {
    let rows = vec![row(LowPower, 3_333, 0.4), row(HighPerformance, 9_999, 1.0)];
    let (_, agent) = shared(Bidder::new("ash", rows, LowPower));
    let mut arbitrator = arbitrator();
    teach(&mut arbitrator, "ash", LowPower, 3_333, 0.77);
    teach(&mut arbitrator, "ash", HighPerformance, 9_999, 0.77);
    let round = |arbitrator: &mut GornaArbitrator| {
        arbitrator.allocate(
            &playing(),
            &frame_of(16.0),
            Trigger::Refresh,
            &[Arc::clone(&agent)],
            &nothing(),
        )
    };

    let held = lent_to(&round(&mut arbitrator), "ash");
    arbitrator.set_adaptation_mode(ASH, AdaptationMode::Stable);
    let kept = lent_to(&round(&mut arbitrator), "ash");

    assert_eq!(kept, held, "the envelope it held, kept");
}
