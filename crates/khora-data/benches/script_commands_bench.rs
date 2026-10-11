//! What a script's component write costs at the frame boundary.
//!
//! 10,000 entities each carry a `RigidBody`; one `SetComponent` per entity
//! writes its `mass` — the per-frame work of a script tuning a field on many
//! bodies — against the same writes as one `WriteColumn`.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use khora_core::math::{Quaternion, Vec3};
use khora_core::script::{ColumnBatch, CommandBuffer, ComponentName, ScriptValue, WorldCommand};
use khora_data::ecs::systems::script_commands::apply_all;
use khora_data::ecs::{RigidBody, Transform, World};
use std::hint::black_box;
use std::sync::Arc;

const ENTITIES: usize = 10_000;

fn bench_set_component(c: &mut Criterion) {
    let mut world = World::default();
    let entities: Vec<_> = (0..ENTITIES)
        .map(|_| {
            world.spawn((
                Transform::new(Vec3::ZERO, Quaternion::IDENTITY, Vec3::ONE),
                RigidBody::default(),
            ))
        })
        .collect();
    let mut buffer = CommandBuffer::default();
    for (n, &entity) in entities.iter().enumerate() {
        buffer.push(WorldCommand::SetComponent {
            entity,
            component: ComponentName::new("RigidBody"),
            value: ScriptValue::Struct(vec![("mass".to_owned(), ScriptValue::Float(n as f32))]),
        });
    }

    c.bench_function("SetComponent RigidBody.mass x10k", |b| {
        b.iter_batched(
            || buffer.clone(),
            |buffer| black_box(apply_all(&mut world, &buffer)),
            BatchSize::LargeInput,
        )
    });
}

/// The same writes as one column write: resolved once, applied page by page.
fn bench_write_column(c: &mut Criterion) {
    let mut world = World::default();
    let entities: Vec<_> = (0..ENTITIES)
        .map(|_| {
            world.spawn((
                Transform::new(Vec3::ZERO, Quaternion::IDENTITY, Vec3::ONE),
                RigidBody::default(),
            ))
        })
        .collect();
    let mut batch = ColumnBatch::new(vec!["mass".to_owned()]).expect("one field");
    for (n, &entity) in entities.iter().enumerate() {
        batch
            .push_row(entity, [ScriptValue::Float(n as f32)])
            .expect("one value a row");
    }
    let mut buffer = CommandBuffer::default();
    buffer.push(WorldCommand::WriteColumn {
        component: ComponentName::new("RigidBody"),
        batch: Arc::new(batch),
    });

    c.bench_function("WriteColumn RigidBody.mass x10k", |b| {
        b.iter_batched(
            || buffer.clone(),
            |buffer| black_box(apply_all(&mut world, &buffer)),
            BatchSize::LargeInput,
        )
    });
}

criterion_group!(benches, bench_set_component, bench_write_column);
criterion_main!(benches);
