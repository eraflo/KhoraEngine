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

//! The guards a fetch runs: a refusal names the field, a warning lets the
//! lane run on the value as it is.

use khora_core::gpu::GpuSetup;
use khora_core::lane::erased::{AnyLane, Erased, RunOutcome};
use khora_core::lane::guard::{InputRefusal, InputWarning, Reason, SourceKind, ValueProblem};
use khora_core::lane::io::{FrameId, InputSource, LaneInputs, OutputSink};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::problems::ProblemSeverity;
use khora_core::Runtime;

use crate::fixtures::{
    CheckedSpeeds, Fastest, FastestLane, MixSettings, OneView, Positions, Speeds, ViewAndSettings,
};

/// A missing input is refused, and the refusal names the field and its
/// source — so `Problems` can say which input of which lane, not "a lane
/// could not run".
#[test]
fn a_missing_input_is_refused_by_name() {
    let frame = FrameId(1);
    let runtime = Runtime::new();
    let mut bus = LaneBus::new();
    bus.set_frame(frame);
    bus.publish(Positions(vec![0.0]));

    let mut source = InputSource::new(frame, &bus, &runtime);
    let refusal = ViewAndSettings::fetch(&mut source)
        .err()
        .expect("the runtime holds no MixSettings");
    assert_eq!(
        refusal,
        InputRefusal {
            field: "settings",
            source: SourceKind::Resource,
            reason: Reason::Missing,
        }
    );

    let empty_bus = {
        let mut bus = LaneBus::new();
        bus.set_frame(frame);
        bus
    };
    let mut runtime = Runtime::new();
    runtime.resources.insert(MixSettings { gain: 1.0 });
    let mut source = InputSource::new(frame, &empty_bus, &runtime);
    let refusal = ViewAndSettings::fetch(&mut source)
        .err()
        .expect("no Flow published Positions");
    assert_eq!(
        refusal,
        InputRefusal {
            field: "view",
            source: SourceKind::Bus,
            reason: Reason::Missing,
        }
    );
}

/// A View published for an earlier frame and never republished is refused as
/// stale: a lane drawing last frame's positions is a bug no one sees. The
/// same view republished for this frame passes.
#[test]
fn a_stale_view_is_refused() {
    let runtime = Runtime::new();
    let mut bus = LaneBus::new();
    bus.set_frame(FrameId(1));
    bus.publish(Positions(vec![1.0]));
    bus.set_frame(FrameId(2));
    assert_eq!(bus.stamp_of::<Positions>(), Some(FrameId(1)));

    let mut source = InputSource::new(FrameId(2), &bus, &runtime);
    let refusal = OneView::fetch(&mut source)
        .err()
        .expect("the view is from frame 1, the source is for frame 2");
    assert_eq!(
        refusal,
        InputRefusal {
            field: "view",
            source: SourceKind::Bus,
            reason: Reason::Stale,
        }
    );

    bus.publish(Positions(vec![2.0]));
    assert_eq!(bus.stamp_of::<Positions>(), Some(FrameId(2)));
    let mut source = InputSource::new(FrameId(2), &bus, &runtime);
    let inputs = OneView::fetch(&mut source).expect("republished for this frame");
    assert_eq!(inputs.view, &Positions(vec![2.0]));
}

/// A View whose own check finds a problem is not refused and not rewritten:
/// the fetch warns, the value reaches the lane as it was published, and the
/// lane runs on it.
#[test]
fn validate_warns_and_keeps_the_value() {
    let frame = FrameId(3);
    let runtime = Runtime::new();
    let mut bus = LaneBus::new();
    bus.set_frame(frame);
    bus.publish(Speeds {
        values: vec![-1.0, 2.0],
    });
    let warning = InputWarning {
        field: "speeds",
        source: SourceKind::Bus,
        problems: vec![ValueProblem {
            field: "values",
            severity: ProblemSeverity::Warning,
            message: "a speed is never negative (it is -1)".to_owned(),
        }],
    };

    let mut source = InputSource::new(frame, &bus, &runtime);
    let inputs = CheckedSpeeds::fetch(&mut source).expect("a warning does not refuse");
    assert_eq!(
        inputs.speeds.values,
        vec![-1.0, 2.0],
        "the negative speed is kept, not clamped"
    );
    assert_eq!(source.warnings(), std::slice::from_ref(&warning));

    let mut lane = Erased::new(FastestLane);
    let mut deck = OutputDeck::new();
    let mut source = InputSource::new(frame, &bus, &runtime);
    let mut gpu = GpuSetup::headless();
    let run = {
        let mut sink = OutputSink::new(&mut deck);
        lane.run(0, &mut source, &mut sink, &mut gpu)
    };
    assert!(
        matches!(run.outcome, RunOutcome::Recorded(_)),
        "the lane still runs: {:?}",
        run.outcome
    );
    assert_eq!(run.warnings, vec![warning]);
    assert_eq!(
        deck.get::<Fastest>(),
        Some(&Fastest(2.0)),
        "the lane ran on the value as published"
    );
}

// ─── Locks, budget, deck checks ─────────────────────────────────────────────

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use khora_core::lane::io::{Budget, Locked};
use khora_macros::LaneInputs;

use crate::fixtures::{Contacts, Counter, EveryInput, SharedCounter};

/// Two fields locking one runtime entry.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct TwoLocksOfOneCounter<'f> {
    #[locked]
    pub first: Locked<'f, Counter>,
    #[locked]
    pub second: Locked<'f, Counter>,
}

/// The budget alone.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct OnlyBudget {
    #[budget]
    pub budget: Budget,
}

/// A deck slot its own check runs on.
#[derive(LaneInputs)]
pub struct CheckedContacts<'f> {
    #[deck(validate)]
    pub contacts: Option<&'f Contacts>,
}

impl khora_core::lane::guard::Validate for Contacts {
    fn validate(&self) -> Vec<ValueProblem> {
        if self.0 == 0 {
            vec![ValueProblem {
                field: "0",
                severity: ProblemSeverity::Warning,
                message: "a contact batch is never empty".to_owned(),
            }]
        } else {
            Vec::new()
        }
    }
}

/// A runtime holding `counter` as the `Arc<Mutex<Counter>>` a lock reaches,
/// and what `EveryInput` reads besides.
fn runtime_with(counter: &SharedCounter) -> khora_core::Runtime {
    let mut runtime = Runtime::new();
    runtime.resources.insert(MixSettings { gain: 1.0 });
    runtime.resources.insert(Arc::clone(counter));
    runtime
}

/// Runs `fetch` on another thread and waits `patience` for it to return, so a
/// fetch that blocks fails the test instead of hanging it.
fn fetch_within<T: Send + 'static>(
    patience: Duration,
    fetch: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (sender, receiver) = mpsc::channel();
    // Test code may spawn threads for isolation (RULES §5).
    thread::spawn(move || {
        let _ = sender.send(fetch());
    });
    receiver.recv_timeout(patience).ok()
}

/// A lock a panic poisoned is refused, naming the field: the lane does not
/// run on a state the panic left half-written, and the fetch does not panic.
#[test]
fn a_poisoned_lock_is_refused() {
    let counter: SharedCounter = Arc::new(Mutex::new(Counter(0)));
    let poisoner = Arc::clone(&counter);
    let _ = thread::spawn(move || {
        let _guard = poisoner.lock().unwrap();
        panic!("poisons the counter");
    })
    .join();
    assert!(counter.is_poisoned());

    let runtime = runtime_with(&counter);
    let frame = FrameId(1);
    let mut bus = LaneBus::new();
    bus.set_frame(frame);
    bus.publish(Positions(vec![0.0]));
    let mut source = InputSource::new(frame, &bus, &runtime).with_budget(Budget {
        time: Duration::from_millis(1),
    });

    let refusal = EveryInput::fetch(&mut source)
        .err()
        .expect("the counter is poisoned");
    assert_eq!(
        refusal,
        InputRefusal {
            field: "counter",
            source: SourceKind::Locked,
            reason: Reason::LockPoisoned,
        }
    );
}

/// A budget of zero is no budget: the lane is refused rather than run with no
/// time to run in.
#[test]
fn a_zero_budget_is_refused() {
    let bus = LaneBus::new();
    let runtime = Runtime::new();
    for budget in [Some(Budget::default()), None] {
        let source = InputSource::new(FrameId(1), &bus, &runtime);
        let mut source = match budget {
            Some(budget) => source.with_budget(budget),
            None => source,
        };
        let refusal = OnlyBudget::fetch(&mut source).err();
        assert_eq!(
            refusal,
            Some(InputRefusal {
                field: "budget",
                source: SourceKind::Budget,
                reason: Reason::NoBudget,
            }),
            "budget {budget:?}"
        );
    }
}

/// A `#[deck(validate)]` slot's own check warns and keeps the value; a slot
/// no earlier phase wrote is not checked.
#[test]
fn a_deck_check_warns_and_keeps_the_value() {
    let bus = LaneBus::new();
    let runtime = Runtime::new();
    let mut deck = OutputDeck::new();

    let mut source = InputSource::new(FrameId(1), &bus, &runtime).with_deck(&deck);
    let inputs = CheckedContacts::fetch(&mut source).expect("an unwritten slot is not refused");
    assert_eq!(inputs.contacts, None);
    assert!(source.warnings().is_empty(), "nothing to check");

    *deck.slot::<Contacts>() = Contacts(0);
    let mut source = InputSource::new(FrameId(1), &bus, &runtime).with_deck(&deck);
    let inputs = CheckedContacts::fetch(&mut source).expect("a warning does not refuse");
    assert_eq!(inputs.contacts, Some(&Contacts(0)), "kept as written");
    assert_eq!(
        source.warnings(),
        &[InputWarning {
            field: "contacts",
            source: SourceKind::Deck,
            problems: vec![ValueProblem {
                field: "0",
                severity: ProblemSeverity::Warning,
                message: "a contact batch is never empty".to_owned(),
            }],
        }]
    );
}

/// Layer 1 of the guards: a lock is obtained *in time*. A lock another thread
/// holds is refused as `LockTimedOut` once the lane's budget has run out,
/// instead of blocking the lane — and its worker — until the holder lets go.
#[test]
fn a_lock_held_elsewhere_is_refused_in_time() {
    let counter: SharedCounter = Arc::new(Mutex::new(Counter(0)));
    let held = counter.lock().unwrap();

    let fetching = Arc::clone(&counter);
    let outcome = fetch_within(Duration::from_secs(2), move || {
        let runtime = runtime_with(&fetching);
        let frame = FrameId(1);
        let mut bus = LaneBus::new();
        bus.set_frame(frame);
        bus.publish(Positions(vec![0.0]));
        let mut source = InputSource::new(frame, &bus, &runtime).with_budget(Budget {
            time: Duration::from_millis(1),
        });
        EveryInput::fetch(&mut source).err()
    });
    drop(held);

    assert_eq!(
        outcome,
        Some(Some(InputRefusal {
            field: "counter",
            source: SourceKind::Locked,
            reason: Reason::LockTimedOut,
        })),
        "the fetch must give up on a lock held elsewhere (None: it was still blocked after 2 s)"
    );
}

/// Two `#[locked]` fields of one entry lock the same mutex twice on one
/// thread, which `std::sync::Mutex` never returns from. The fetch must
/// refuse (or the derive must reject the struct), not hang the lane.
#[test]
fn two_locks_of_one_entry_do_not_hang_the_fetch() {
    let counter: SharedCounter = Arc::new(Mutex::new(Counter(0)));

    let outcome = fetch_within(Duration::from_secs(2), move || {
        let runtime = runtime_with(&counter);
        let bus = LaneBus::new();
        let mut source = InputSource::new(FrameId(1), &bus, &runtime);
        TwoLocksOfOneCounter::fetch(&mut source).err()
    });

    assert!(
        matches!(outcome, Some(Some(_))),
        "the fetch neither refused nor returned within 2 s: {outcome:?}"
    );
}

// ─── A lock's wait, against the lane's budget ───────────────────────────────

use std::time::Instant;

/// A second runtime entry registered as `Arc<Mutex<Tally>>`, locked.
#[derive(Debug, Default)]
pub struct Tally;

/// Two locks of two different entries.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct CounterAndTally<'f> {
    #[locked]
    pub counter: Locked<'f, Counter>,
    #[locked]
    pub tally: Locked<'f, Tally>,
}

/// The budget bounds the whole fetch, not each lock in turn: a lane whose
/// first lock comes free late in its budget must not then wait a whole budget
/// again for its second. Here the counter is let go after 80 ms of a 100 ms
/// budget and the tally is never let go; the fetch must have given up by the
/// end of the budget.
#[test]
fn several_locks_share_one_budget() {
    let budget = Duration::from_millis(100);
    let counter: SharedCounter = Arc::new(Mutex::new(Counter(0)));
    let tally = Arc::new(Mutex::new(Tally));

    let (held, holding) = mpsc::channel();
    let (holder_counter, holder_tally) = (Arc::clone(&counter), Arc::clone(&tally));
    // Test code may spawn threads for isolation (RULES §5).
    let holder = thread::spawn(move || {
        let counter_guard = holder_counter.lock().unwrap();
        let tally_guard = holder_tally.lock().unwrap();
        held.send(()).unwrap();
        thread::sleep(Duration::from_millis(80));
        drop(counter_guard);
        thread::sleep(Duration::from_millis(600));
        drop(tally_guard);
    });
    holding.recv().unwrap();

    let outcome = fetch_within(Duration::from_secs(2), move || {
        let mut runtime = Runtime::new();
        runtime.resources.insert(counter);
        runtime.resources.insert(tally);
        let bus = LaneBus::new();
        let mut source =
            InputSource::new(FrameId(1), &bus, &runtime).with_budget(Budget { time: budget });
        let started = Instant::now();
        let refusal = CounterAndTally::fetch(&mut source).err();
        (started.elapsed(), refusal)
    });
    holder.join().unwrap();

    let (elapsed, refusal) = outcome.expect("the fetch returned within 2 s");
    assert_eq!(
        refusal,
        Some(InputRefusal {
            field: "tally",
            source: SourceKind::Locked,
            reason: Reason::LockTimedOut,
        })
    );
    assert!(
        elapsed < budget + Duration::from_millis(40),
        "the fetch waited {elapsed:?} on a {budget:?} budget"
    );
}

/// A budget is the lane's share of a frame, whatever its size: a very large
/// one (`Duration::MAX`, "no limit") must not panic the fetch of a lock that
/// is free.
#[test]
fn a_lock_under_an_unbounded_budget_does_not_panic() {
    let counter: SharedCounter = Arc::new(Mutex::new(Counter(0)));
    let runtime = runtime_with(&counter);
    let frame = FrameId(1);
    let mut bus = LaneBus::new();
    bus.set_frame(frame);
    bus.publish(Positions(vec![0.0]));

    let fetched = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut source = InputSource::new(frame, &bus, &runtime).with_budget(Budget {
            time: Duration::MAX,
        });
        EveryInput::fetch(&mut source).map(|inputs| inputs.counter.0)
    }));

    assert!(
        matches!(fetched, Ok(Ok(0))),
        "a free lock under a `Duration::MAX` budget: {:?}",
        fetched.map_err(|panic| panic
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| panic.downcast_ref::<String>().cloned()))
    );
}
