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
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Contention;

fn meta(id: AgentId, deps: &[AgentId]) -> WaveMeta {
    WaveMeta {
        id: Some(id),
        hard_dep_targets: deps.to_vec(),
        contention: Contention::none(),
    }
}

#[test]
fn agents_that_declare_nothing_form_one_wave() {
    let metas = vec![
        meta(AgentId::new("audio"), &[]),
        meta(AgentId::new("shadows"), &[]),
    ];
    assert_eq!(partition_waves(&metas, true), vec![vec![0, 1]]);
}

#[test]
fn hard_dep_on_wave_member_splits_it() {
    // The second agent hard-depends on the first, so it must run in
    // a later wave — preserving the dependency ordering.
    let metas = vec![
        meta(AgentId::new("audio"), &[]),
        meta(AgentId::new("shadows"), &[AgentId::new("audio")]),
    ];
    assert_eq!(partition_waves(&metas, true), vec![vec![0], vec![1]]);
}

fn meta_writes(id: AgentId, writes: &[std::any::TypeId]) -> WaveMeta {
    meta_declaring(id, Contention::none().writing_deck(writes.to_vec()))
}

fn meta_declaring(id: AgentId, contention: Contention) -> WaveMeta {
    WaveMeta {
        id: Some(id),
        hard_dep_targets: Vec::new(),
        contention,
    }
}

/// **What splits a wave is what agents declare, and a hard dependency.**
/// Agents whose contentions are disjoint share a wave; two that lock one
/// resource, or write one deck slot, never do; an agent hard-depending on a
/// member of the wave starts the next one. Nothing else breaks a wave: the
/// agent after a split joins the wave it lands in when it contends with
/// nobody there.
#[test]
fn waves_split_on_contention_alone() {
    struct Draws;
    struct Overlays;
    struct Atlas;

    let disjoint = vec![
        meta_declaring(
            AgentId::new("renderer"),
            Contention::none().writing_deck([std::any::TypeId::of::<Draws>()]),
        ),
        meta_declaring(
            AgentId::new("ui"),
            Contention::none().writing_deck([std::any::TypeId::of::<Overlays>()]),
        ),
        meta_declaring(AgentId::new("audio"), Contention::none().reading::<Atlas>()),
    ];
    assert_eq!(
        partition_waves(&disjoint, true),
        vec![vec![0, 1, 2]],
        "disjoint declarations share one wave"
    );

    let one_lock = vec![
        meta_declaring(
            AgentId::new("renderer"),
            Contention::none().locking::<Atlas>(),
        ),
        meta_declaring(
            AgentId::new("ui"),
            Contention::none().writing_deck([std::any::TypeId::of::<Overlays>()]),
        ),
        meta_declaring(AgentId::new("text"), Contention::none().locking::<Atlas>()),
    ];
    assert_eq!(
        partition_waves(&one_lock, true),
        vec![vec![0, 1], vec![2]],
        "a second lock of one resource starts a wave; the agent before it stays"
    );

    let one_slot = vec![
        meta_declaring(
            AgentId::new("renderer"),
            Contention::none().writing_deck([std::any::TypeId::of::<Draws>()]),
        ),
        meta_declaring(
            AgentId::new("grid"),
            Contention::none().writing_deck([std::any::TypeId::of::<Draws>()]),
        ),
    ];
    assert_eq!(
        partition_waves(&one_slot, true),
        vec![vec![0], vec![1]],
        "two writers of one deck slot never share a wave"
    );

    let dependent = vec![
        meta_declaring(
            AgentId::new("renderer"),
            Contention::none().writing_deck([std::any::TypeId::of::<Draws>()]),
        ),
        WaveMeta {
            hard_dep_targets: vec![AgentId::new("renderer")],
            ..meta_declaring(
                AgentId::new("ui"),
                Contention::none().writing_deck([std::any::TypeId::of::<Overlays>()]),
            )
        },
        meta_declaring(AgentId::new("audio"), Contention::none()),
    ];
    assert_eq!(
        partition_waves(&dependent, true),
        vec![vec![0], vec![1, 2]],
        "a hard dependency on a member splits the wave, however disjoint the two are"
    );
}

#[test]
fn disjoint_deck_writes_pass_the_check() {
    // Two agents writing distinct slot types share a wave cleanly.
    let metas = vec![
        meta_writes(AgentId::new("ui"), &[std::any::TypeId::of::<u32>()]),
        meta_writes(AgentId::new("grid"), &[std::any::TypeId::of::<u64>()]),
    ];
    assert_eq!(check_wave_disjoint(&[0, 1], &metas), 0);
}

/// **Two agents that declare disjoint slots share a wave.** The renderer and
/// the UI agent both write in `OUTPUT`; declared and disjoint, they run at
/// once.
///
/// This is also the **only** assertion a too-broad declaration fails. A test
/// that checked collisions alone is satisfied by an agent that declares
/// everything — and over-declaring fails silently in the direction of
/// slowness: nothing breaks, everything serialises, the engine is just
/// slower.
#[test]
fn two_agents_that_declare_disjoint_slots_share_a_wave() {
    struct Draws;
    struct Overlays;

    let metas = vec![
        meta_declaring(
            AgentId::new("renderer"),
            Contention::none().writing_deck([std::any::TypeId::of::<Draws>()]),
        ),
        meta_declaring(
            AgentId::new("ui"),
            Contention::none().writing_deck([std::any::TypeId::of::<Overlays>()]),
        ),
    ];

    assert_eq!(partition_waves(&metas, true), vec![vec![0, 1]]);
}

/// And they are kept apart when they *do* contend: a lock and a read of one
/// resource never share a wave.
#[test]
fn agents_that_contend_are_split_into_separate_waves() {
    struct Atlas;

    let metas = vec![
        meta_declaring(
            AgentId::new("renderer"),
            Contention::none().locking::<Atlas>(),
        ),
        meta_declaring(AgentId::new("ui"), Contention::none().reading::<Atlas>()),
    ];

    assert_eq!(partition_waves(&metas, true), vec![vec![0], vec![1]]);
}

/// Two readers of one resource stay together, which is what makes declaring
/// a read worth doing at all.
#[test]
fn two_readers_of_one_resource_share_a_wave() {
    struct Input;

    let metas = vec![
        meta_declaring(AgentId::new("ui"), Contention::none().reading::<Input>()),
        meta_declaring(
            AgentId::new("script"),
            Contention::none().reading::<Input>(),
        ),
    ];

    assert_eq!(partition_waves(&metas, true), vec![vec![0, 1]]);
}

/// A locked resource is reported, and both agents are named.
#[test]
fn a_contended_resource_is_detected() {
    struct Queue;

    let metas = vec![
        meta_declaring(AgentId::new("ui"), Contention::none().locking::<Queue>()),
        meta_declaring(
            AgentId::new("script"),
            Contention::none().locking::<Queue>(),
        ),
    ];

    assert_eq!(check_wave_disjoint(&[0, 1], &metas), 1);
}

#[test]
fn colliding_deck_writes_are_detected() {
    // Two agents in one wave both declaring the same slot type collide.
    let metas = vec![
        meta_writes(AgentId::new("ui"), &[std::any::TypeId::of::<u32>()]),
        meta_writes(AgentId::new("grid"), &[std::any::TypeId::of::<u32>()]),
    ];
    assert_eq!(check_wave_disjoint(&[0, 1], &metas), 1);
}

/// **With concurrency off, every agent is a wave of its own**, however
/// disjoint their declarations: the serial path is the all-singleton
/// partition.
#[test]
fn without_concurrency_every_agent_is_a_wave_of_its_own() {
    let metas = vec![
        meta(AgentId::new("audio"), &[]),
        meta(AgentId::new("shadows"), &[]),
        meta(AgentId::new("ui"), &[]),
    ];
    assert_eq!(
        partition_waves(&metas, false),
        vec![vec![0], vec![1], vec![2]]
    );
}

/// **An agent that could not be read is always alone**: nothing is known of
/// what it contends for, so it shares no wave, and the agent after it starts
/// a new one.
#[test]
fn an_agent_that_could_not_be_read_is_always_alone() {
    let metas = vec![
        meta(AgentId::new("audio"), &[]),
        WaveMeta {
            id: None,
            hard_dep_targets: Vec::new(),
            contention: Contention::none(),
        },
        meta(AgentId::new("shadows"), &[]),
        meta(AgentId::new("ui"), &[]),
    ];
    assert_eq!(
        partition_waves(&metas, true),
        vec![vec![0], vec![1], vec![2, 3]]
    );
}
