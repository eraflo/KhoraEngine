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

//! What a running game spawns prefabs from: the library and catalog the SDK
//! installs over an asset service.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use khora_core::Runtime;
use khora_data::ecs::{Name, Transform, World};
use khora_data::scene::{
    capture_subtree, write_scene_file, CompactEncoding, PrefabCatalog, PrefabLibrary,
};
use khora_io::asset::{AssetIdRegistry, AssetService};
use khora_telemetry::MetricsRegistry;

/// A project folder of its own under the system's temporary folder, holding
/// one prefab — removed when dropped.
struct Project(PathBuf);

impl Project {
    fn with_prefab(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-prefab-library-{name}-{}",
            std::process::id()
        ));
        let assets = root.join("assets");
        std::fs::create_dir_all(&assets).expect("the asset folder");
        let mut world = World::new();
        let entity = world.spawn((Transform::identity(), Name::new(name)));
        world.mark_authored(entity).expect("authored");
        let record = capture_subtree(&world, entity).expect("the prefab captures");
        let file = write_scene_file(&record, &CompactEncoding).expect("the record writes");
        std::fs::write(assets.join(format!("{name}.kprefab")), file.to_bytes())
            .expect("the prefab is written");
        Self(root)
    }

    fn assets(&self) -> Arc<Mutex<AssetService>> {
        let service = AssetService::open_loose_files(
            &self.0,
            &AssetIdRegistry::load(&self.0),
            Arc::new(MetricsRegistry::new()),
        )
        .expect("the project opens");
        Arc::new(Mutex::new(service))
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// **The installed catalog holds the project's prefabs**, and the library
/// reads them.
#[test]
fn installing_over_an_asset_service_publishes_its_prefabs() {
    let project = Project::with_prefab("Crate");
    let assets = project.assets();
    let prefab = *khora_io::serialization::prefab_catalog(&assets.lock().expect("unpoisoned"))
        .iter()
        .next()
        .expect("the project indexes its prefab");
    let mut runtime = Runtime::default();
    runtime.services.insert(Arc::clone(&assets));

    khora_sdk::prefabs::install(&mut runtime, assets);

    let catalog = runtime.resources.get::<PrefabCatalog>().expect("a catalog");
    assert!(catalog.holds(prefab));
    let library = runtime.resources.get::<PrefabLibrary>().expect("a library");
    assert!(
        library.0.prefab(prefab).is_ok(),
        "the library reads the prefab"
    );
}

/// **A runtime with a catalog already keeps it**, refreshed: what an
/// application installed itself is not replaced.
#[test]
fn installing_keeps_a_catalog_already_there() {
    let project = Project::with_prefab("Barrel");
    let assets = project.assets();
    let mut runtime = Runtime::default();
    runtime.services.insert(Arc::clone(&assets));
    let own = PrefabCatalog::new();
    runtime.resources.insert(own.clone());

    khora_sdk::prefabs::install(&mut runtime, assets);

    assert_eq!(
        own.snapshot().len(),
        1,
        "the application's catalog was filled"
    );
}

/// **Refreshing without an asset service changes nothing.**
#[test]
fn refreshing_without_an_asset_service_changes_nothing() {
    let mut runtime = Runtime::default();
    runtime.resources.insert(PrefabCatalog::new());

    khora_sdk::prefabs::refresh(&runtime);

    let catalog = runtime.resources.get::<PrefabCatalog>().expect("a catalog");
    assert!(catalog.snapshot().is_empty());
}

/// **The catalog is filled from the asset service it is given**, as the
/// library reads from it — not from whatever service the runtime happens to
/// hold: one installed over a service the runtime does not keep lists that
/// service's prefabs.
#[test]
fn installing_fills_the_catalog_from_the_given_asset_service() {
    let project = Project::with_prefab("Lamp");
    let assets = project.assets();
    let prefab = *khora_io::serialization::prefab_catalog(&assets.lock().expect("unpoisoned"))
        .iter()
        .next()
        .expect("the project indexes its prefab");
    let mut runtime = Runtime::default();

    khora_sdk::prefabs::install(&mut runtime, assets);

    let library = runtime.resources.get::<PrefabLibrary>().expect("a library");
    assert!(library.0.prefab(prefab).is_ok(), "the library reads it");
    let catalog = runtime.resources.get::<PrefabCatalog>().expect("a catalog");
    assert!(
        catalog.holds(prefab),
        "the library reads a prefab the catalog does not hold: every spawn of it faults"
    );
}
