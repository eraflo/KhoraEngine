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

use crate::scheduler::waves::{check_wave_disjoint, partition_waves, WaveMeta};
use khora_core::agent::AgentAccess;
use khora_core::agent::Contention;
use khora_core::control::gorna::AgentId;

fn meta(id: AgentId, access: AgentAccess, deps: &[AgentId]) -> WaveMeta {
    WaveMeta {
        id: Some(id),
        access,
        hard_dep_targets: deps.to_vec(),
        contention: Contention::none(),
    }
}

#[test]
fn all_exclusive_are_singletons() {
    let metas = vec![
        meta(AgentId::Physics, AgentAccess::Exclusive, &[]),
        meta(AgentId::Renderer, AgentAccess::Exclusive, &[]),
    ];
    assert_eq!(partition_waves(&metas), vec![vec![0], vec![1]]);
}

#[test]
fn consecutive_isolated_form_one_wave() {
    let metas = vec![
        meta(AgentId::Audio, AgentAccess::Isolated, &[]),
        meta(AgentId::ShadowRenderer, AgentAccess::Isolated, &[]),
    ];
    assert_eq!(partition_waves(&metas), vec![vec![0, 1]]);
}

#[test]
fn exclusive_breaks_the_wave() {
    let metas = vec![
        meta(AgentId::Audio, AgentAccess::Isolated, &[]),
        meta(AgentId::Physics, AgentAccess::Exclusive, &[]),
        meta(AgentId::ShadowRenderer, AgentAccess::Isolated, &[]),
    ];
    assert_eq!(partition_waves(&metas), vec![vec![0], vec![1], vec![2]]);
}

#[test]
fn hard_dep_on_wave_member_splits_it() {
    // The second Isolated agent hard-depends on the first, so it must run in
    // a later wave — preserving the dependency ordering.
    let metas = vec![
        meta(AgentId::Audio, AgentAccess::Isolated, &[]),
        meta(
            AgentId::ShadowRenderer,
            AgentAccess::Isolated,
            &[AgentId::Audio],
        ),
    ];
    assert_eq!(partition_waves(&metas), vec![vec![0], vec![1]]);
}

#[test]
fn shared_world_joins_isolated_agents() {
    // One SharedWorld reader runs alongside any number of Isolated agents.
    let metas = vec![
        meta(AgentId::Renderer, AgentAccess::SharedWorld, &[]),
        meta(AgentId::Audio, AgentAccess::Isolated, &[]),
        meta(AgentId::ShadowRenderer, AgentAccess::Isolated, &[]),
    ];
    assert_eq!(partition_waves(&metas), vec![vec![0, 1, 2]]);
}

/// **This used to assert the opposite**, and the reason it did is the reason
/// it no longer can: two `SharedWorld` agents were split because they "may
/// write shared resources" and nobody could say which. Now they say, so two
/// that declare nothing in common share a wave. What separates them is
/// contention, asserted just below.
#[test]
fn two_shared_world_agents_that_declare_nothing_share_a_wave() {
    let metas = vec![
        meta(AgentId::Renderer, AgentAccess::SharedWorld, &[]),
        meta(AgentId::Overlay, AgentAccess::SharedWorld, &[]),
    ];
    assert_eq!(partition_waves(&metas), vec![vec![0, 1]]);
}

#[test]
fn exclusive_still_breaks_a_mixed_wave() {
    let metas = vec![
        meta(AgentId::Audio, AgentAccess::Isolated, &[]),
        meta(AgentId::Renderer, AgentAccess::SharedWorld, &[]),
        meta(AgentId::Physics, AgentAccess::Exclusive, &[]),
        meta(AgentId::ShadowRenderer, AgentAccess::Isolated, &[]),
    ];
    assert_eq!(partition_waves(&metas), vec![vec![0, 1], vec![2], vec![3]]);
}

fn meta_writes(id: AgentId, access: AgentAccess, writes: &[std::any::TypeId]) -> WaveMeta {
    meta_declaring(id, access, Contention::none().writing_deck(writes.to_vec()))
}

fn meta_declaring(id: AgentId, access: AgentAccess, contention: Contention) -> WaveMeta {
    WaveMeta {
        id: Some(id),
        access,
        hard_dep_targets: Vec::new(),
        contention,
    }
}

#[test]
fn disjoint_deck_writes_pass_the_check() {
    // Two agents writing distinct slot types share a wave cleanly.
    let metas = vec![
        meta_writes(
            AgentId::Ui,
            AgentAccess::SharedWorld,
            &[std::any::TypeId::of::<u32>()],
        ),
        meta_writes(
            AgentId::Overlay,
            AgentAccess::Isolated,
            &[std::any::TypeId::of::<u64>()],
        ),
    ];
    assert_eq!(check_wave_disjoint(&[0, 1], &metas), 0);
}

/// **The gain the whole contract exists for.** `RenderAgent` and `UiAgent`
/// are both `SharedWorld` and both in `OUTPUT`, and a rule that refused a
/// second `SharedWorld` outright kept them apart — not because they contend,
/// but because nobody could say whether they did. Declared and disjoint,
/// they share a wave.
///
/// This is also the **only** assertion a too-broad declaration fails. A test
/// that checked collisions alone is satisfied by an agent that declares
/// everything — and over-declaring fails silently in the direction of
/// slowness: nothing breaks, everything serialises, the engine is just
/// slower.
#[test]
fn two_shared_world_agents_share_a_wave_when_disjoint() {
    struct Draws;
    struct Overlays;

    let metas = vec![
        meta_declaring(
            AgentId::Renderer,
            AgentAccess::SharedWorld,
            Contention::none().writing_deck([std::any::TypeId::of::<Draws>()]),
        ),
        meta_declaring(
            AgentId::Ui,
            AgentAccess::SharedWorld,
            Contention::none().writing_deck([std::any::TypeId::of::<Overlays>()]),
        ),
    ];

    assert_eq!(partition_waves(&metas), vec![vec![0, 1]]);
}

/// And they are kept apart when they *do* contend — the rule that was
/// removed is replaced by one that knows what it is refusing.
#[test]
fn agents_that_contend_are_split_into_separate_waves() {
    struct Atlas;

    let metas = vec![
        meta_declaring(
            AgentId::Renderer,
            AgentAccess::SharedWorld,
            Contention::none().locking::<Atlas>(),
        ),
        meta_declaring(
            AgentId::Ui,
            AgentAccess::Isolated,
            Contention::none().reading::<Atlas>(),
        ),
    ];

    assert_eq!(partition_waves(&metas), vec![vec![0], vec![1]]);
}

/// Two readers of one resource stay together, which is what makes declaring
/// a read worth doing at all.
#[test]
fn two_readers_of_one_resource_share_a_wave() {
    struct Input;

    let metas = vec![
        meta_declaring(
            AgentId::Ui,
            AgentAccess::Isolated,
            Contention::none().reading::<Input>(),
        ),
        meta_declaring(
            AgentId::Script,
            AgentAccess::Isolated,
            Contention::none().reading::<Input>(),
        ),
    ];

    assert_eq!(partition_waves(&metas), vec![vec![0, 1]]);
}

/// A locked resource is reported, and both agents are named.
#[test]
fn a_contended_resource_is_detected() {
    struct Queue;

    let metas = vec![
        meta_declaring(
            AgentId::Ui,
            AgentAccess::Isolated,
            Contention::none().locking::<Queue>(),
        ),
        meta_declaring(
            AgentId::Script,
            AgentAccess::Isolated,
            Contention::none().locking::<Queue>(),
        ),
    ];

    assert_eq!(check_wave_disjoint(&[0, 1], &metas), 1);
}

#[test]
fn colliding_deck_writes_are_detected() {
    // Two agents in one wave both declaring the same slot type collide.
    let metas = vec![
        meta_writes(
            AgentId::Ui,
            AgentAccess::SharedWorld,
            &[std::any::TypeId::of::<u32>()],
        ),
        meta_writes(
            AgentId::Overlay,
            AgentAccess::Isolated,
            &[std::any::TypeId::of::<u32>()],
        ),
    ];
    assert_eq!(check_wave_disjoint(&[0, 1], &metas), 1);
}
