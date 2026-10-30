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

//! No change is ever missed: the theorem `Changed_o(c) ⊆ A_o(c)`, read back
//! through `Changed<T>`.
//!
//! Random sequences mix spawns, despawns, components added and removed
//! (migrations, which leave orphan rows), compactions, clock runs, keys
//! watched late, and writes through `set_component`, `get_mut`,
//! `get_many_mut` and `query_mut` (a `&mut T` term, a joined
//! `Option<&mut T>` one, and `&mut T` filtered by `Changed`), between runs
//! and inside them. A model remembers, for each live component, the tick it
//! was last written at — `last(c, r)`. At random checkpoints a reader asks
//! `Changed<T>` since some tick `s` — alone, through `query_mut`, beside a
//! term of another domain (Transversal), or joined through a foreign
//! `Option` term — and the answer is checked:
//!
//! - **the theorem**: every row written after `s` is in it;
//! - **the rule**: an entity is in it exactly when its column's summary is
//!   past `s` and the column keeps no row ticks, or `s` is under its floor,
//!   or its row tick is past `s` — so it is exact against the model when
//!   `s ≥ φ`, the column keeps row ticks and the row's tick is its `last`;
//! - only live entities holding every required term, each once.
//!
//! Observers read as the runners make them: `since` is the tick of their
//! previous read, each read is a run of its own.

use std::collections::HashMap;

use khora_core::ecs::entity::EntityId;

use super::page_ticks::{page_of, row_tick, ticks};
use super::tick_invariants::Rng;
use super::{Position, RenderId, RenderTag, Velocity};
use crate::ecs::{Changed, Component, ComponentKey, SemanticDomain, Tick, World};

/// The components the sequences use: two Spatial, two Render. `RenderTag`
/// is only ever moved, never read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Part {
    Position,
    Velocity,
    RenderId,
    RenderTag,
}

const PARTS: [Part; 4] = [
    Part::Position,
    Part::Velocity,
    Part::RenderId,
    Part::RenderTag,
];

/// The parts a reader asks `Changed` of.
const READ: [Part; 3] = [Part::Position, Part::Velocity, Part::RenderId];

impl Part {
    fn key(self) -> ComponentKey {
        match self {
            Part::Position => ComponentKey::of::<Position>(),
            Part::Velocity => ComponentKey::of::<Velocity>(),
            Part::RenderId => ComponentKey::of::<RenderId>(),
            Part::RenderTag => ComponentKey::of::<RenderTag>(),
        }
    }

    fn domain(self) -> SemanticDomain {
        match self {
            Part::Position | Part::Velocity => SemanticDomain::Spatial,
            Part::RenderId | Part::RenderTag => SemanticDomain::Render,
        }
    }

    /// A read part of the other domain, for Transversal and joined reads.
    fn foreign(self) -> Part {
        match self.domain() {
            SemanticDomain::Spatial => Part::RenderId,
            _ => Part::Position,
        }
    }

    fn add(self, world: &mut World, entity: EntityId, value: i32) -> bool {
        match self {
            Part::Position => world.add_component(entity, Position(value)).is_ok(),
            Part::Velocity => world.add_component(entity, Velocity(value)).is_ok(),
            Part::RenderId => world.add_component(entity, RenderId(value)).is_ok(),
            Part::RenderTag => world.add_component(entity, RenderTag).is_ok(),
        }
    }

    fn remove(self, world: &mut World, entity: EntityId) -> bool {
        match self {
            Part::Position => world.remove_component::<Position>(entity).is_ok(),
            Part::Velocity => world.remove_component::<Velocity>(entity).is_ok(),
            Part::RenderId => world.remove_component::<RenderId>(entity).is_ok(),
            Part::RenderTag => world.remove_component::<RenderTag>(entity).is_ok(),
        }
    }

    fn set(self, world: &mut World, entity: EntityId, value: i32) -> bool {
        match self {
            Part::Position => world.set_component(entity, Position(value)),
            Part::Velocity => world.set_component(entity, Velocity(value)),
            Part::RenderId => world.set_component(entity, RenderId(value)),
            Part::RenderTag => world.set_component(entity, RenderTag),
        }
    }
}

/// How a reader asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reader {
    /// `query::<(EntityId, Changed<T>)>`.
    Alone,
    /// `query_mut::<(EntityId, Changed<T>)>`.
    AloneMut,
    /// `query::<(EntityId, Changed<T>, &U)>`, `U` in the other domain.
    Transversal,
    /// `query::<(EntityId, Changed<T>, Option<&U>)>`, `U` in the other
    /// domain: a Native iteration joined through the entity's live rows.
    Joined,
}

const READERS: [Reader; 4] = [
    Reader::Alone,
    Reader::AloneMut,
    Reader::Transversal,
    Reader::Joined,
];

fn alone<T: Component>(world: &World, since: u64) -> Vec<EntityId> {
    world
        .query::<(EntityId, Changed<T>)>()
        .since(Tick(since))
        .map(|(entity, ())| entity)
        .collect()
}

fn alone_mut<T: Component>(world: &mut World, since: u64) -> Vec<EntityId> {
    world
        .query_mut::<(EntityId, Changed<T>)>()
        .since(Tick(since))
        .map(|(entity, ())| entity)
        .collect()
}

fn transversal<T: Component, U: Component>(world: &World, since: u64) -> Vec<EntityId> {
    world
        .query::<(EntityId, Changed<T>, &U)>()
        .since(Tick(since))
        .map(|(entity, (), _)| entity)
        .collect()
}

fn joined<T: Component, U: Component>(world: &World, since: u64) -> Vec<EntityId> {
    world
        .query::<(EntityId, Changed<T>, Option<&U>)>()
        .since(Tick(since))
        .map(|(entity, (), _)| entity)
        .collect()
}

/// What `reader` answers for `Changed<part>` since `since`.
fn read(world: &mut World, part: Part, reader: Reader, since: u64) -> Vec<EntityId> {
    match (part, reader) {
        (Part::Position, Reader::Alone) => alone::<Position>(world, since),
        (Part::Position, Reader::AloneMut) => alone_mut::<Position>(world, since),
        (Part::Position, Reader::Transversal) => transversal::<Position, RenderId>(world, since),
        (Part::Position, Reader::Joined) => joined::<Position, RenderId>(world, since),
        (Part::Velocity, Reader::Alone) => alone::<Velocity>(world, since),
        (Part::Velocity, Reader::AloneMut) => alone_mut::<Velocity>(world, since),
        (Part::Velocity, Reader::Transversal) => transversal::<Velocity, RenderId>(world, since),
        (Part::Velocity, Reader::Joined) => joined::<Velocity, RenderId>(world, since),
        (Part::RenderId, Reader::Alone) => alone::<RenderId>(world, since),
        (Part::RenderId, Reader::AloneMut) => alone_mut::<RenderId>(world, since),
        (Part::RenderId, Reader::Transversal) => transversal::<RenderId, Position>(world, since),
        (Part::RenderId, Reader::Joined) => joined::<RenderId, Position>(world, since),
        (Part::RenderTag, _) => unreachable!("RenderTag is never read"),
    }
}

/// Each live entity, and the tick each of its components was last written
/// at.
type Model = HashMap<EntityId, HashMap<Part, u64>>;

/// One random sequence and what it knows.
struct Sequence {
    seed: u64,
    rng: Rng,
    world: World,
    model: Model,
    live: Vec<EntityId>,
    /// Each read part's observer: the tick of its previous read.
    seen: HashMap<Part, u64>,
    /// Reads checked against the model where the answer must be exact.
    exact: usize,
    /// Rows the theorem was checked for: written after `since`, and found.
    caught: usize,
}

impl Sequence {
    fn new(seed: u64) -> Self {
        let mut world = World::new();
        world.register_component::<Position>(SemanticDomain::Spatial);
        world.register_component::<Velocity>(SemanticDomain::Spatial);
        world.register_component::<RenderId>(SemanticDomain::Render);
        world.register_component::<RenderTag>(SemanticDomain::Render);
        world.observe(Part::Position.key());
        Self {
            seed,
            rng: Rng::new(seed),
            world,
            model: HashMap::new(),
            live: Vec::new(),
            seen: READ.iter().map(|&part| (part, 0)).collect(),
            exact: 0,
            caught: 0,
        }
    }

    fn now(&self) -> u64 {
        self.world.current_tick().0
    }

    fn holders(&self, part: Part) -> Vec<EntityId> {
        self.live
            .iter()
            .copied()
            .filter(|entity| self.model[entity].contains_key(&part))
            .collect()
    }

    fn pick<T: Copy>(&mut self, from: &[T]) -> Option<T> {
        (!from.is_empty()).then(|| from[self.rng.below(from.len())])
    }

    fn wrote(&mut self, entity: EntityId, part: Part) {
        let now = self.now();
        if let Some(held) = self.model.get_mut(&entity) {
            held.insert(part, now);
        }
    }

    fn spawn(&mut self, value: i32) {
        let world = &mut self.world;
        let (entity, parts) = match self.rng.below(7) {
            0 => (world.spawn(Position(value)), vec![Part::Position]),
            1 => (
                world.spawn((Position(value), Velocity(value))),
                vec![Part::Position, Part::Velocity],
            ),
            2 => (
                world.spawn((Position(value), RenderId(value))),
                vec![Part::Position, Part::RenderId],
            ),
            3 => (world.spawn(RenderId(value)), vec![Part::RenderId]),
            4 => (
                world.spawn((Velocity(value), RenderId(value), RenderTag)),
                vec![Part::Velocity, Part::RenderId, Part::RenderTag],
            ),
            5 => (world.spawn(Velocity(value)), vec![Part::Velocity]),
            _ => (
                world.spawn((Position(value), Velocity(value), RenderId(value))),
                vec![Part::Position, Part::Velocity, Part::RenderId],
            ),
        };
        let now = self.now();
        self.model
            .insert(entity, parts.into_iter().map(|part| (part, now)).collect());
        self.live.push(entity);
    }

    /// One write through a path chosen at random; what it was.
    fn write(&mut self, value: i32) -> &'static str {
        match self.rng.below(6) {
            0 => {
                let Some(entity) = self.pick(&self.live.clone()) else {
                    return "set_component";
                };
                let held: Vec<Part> = READ
                    .into_iter()
                    .filter(|part| self.model[&entity].contains_key(part))
                    .collect();
                let Some(part) = self.pick(&held) else {
                    return "set_component";
                };
                assert!(part.set(&mut self.world, entity, value));
                self.wrote(entity, part);
                "set_component"
            }
            1 => {
                let part = READ[self.rng.below(READ.len())];
                let Some(entity) = self.pick(&self.holders(part)) else {
                    return "get_mut";
                };
                let world = &mut self.world;
                match part {
                    Part::Position => {
                        world.get_mut::<Position>(entity).expect("a Position").0 = value
                    }
                    Part::Velocity => {
                        world.get_mut::<Velocity>(entity).expect("a Velocity").0 = value
                    }
                    _ => world.get_mut::<RenderId>(entity).expect("a RenderId").0 = value,
                }
                self.wrote(entity, part);
                "get_mut"
            }
            2 => {
                let holders = self.holders(Part::Position);
                let (Some(a), Some(b)) = (self.pick(&holders), self.pick(&holders)) else {
                    return "get_many_mut";
                };
                if a == b {
                    return "get_many_mut";
                }
                let write_b = self.rng.below(2) == 0;
                {
                    let [first, second] = self.world.get_many_mut::<Position, 2>([a, b]);
                    first.expect("a Position").0 = value;
                    if write_b {
                        second.expect("a Position").0 = value;
                    }
                }
                self.wrote(a, Part::Position);
                if write_b {
                    self.wrote(b, Part::Position);
                }
                "get_many_mut"
            }
            3 => {
                let Sequence { world, rng, .. } = self;
                let mut written = Vec::new();
                for (entity, mut item) in world.query_mut::<(EntityId, &mut Position)>() {
                    if rng.below(3) == 0 {
                        item.0 = value;
                        written.push(entity);
                    }
                }
                for entity in written {
                    self.wrote(entity, Part::Position);
                }
                "query_mut"
            }
            4 => {
                let Sequence { world, rng, .. } = self;
                let mut written = Vec::new();
                for (entity, mut render, speed) in
                    world.query_mut::<(EntityId, &mut RenderId, Option<&mut Velocity>)>()
                {
                    if rng.below(2) == 0 {
                        render.0 = value;
                        written.push((entity, Part::RenderId));
                    }
                    if let Some(mut speed) = speed {
                        if rng.below(2) == 0 {
                            speed.0 = value;
                            written.push((entity, Part::Velocity));
                        }
                    }
                }
                for (entity, part) in written {
                    self.wrote(entity, part);
                }
                "query_mut (joined)"
            }
            _ => {
                let since = self.rng.below(self.now() as usize + 1) as u64;
                let Sequence { world, .. } = self;
                let mut written = Vec::new();
                for (entity, mut speed, ()) in world
                    .query_mut::<(EntityId, &mut Velocity, Changed<Position>)>()
                    .since(Tick(since))
                {
                    speed.0 = value;
                    written.push(entity);
                }
                for entity in written {
                    self.wrote(entity, Part::Velocity);
                }
                "query_mut (filtered by Changed)"
            }
        }
    }

    /// Checks what `reader` answered for `Changed<part>` since `since`.
    fn check(&mut self, part: Part, reader: Reader, since: u64, got: &[EntityId], at: &str) {
        let (seed, world) = (self.seed, &self.world);
        let ctx = format!("seed {seed} {at}: Changed<{part:?}> by {reader:?} since {since}");
        let mut unique = got.to_vec();
        unique.sort_by_key(|entity| (entity.index, entity.generation));
        unique.dedup();
        assert_eq!(
            unique.len(),
            got.len(),
            "{ctx}: an entity yielded twice: {got:?}"
        );

        let other = part.foreign();
        let required = |entity: &EntityId| match reader {
            Reader::Transversal => self.model[entity].contains_key(&other),
            _ => true,
        };
        let holders: Vec<EntityId> = self
            .holders(part)
            .into_iter()
            .filter(|entity| required(entity))
            .collect();
        for entity in got {
            assert!(
                holders.contains(entity),
                "{ctx}: {entity:?} yielded, but it is not a live holder of every required term"
            );
        }

        let key = part.key();
        for &entity in &holders {
            let last = self.model[&entity][&part];
            let (page, row) = page_of(world, entity, part.domain());
            let column = ticks(page, key);
            let tau = row_tick(column, row);
            let in_answer = got.contains(&entity);

            if last > since {
                assert!(
                    in_answer,
                    "{ctx}: MISSED {entity:?}, written at {last} (σ {:?}, φ {:?}, τ {tau})",
                    column.summary, column.floor
                );
                self.caught += 1;
            }

            let expected = column.summary.0 > since
                && (column.rows.is_none() || since < column.floor.0 || tau > since);
            assert_eq!(
                in_answer,
                expected,
                "{ctx}: {entity:?} (last {last}, σ {:?}, φ {:?}, τ {tau}, rows kept: {})",
                column.summary,
                column.floor,
                column.rows.is_some()
            );

            if column.rows.is_some() && since >= column.floor.0 && tau == last {
                assert_eq!(
                    in_answer,
                    last > since,
                    "{ctx}: {entity:?} written at {last}: exact here"
                );
                self.exact += 1;
            }
        }
    }

    /// A probe: some reader, some part, some `since` up to now.
    fn probe(&mut self, at: &str) {
        let part = READ[self.rng.below(READ.len())];
        let reader = READERS[self.rng.below(READERS.len())];
        let since = match self.rng.below(4) {
            0 => 0,
            1 => self.now().saturating_sub(1),
            2 => self.now(),
            _ => self.rng.below(self.now() as usize + 1) as u64,
        };
        let got = read(&mut self.world, part, reader, since);
        self.check(part, reader, since, &got, at);
    }

    /// An observer's read, as a runner makes it: a run of its own, since the
    /// tick of its previous read.
    fn observer_read(&mut self, at: &str) {
        let part = READ[self.rng.below(READ.len())];
        let reader = READERS[self.rng.below(READERS.len())];
        let since = self.seen[&part];
        let tick = self.world.begin_run();
        let got = read(&mut self.world, part, reader, since);
        self.world.end_run();
        self.check(part, reader, since, &got, at);
        self.seen.insert(part, tick.0);
    }

    fn step(&mut self, step: usize) {
        let value = step as i32;
        let at = |op: &str| format!("step {step} ({op})");
        match self.rng.below(18) {
            0..=2 => self.spawn(value),
            3 if !self.live.is_empty() => {
                let entity = self.live.swap_remove(self.rng.below(self.live.len()));
                assert!(self.world.despawn(entity));
                self.model.remove(&entity);
            }
            4 | 5 if !self.live.is_empty() => {
                let entity = self.live[self.rng.below(self.live.len())];
                let part = PARTS[self.rng.below(PARTS.len())];
                let had = self.model[&entity].contains_key(&part);
                let added = part.add(&mut self.world, entity, value);
                assert_eq!(added, !had, "seed {}: add {part:?}", self.seed);
                if added {
                    self.wrote(entity, part);
                }
            }
            6 if !self.live.is_empty() => {
                let entity = self.live[self.rng.below(self.live.len())];
                let part = PARTS[self.rng.below(PARTS.len())];
                let had = self.model[&entity].contains_key(&part);
                // An entity is never left with nothing: the last component
                // stays, so it stays readable.
                if had && self.model[&entity].len() > 1 {
                    assert!(part.remove(&mut self.world, entity));
                    if let Some(held) = self.model.get_mut(&entity) {
                        held.remove(&part);
                    }
                }
            }
            7 => {
                let budget = if self.rng.below(2) == 0 {
                    1
                } else {
                    usize::MAX
                };
                self.world.run_compaction(budget);
            }
            8 => {
                for _ in 0..=self.rng.below(3) {
                    self.world.begin_run();
                    self.world.end_run();
                }
            }
            9 => {
                let part = READ[self.rng.below(READ.len())];
                self.world.observe(part.key());
            }
            10..=12 => {
                self.write(value);
            }
            13 => {
                self.world.begin_run();
                for _ in 0..=self.rng.below(3) {
                    self.write(value);
                }
                self.world.end_run();
            }
            14..=15 => self.observer_read(&at("observer")),
            _ => {}
        }
        if self.rng.below(3) == 0 {
            self.probe(&at("probe"));
        }
    }
}

fn run_sequence(seed: u64, steps: usize) -> (usize, usize) {
    let mut sequence = Sequence::new(seed);
    for step in 0..steps {
        sequence.step(step);
    }
    (sequence.exact, sequence.caught)
}

/// **No change is ever missed**, over many random sequences of moves, runs,
/// late watches and writes through every path, read at random checkpoints
/// by every kind of reader — and the answer is exact wherever the rule says
/// it is.
#[test]
fn no_change_is_ever_missed() {
    let (mut exact, mut caught) = (0, 0);
    for seed in 1..=48u64 {
        let (e, c) = run_sequence(seed, 250);
        exact += e;
        caught += c;
    }
    assert!(exact > 500, "the exact case was checked {exact} times only");
    assert!(caught > 500, "the theorem was checked {caught} times only");
}

/// A long sequence, where pages are recycled, orphans pile up between
/// compactions and columns go from unwatched to watched halfway through.
#[test]
fn no_change_is_missed_over_a_long_sequence() {
    let (exact, caught) = run_sequence(0x0B5E_57ED, 3000);
    assert!(exact > 0 && caught > 0, "exact {exact}, caught {caught}");
}
