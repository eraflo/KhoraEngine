use khora_engine_core::{self, Engine};

fn main() {
    let mut khora_engine = Engine::new();

    khora_engine.setup();

    khora_engine.run();

    khora_engine.shutdown();
}
