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

//! The script pump over several watched roots at once: saves under two
//! roots read in one frame are each recompiled against their own root.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_core::lane::OutputDeck;
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, World};
use khora_io::asset::{AssetChangeEvent, AssetWatch};
use khora_io::script::declarations::declaration_channel;
use khora_io::script::hot_reload::{reload_channel, PendingReloads};

const MAX_WAIT: Duration = Duration::from_secs(10);

fn data_system(name: &str) -> &'static DataSystemRegistration {
    inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == name)
        .unwrap_or_else(|| panic!("`{name}` is registered"))
}

/// Writes `scripts/<module>` under `assets`.
fn script(assets: &Path, module: &str, source: &str) {
    let scripts = assets.join("scripts");
    std::fs::create_dir_all(&scripts).expect("the script root");
    std::fs::write(scripts.join(module), source).expect("writes");
}

fn mentions(events: &[AssetChangeEvent], rel_path: &str) -> bool {
    events.iter().any(|event| event.rel_path == rel_path)
}

/// **Two roots saved in one frame are both recompiled**, each against its
/// own script root: one pump run over both changes sends both modules.
#[test]
fn saves_under_two_roots_read_in_one_frame_are_each_recompiled() {
    let first = tempfile::TempDir::new().expect("a temp dir");
    let second = tempfile::TempDir::new().expect("a temp dir");
    script(first.path(), "left.erg", "behavior Left { int n = 0; }");
    script(second.path(), "right.erg", "behavior Right { int n = 0; }");

    let watch = Arc::new(AssetWatch::default());
    watch.watch(first.path()).expect("watches the first");
    watch.watch(second.path()).expect("watches the second");
    let mut runtime = Runtime::default();
    runtime.resources.insert(Arc::clone(&watch));
    runtime.resources.insert(reload_channel());
    runtime.resources.insert(declaration_channel());
    std::thread::sleep(Duration::from_millis(150));

    script(first.path(), "left.erg", "behavior Left { int n = 1; }");
    script(second.path(), "right.erg", "behavior Right { int n = 1; }");
    // Both saves have come back before the pump runs once.
    let mut heard = Vec::new();
    let deadline = Instant::now() + MAX_WAIT;
    while Instant::now() < deadline
        && !(mentions(&heard, "scripts/left.erg") && mentions(&heard, "scripts/right.erg"))
    {
        heard.extend(watch.poll_for("probe"));
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        mentions(&heard, "scripts/left.erg") && mentions(&heard, "scripts/right.erg"),
        "the premise: both saves came back: {heard:?}"
    );

    (data_system("script_hot_reload").run)(
        &mut World::new(),
        &runtime,
        &mut OutputDeck::new(),
        khora_data::ecs::SystemContext {
            since: khora_data::ecs::Tick(0),
        },
    );

    let mut sent: Vec<String> = runtime
        .resources
        .get::<PendingReloads>()
        .expect("a reload channel")
        .drain()
        .into_iter()
        .map(|reload| reload.module)
        .collect();
    sent.sort();
    assert_eq!(sent, vec!["left.erg".to_owned(), "right.erg".to_owned()]);
}
