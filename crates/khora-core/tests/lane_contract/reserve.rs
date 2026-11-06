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

//! The reserve: what runs staged waits there, across frames, until a frame's
//! `Boundary` takes it; a newer output of one key replaces a waiting one,
//! which is abandoned and never applied.

use khora_core::lane::{Lease, Reserve, Staging};

use crate::bakery::{abandoned, batches, Abandoned, Loaf, Note};

/// What one run staged: `loaves`, as `(key, batch)`, in order.
fn run_staging(log: &Abandoned, loaves: &[(u32, u32)]) -> Staging {
    let mut staging = Staging::new();
    staging.stage(
        loaves
            .iter()
            .map(|&(key, batch)| Loaf::new(key, batch, log))
            .collect(),
    );
    staging
}

/// **The reserve keeps what it received until it is taken.** Two runs'
/// loaves wait side by side, oldest first; taking them leaves none, and
/// nothing taken was abandoned.
#[test]
fn the_reserve_keeps_what_it_received_until_taken() {
    let log = Abandoned::default();
    let reserve = Reserve::new();

    reserve.receive(run_staging(&log, &[(1, 1)]));
    reserve.receive(run_staging(&log, &[(2, 1), (3, 1)]));
    assert_eq!(reserve.len::<Loaf>(), 3);

    assert_eq!(
        batches(&reserve.take::<Loaf>()),
        vec![(1, 1), (2, 1), (3, 1)]
    );
    assert!(reserve.is_empty::<Loaf>());
    assert!(reserve.take::<Loaf>().is_empty(), "taken once");
    assert!(abandoned(&log).is_empty());
}

/// **A newer output of one key replaces a waiting one, which is
/// abandoned.** Batch 2 of key 1 arrives while batch 1 still waits: batch 1
/// is never applied — it goes through its abandon — and the other key's loaf
/// is untouched.
#[test]
fn a_newer_output_of_one_key_replaces_a_waiting_one_and_abandons_it() {
    let log = Abandoned::default();
    let reserve = Reserve::new();

    reserve.receive(run_staging(&log, &[(1, 1), (2, 1)]));
    reserve.receive(run_staging(&log, &[(1, 2)]));

    let taken = batches(&reserve.take::<Loaf>());
    assert_eq!(taken.len(), 2, "one loaf per key: {taken:?}");
    assert!(
        taken.contains(&(1, 2)) && taken.contains(&(2, 1)),
        "{taken:?}"
    );
    assert!(
        !taken.contains(&(1, 1)),
        "the replaced loaf is never applied"
    );
    assert_eq!(abandoned(&log), vec![(1, 1)]);
}

/// **Within one run, the later output of a key wins**: the run wrote batch 1
/// then batch 2 of key 1; only batch 2 is applied, batch 1 is abandoned.
#[test]
fn within_one_run_the_later_output_of_a_key_wins() {
    let log = Abandoned::default();
    let reserve = Reserve::new();

    reserve.receive(run_staging(&log, &[(1, 1), (1, 2)]));

    assert_eq!(batches(&reserve.take::<Loaf>()), vec![(1, 2)]);
    assert_eq!(abandoned(&log), vec![(1, 1)]);
}

/// **An output already taken is never abandoned**: once a `Boundary` took
/// batch 1 it was applied; batch 2 of the same key arrives after and waits on
/// its own.
#[test]
fn an_output_already_taken_is_never_abandoned() {
    let log = Abandoned::default();
    let reserve = Reserve::new();

    reserve.receive(run_staging(&log, &[(1, 1)]));
    assert_eq!(batches(&reserve.take::<Loaf>()), vec![(1, 1)]);
    reserve.receive(run_staging(&log, &[(1, 2)]));

    assert_eq!(batches(&reserve.take::<Loaf>()), vec![(1, 2)]);
    assert!(abandoned(&log).is_empty());
}

/// **An output that never supersedes is never replaced**: two equal notes
/// both wait, and both are applied.
#[test]
fn an_output_that_never_supersedes_is_never_replaced() {
    let reserve = Reserve::new();
    for _ in 0..2 {
        let mut staging = Staging::new();
        staging.stage(vec![Note(7)]);
        reserve.receive(staging);
    }

    assert_eq!(reserve.take::<Note>(), vec![Note(7), Note(7)]);
}

/// **Each type waits apart**: taking the notes leaves the loaves.
#[test]
fn each_output_type_waits_apart() {
    let log = Abandoned::default();
    let reserve = Reserve::new();
    let mut staging = run_staging(&log, &[(1, 1)]);
    staging.stage(vec![Note(3)]);
    reserve.receive(staging);

    assert_eq!(reserve.take::<Note>(), vec![Note(3)]);
    assert_eq!(reserve.len::<Loaf>(), 1);
}

/// **A clone of the reserve is the same reserve**: the runtime holds one,
/// and what the scheduler hands it is what a `Boundary` system takes.
#[test]
fn a_clone_of_the_reserve_is_the_same_reserve() {
    let log = Abandoned::default();
    let reserve = Reserve::new();
    let held_by_the_runtime = reserve.clone();

    reserve.receive(run_staging(&log, &[(4, 1)]));

    assert_eq!(batches(&held_by_the_runtime.take::<Loaf>()), vec![(4, 1)]);
    assert!(reserve.is_empty::<Loaf>());
}

/// **An empty staging is empty**, and one that staged is not.
#[test]
fn a_staging_says_whether_it_holds_anything() {
    let log = Abandoned::default();
    assert!(Staging::new().is_empty());
    assert!(!run_staging(&log, &[(1, 1)]).is_empty());
}

/// **What the host stages lands at the next frame start, after the runs that
/// ended**: nothing is held before it lands, and once landed it is the later
/// word — a host's loaf of a key replaces the loaf a run staged for it.
#[test]
fn what_the_host_stages_lands_after_the_runs_that_ended() {
    let log = Abandoned::default();
    let reserve = Reserve::new();

    reserve.stage_from_host(run_staging(&log, &[(1, 2)]));
    assert_eq!(
        reserve.len::<Loaf>(),
        0,
        "nothing lands before a frame start"
    );

    reserve.receive(run_staging(&log, &[(1, 1)]));
    reserve.land_from_host();

    assert_eq!(batches(&reserve.take::<Loaf>()), vec![(1, 2)]);
    assert_eq!(
        abandoned(&log),
        vec![(1, 1)],
        "the run's loaf was overtaken"
    );
}

/// **Outputs staged under a lease count only while it holds when the reserve
/// takes them in**: the lease is asked at receipt, not when they were staged;
/// a lease that stopped holding has its outputs abandoned — nothing of them
/// waits, nothing they would have superseded is touched — while what the same
/// run staged without it, or under a lease that holds, is taken in.
#[test]
fn outputs_under_a_lease_that_stopped_holding_are_abandoned_at_receipt() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let log = Abandoned::default();
    let reserve = Reserve::new();
    reserve.receive(run_staging(&log, &[(1, 1)]));

    let open = Arc::new(AtomicBool::new(true));
    let closed_later = {
        let open = Arc::clone(&open);
        Lease::new(move || open.load(Ordering::SeqCst))
    };
    let mut staging = Staging::new();
    staging.stage_while(vec![Loaf::new(1, 2, &log)], closed_later);
    staging.stage(vec![Loaf::new(2, 1, &log)]);
    staging.stage_while(vec![Loaf::new(3, 1, &log)], Lease::new(|| true));

    open.store(false, Ordering::SeqCst);
    reserve.receive(staging);

    assert_eq!(
        batches(&reserve.take::<Loaf>()),
        vec![(1, 1), (2, 1), (3, 1)],
        "the waiting loaf of key 1 kept, the others taken in"
    );
    assert_eq!(abandoned(&log), vec![(1, 2)], "the leased loaf abandoned");
}
