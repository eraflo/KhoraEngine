use khora_engine_core::memory::SaaTrackingAllocator;
use std::alloc::System;

use khora_engine_core::{self, Engine};

#[global_allocator]
static GLOBAL_ALLOCATOR: SaaTrackingAllocator<System> = SaaTrackingAllocator::new(System);

fn main() {
    let mut khora_engine = Engine::new();

    khora_engine.setup();

    khora_engine.run();
}
