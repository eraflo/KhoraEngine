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

//! How long a scene of 10 000 entities takes to load: as a compact record
//! (names resolved, fields matched by name) and as a snapshot (positional,
//! bound to this build's schema). The snapshot is what `FastestLoad` writes.

use criterion::{criterion_group, criterion_main, Criterion};
use khora_core::math::Vec3;
use khora_data::ecs::{Name, Transform, World};
use khora_data::scene::snapshot::{prepare_snapshot, write_snapshot};
use khora_data::scene::{
    capture_world, prepare, read_scene_file, write_scene_file, CompactEncoding, Identity,
};
use std::hint::black_box;

/// 10 000 entities, each a transform and a name — the shape most of a scene
/// is made of.
fn scene() -> World {
    let mut world = World::new();
    for i in 0..10_000 {
        let at = i as f32;
        world.spawn((
            Transform::from_translation(Vec3::new(at, at * 0.5, -at)),
            Name::new(format!("Entity {i}")),
        ));
    }
    world
}

fn bench_scene_load(c: &mut Criterion) {
    let world = scene();
    let compact = write_scene_file(&capture_world(&world).expect("captures"), &CompactEncoding)
        .expect("encodes");
    let snapshot = write_snapshot(&world).expect("writes");

    let mut group = c.benchmark_group("Scene load, 10k entities");
    group.sample_size(20);
    group.bench_function("compact record", |b| {
        b.iter(|| {
            let mut target = World::new();
            let record = read_scene_file(black_box(&compact)).expect("decodes");
            let prepared = prepare(&mut target, &record).expect("prepares");
            black_box(prepared.commit(&mut target, Identity::Keep));
        });
    });
    group.bench_function("snapshot", |b| {
        b.iter(|| {
            let mut target = World::new();
            let prepared = prepare_snapshot(&mut target, black_box(&snapshot)).expect("prepares");
            black_box(prepared.commit(&mut target, Identity::Keep));
        });
    });
    group.finish();
}

criterion_group!(benches, bench_scene_load);
criterion_main!(benches);
