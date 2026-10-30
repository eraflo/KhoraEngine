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

//! No write is ever lost in the page's ticks, whichever path wrote it.
//!
//! Random sequences mix the moves the page ticks already survive — spawns,
//! despawns, a declared component attached and detached, compactions, clock
//! runs, keys newly watched — with writes through every path the `World`
//! writes by: `set_component`, `get_mut`, `get_many_mut`, `query_mut` (a
//! `&mut T` term and an `Option<&mut T>` one), `write_column`,
//! `RowMut::{write_fields, set_field}`, `relayout` and
//! `for_each_soa_column_mut`, between runs and inside them. After every
//! operation they are checked against a model that remembers, for each live
//! component, the tick it was last written at — `last(c, r)`:
//!
//! - **I1** `last(c, r) ≤ σ(c)`;
//! - **I2** `last(c, r) ≤ max(τ(c, r), φ(c))`, τ read as 0 when the column
//!   keeps no row ticks;
//!
//! and beside them: one tick per row, no summary ahead of the clock, and no
//! write moves a page's order version.

use std::collections::HashMap;

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptValue;

use super::page_ticks::{page_of, row_tick, ticks};
use super::runtime_components::{declare, field, layout, slot};
use super::tick_invariants::Rng;
use super::write_stamps::{assign, Glide, Voltage};
use super::{Position, Velocity};
use crate::ecs::{ComponentKey, FieldKind, PackedLayout, SemanticDomain, World};

/// Each live entity, and the tick each of its components was last written
/// at.
type Model = HashMap<EntityId, HashMap<ComponentKey, u64>>;

/// The keys the sequences use: four Rust components over three domains and
/// one declared component.
struct Keys {
    position: ComponentKey,
    velocity: ComponentKey,
    charge: ComponentKey,
    glide: ComponentKey,
    ember: ComponentKey,
}

impl Keys {
    fn all(&self) -> [ComponentKey; 5] {
        [
            self.position,
            self.velocity,
            self.charge,
            self.glide,
            self.ember,
        ]
    }
}

/// `Ember`'s two layouts, switched by each relayout: a field kept by name,
/// one gained or lost.
fn ember_layout(wide: bool) -> PackedLayout {
    let mut fields = vec![field("amount", FieldKind::Int, ScriptValue::Int(0))];
    if wide {
        fields.push(field("heat", FieldKind::Float, ScriptValue::Float(0.0)));
    }
    layout(fields)
}

/// One random sequence and what it knows.
struct Sequence {
    seed: u64,
    rng: Rng,
    world: World,
    keys: Keys,
    model: Model,
    live: Vec<EntityId>,
    /// Whether `Ember` holds its wide layout.
    wide: bool,
}

impl Sequence {
    fn new(seed: u64) -> Self {
        let mut world = World::new();
        world.register_component::<Position>(SemanticDomain::Spatial);
        world.register_component::<Velocity>(SemanticDomain::Spatial);
        world.register_component::<Voltage>(SemanticDomain::Physics);
        world.register_component::<Glide>(SemanticDomain::Render);
        let ember = declare(
            &mut world,
            "Ember",
            SemanticDomain::Script,
            vec![field("amount", FieldKind::Int, ScriptValue::Int(0))],
        );
        let keys = Keys {
            position: ComponentKey::of::<Position>(),
            velocity: ComponentKey::of::<Velocity>(),
            charge: ComponentKey::of::<Voltage>(),
            glide: ComponentKey::of::<Glide>(),
            ember,
        };
        world.observe(keys.position);
        world.observe(keys.glide);
        Self {
            seed,
            rng: Rng::new(seed),
            world,
            keys,
            model: HashMap::new(),
            live: Vec::new(),
            wide: false,
        }
    }

    fn now(&self) -> u64 {
        self.world.current_tick().0
    }

    /// The live entities holding `key`, in a fixed order.
    fn holders(&self, key: ComponentKey) -> Vec<EntityId> {
        self.live
            .iter()
            .copied()
            .filter(|entity| self.model[entity].contains_key(&key))
            .collect()
    }

    fn pick<T: Copy>(&mut self, from: &[T]) -> Option<T> {
        (!from.is_empty()).then(|| from[self.rng.below(from.len())])
    }

    /// Notes `entity`'s `key` written now.
    fn wrote(&mut self, entity: EntityId, key: ComponentKey) {
        let now = self.now();
        if let Some(held) = self.model.get_mut(&entity) {
            held.insert(key, now);
        }
    }

    fn spawn(&mut self, value: i32) {
        let level = i64::from(value);
        let x = value as f32;
        let k = &self.keys;
        let (entity, keys) = match self.rng.below(7) {
            0 => (self.world.spawn(Position(value)), vec![k.position]),
            1 => (
                self.world.spawn((Position(value), Velocity(value))),
                vec![k.position, k.velocity],
            ),
            2 => (
                self.world.spawn((Position(value), Voltage { level })),
                vec![k.position, k.charge],
            ),
            3 => (self.world.spawn(Voltage { level }), vec![k.charge]),
            4 => (
                self.world.spawn((Velocity(value), Glide { x })),
                vec![k.velocity, k.glide],
            ),
            5 => (self.world.spawn(Glide { x }), vec![k.glide]),
            _ => (
                self.world.spawn((
                    Position(value),
                    Velocity(value),
                    Voltage { level },
                    Glide { x },
                )),
                vec![k.position, k.velocity, k.charge, k.glide],
            ),
        };
        let now = self.now();
        self.model
            .insert(entity, keys.into_iter().map(|key| (key, now)).collect());
        self.live.push(entity);
    }

    /// One write through a path chosen at random; what it was.
    fn write(&mut self, value: i32) -> &'static str {
        let [position, velocity, charge, glide, ember] = self.keys.all();
        match self.rng.below(10) {
            0 => {
                let Some(entity) = self.pick(&self.live.clone()) else {
                    return "set_component";
                };
                let held: Vec<ComponentKey> = [position, velocity, charge, glide]
                    .into_iter()
                    .filter(|key| self.model[&entity].contains_key(key))
                    .collect();
                let Some(key) = self.pick(&held) else {
                    return "set_component";
                };
                let world = &mut self.world;
                let written = if key == position {
                    world.set_component(entity, Position(value))
                } else if key == velocity {
                    world.set_component(entity, Velocity(value))
                } else if key == charge {
                    world.set_component(
                        entity,
                        Voltage {
                            level: value.into(),
                        },
                    )
                } else {
                    world.set_component(entity, Glide { x: value as f32 })
                };
                assert!(written, "seed {}: {entity:?} holds {key:?}", self.seed);
                self.wrote(entity, key);
                "set_component"
            }
            1 => {
                let key = [position, velocity, charge][self.rng.below(3)];
                let Some(entity) = self.pick(&self.holders(key)) else {
                    return "get_mut";
                };
                let world = &mut self.world;
                if key == position {
                    world.get_mut::<Position>(entity).expect("a Position").0 = value;
                } else if key == velocity {
                    world.get_mut::<Velocity>(entity).expect("a Velocity").0 = value;
                } else {
                    world.get_mut::<Voltage>(entity).expect("a Voltage").level = value.into();
                }
                self.wrote(entity, key);
                "get_mut"
            }
            2 => {
                let holders = self.holders(position);
                let (Some(a), Some(b)) = (self.pick(&holders), self.pick(&holders)) else {
                    return "get_many_mut";
                };
                if a == b {
                    return "get_many_mut";
                }
                let (write_a, write_b) = (self.rng.below(2) == 0, self.rng.below(2) == 0);
                {
                    let [first, second] = self.world.get_many_mut::<Position, 2>([a, b]);
                    let (first, second) = (first.expect("a Position"), second.expect("a Position"));
                    if write_a {
                        assign(first, Position(value));
                    }
                    if write_b {
                        assign(second, Position(value));
                    }
                }
                if write_a {
                    self.wrote(a, position);
                }
                if write_b {
                    self.wrote(b, position);
                }
                "get_many_mut"
            }
            3 => {
                let Sequence { world, rng, .. } = self;
                let mut written = Vec::new();
                for (entity, item) in world.query_mut::<(EntityId, &mut Position)>() {
                    if rng.below(3) == 0 {
                        assign(item, Position(value));
                        written.push(entity);
                    }
                }
                for entity in written {
                    self.wrote(entity, position);
                }
                "query_mut"
            }
            4 => {
                let Sequence { world, rng, .. } = self;
                let mut written = Vec::new();
                for (entity, item, speed) in
                    world.query_mut::<(EntityId, &mut Voltage, Option<&mut Velocity>)>()
                {
                    if rng.below(2) == 0 {
                        assign(item, Voltage { level: 1 });
                        written.push((entity, charge));
                    }
                    if let Some(speed) = speed {
                        if rng.below(2) == 0 {
                            assign(speed, Velocity(value));
                            written.push((entity, velocity));
                        }
                    }
                }
                for (entity, key) in written {
                    self.wrote(entity, key);
                }
                "query_mut (optional)"
            }
            5 => {
                let level = self
                    .world
                    .components()
                    .vtable(charge)
                    .expect("registered")
                    .columns
                    .field_slot("level")
                    .expect("Voltage has `level`");
                let mut rows = Vec::new();
                for entity in self.holders(charge) {
                    match self.rng.below(4) {
                        0 => rows.push((entity, vec![ScriptValue::Str("x".to_owned())], false)),
                        1 | 2 => rows.push((entity, vec![ScriptValue::Int(value.into())], true)),
                        _ => {}
                    }
                }
                let report = self.world.write_column(
                    charge,
                    &[level],
                    rows.iter()
                        .map(|(entity, values, _)| (*entity, values.as_slice())),
                );
                let accepted = rows.iter().filter(|(.., ok)| *ok).count();
                assert_eq!(report.written, accepted, "seed {}: write_column", self.seed);
                for (entity, _, ok) in rows {
                    if ok {
                        self.wrote(entity, charge);
                    }
                }
                "write_column"
            }
            6 | 7 => {
                let fields = self.rng.below(2) == 0;
                let Some(entity) = self.pick(&self.holders(ember)) else {
                    return "row_mut";
                };
                let amount = slot(&self.world, ember, "amount").expect("Ember has `amount`");
                let accepted = self.rng.below(4) != 0;
                let offered = if accepted {
                    ScriptValue::Int(value.into())
                } else {
                    ScriptValue::Str("lots".to_owned())
                };
                let ok = {
                    let mut row = self.world.row_mut(entity, ember).expect("an Ember row");
                    if fields {
                        row.write_fields(&[(amount, &offered)]).is_ok()
                    } else {
                        row.set_field(amount, &offered).is_ok()
                    }
                };
                assert_eq!(ok, accepted, "seed {}: an int field takes ints", self.seed);
                if ok {
                    self.wrote(entity, ember);
                }
                if fields {
                    "write_fields"
                } else {
                    "set_field"
                }
            }
            8 => {
                self.wide = !self.wide;
                self.world
                    .relayout(ember, ember_layout(self.wide))
                    .expect("Ember relays out");
                for entity in self.holders(ember) {
                    self.wrote(entity, ember);
                }
                "relayout"
            }
            _ => {
                self.world.for_each_soa_column_mut::<Glide>(|column| {
                    for x in column.field_mut(0) {
                        *x += 1.0;
                    }
                });
                for entity in self.holders(glide) {
                    self.wrote(entity, glide);
                }
                "for_each_soa_column_mut"
            }
        }
    }

    /// Every page's order version, by page id.
    fn orders(&self) -> Vec<u64> {
        self.world.storage.pages.iter().map(|p| p.order()).collect()
    }

    /// I1, I2 and the shape of every page's ticks, after operation `step`.
    fn check(&self, step: usize, op: &str) {
        let (seed, world) = (self.seed, &self.world);
        let now = world.current_tick();
        for (page_id, page) in world.storage.pages.iter().enumerate() {
            for &key in &page.keys {
                let Some(column) = page.column_ticks(key) else {
                    assert_eq!(
                        page.row_count(),
                        0,
                        "seed {seed} step {step} ({op}): page {page_id} holds rows but no ticks for {key:?}"
                    );
                    continue;
                };
                assert!(
                    column.summary <= now,
                    "seed {seed} step {step} ({op}): page {page_id} {key:?} summary {:?} ahead of the clock {now:?}",
                    column.summary
                );
                if let Some(rows) = &column.rows {
                    assert_eq!(
                        rows.len(),
                        page.row_count(),
                        "seed {seed} step {step} ({op}): page {page_id} {key:?}: one tick per row"
                    );
                }
            }
        }
        for &entity in &self.live {
            for (&key, &last) in &self.model[&entity] {
                let domain = world.components().vtable(key).expect("registered").domain;
                let (page, row) = page_of(world, entity, domain);
                assert!(
                    page.keys.contains(&key),
                    "seed {seed} step {step} ({op}): {entity:?} lost {key:?}"
                );
                let column = ticks(page, key);
                assert!(
                    last <= column.summary.0,
                    "seed {seed} step {step} ({op}): I1 broken for {entity:?} {key:?}: last {last} > σ {:?}",
                    column.summary
                );
                let covered = row_tick(column, row).max(column.floor.0);
                assert!(
                    last <= covered,
                    "seed {seed} step {step} ({op}): I2 broken for {entity:?} {key:?}: last {last} > max(τ {}, φ {:?})",
                    row_tick(column, row),
                    column.floor
                );
            }
        }
    }

    /// One operation: a move, a run, a key watched, or writes.
    fn step(&mut self, step: usize) {
        let value = step as i32;
        let op = match self.rng.below(16) {
            0..=2 => {
                self.spawn(value);
                "spawn"
            }
            3 if !self.live.is_empty() => {
                let entity = self.live.swap_remove(self.rng.below(self.live.len()));
                assert!(self.world.despawn(entity));
                self.model.remove(&entity);
                "despawn"
            }
            4 => {
                let ember = self.keys.ember;
                let bare: Vec<EntityId> = self
                    .live
                    .iter()
                    .copied()
                    .filter(|entity| !self.model[entity].contains_key(&ember))
                    .collect();
                if let Some(entity) = self.pick(&bare) {
                    self.world
                        .add_runtime_component(entity, ember, &ScriptValue::Unit)
                        .expect("Ember attaches");
                    let now = self.now();
                    if let Some(held) = self.model.get_mut(&entity) {
                        held.insert(ember, now);
                    }
                }
                "attach"
            }
            5 => {
                let ember = self.keys.ember;
                if let Some(entity) = self.pick(&self.holders(ember)) {
                    assert!(self.world.remove_component_by_key(entity, ember));
                    if let Some(held) = self.model.get_mut(&entity) {
                        held.remove(&ember);
                    }
                }
                "detach"
            }
            6 => {
                let budget = if self.rng.below(2) == 0 {
                    1
                } else {
                    usize::MAX
                };
                self.world.run_compaction(budget);
                "compaction"
            }
            7 => {
                for _ in 0..=self.rng.below(3) {
                    self.world.begin_run();
                    self.world.end_run();
                }
                "runs"
            }
            8 => {
                let key = self.keys.all()[self.rng.below(5)];
                self.world.observe(key);
                "observe"
            }
            9..=14 => {
                let before = self.orders();
                let op = self.write(value);
                assert_eq!(
                    self.orders(),
                    before,
                    "seed {} step {step} ({op}): a write moved an order version",
                    self.seed
                );
                op
            }
            _ => {
                let before = self.orders();
                self.world.begin_run();
                for _ in 0..=self.rng.below(3) {
                    self.write(value);
                }
                self.world.end_run();
                assert_eq!(
                    self.orders(),
                    before,
                    "seed {} step {step} (run of writes): a write moved an order version",
                    self.seed
                );
                "run of writes"
            }
        };
        self.check(step, op);
    }
}

fn run_sequence(seed: u64, steps: usize) {
    let mut sequence = Sequence::new(seed);
    for step in 0..steps {
        sequence.step(step);
    }
}

/// I1 and I2 hold after every operation of many random sequences of moves
/// and writes.
#[test]
fn no_sequence_of_writes_breaks_i1_or_i2() {
    for seed in 1..=48u64 {
        run_sequence(seed, 250);
    }
}

/// A long sequence, where pages are recycled, the declared component is
/// relaid out many times and columns go from unwatched to watched halfway
/// through their life.
#[test]
fn a_long_sequence_of_writes_keeps_i1_and_i2() {
    run_sequence(0x5EED_F00D, 3000);
}
