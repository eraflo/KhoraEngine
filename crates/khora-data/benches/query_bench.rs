use criterion::{black_box, criterion_group, criterion_main, Criterion};
use khora_data::ecs::{Component, SemanticDomain, World};

#[derive(Debug, Clone, Copy, Default)]
struct Position(u32);
impl Component for Position {}

#[derive(Debug, Clone, Copy, Default)]
struct RenderTag;
impl Component for RenderTag {}

fn bench_queries(c: &mut Criterion) {
    let mut world = World::default();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<RenderTag>(SemanticDomain::Render);

    // Setup 10,000 entities
    for i in 0..10_000 {
        if i % 2 == 0 {
            // Native-ish (both in same page if we used a single domain,
            // but here transversal because they are in different domains)
            world.spawn((Position(i), RenderTag));
        } else {
            world.spawn(Position(i));
        }
    }

    let mut group = c.benchmark_group("ECS Queries");

    group.bench_function("Transversal Join (Spatial & Render)", |b| {
        b.iter(|| {
            let mut count = 0;
            for (pos, _tag) in world.query::<(&Position, &RenderTag)>() {
                count += pos.0;
                black_box(count);
            }
        });
    });

    group.bench_function("Native (Spatial only)", |b| {
        b.iter(|| {
            let mut count = 0;
            for pos in world.query::<&Position>() {
                count += pos.0;
                black_box(count);
            }
        });
    });

    group.finish();
}

criterion_group!(benches, bench_queries);
criterion_main!(benches);
