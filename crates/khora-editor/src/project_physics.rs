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

//! A project's physics settings, read from its `project.json` before the
//! engine registers its families: the engine runs its physics family at their step.

use std::path::Path;
use std::sync::Arc;

use khora_sdk::khora_core::lane::family::LaneFamily;
use khora_sdk::khora_core::physics::PhysicsSettings;
use khora_sdk::khora_core::sync::LockExt;
use khora_sdk::khora_lanes::physics::PhysicsFamily;
use khora_sdk::trust::{FileGuard, ProjectTrust, Refusals, Trust};
use khora_sdk::AgentRegistry;

/// Returns the physics settings of the project at `root`, read at the trust `decided` of it.
///
/// A trusted project's `project.json` is read under its caps; a restricted
/// one's, or one nothing was decided of yet, under the strict caps: the trust
/// Build Game reads it at, so Play steps as the game it builds. Refusals go to
/// `refusals`; see [`project_physics`].
pub(crate) fn physics_at(
    root: &Path,
    decided: Option<ProjectTrust>,
    refusals: Arc<Refusals>,
) -> PhysicsSettings {
    let trust = decided.map_or(Trust::Restricted, ProjectTrust::trust);
    project_physics(&FileGuard::new(root, trust, refusals))
}

/// Sets the running physics family's fixed step to `settings`' step; returns whether it was set.
///
/// The family is found in `registry` by its agent; `false`, nothing set, when
/// the registry holds none or its place runs every frame.
pub(crate) fn apply_physics_step(registry: &AgentRegistry, settings: PhysicsSettings) -> bool {
    let Some(agent) = registry.get_by_id(<PhysicsFamily as LaneFamily>::AGENT) else {
        return false;
    };
    // The agent's lock is let go before its place is taken.
    let place = agent.lock_recovered().place();
    let mut place = place.lock_recovered();
    if place.fixed_step().is_none() {
        return false;
    }
    place.set_fixed_step(Some(settings.step));
    true
}

/// Returns the physics settings of the project `project` guards, from its `project.json`.
///
/// The defaults stand in when the file is missing, refused (reported to the
/// guard's refusals), unreadable or has no `physics` section; a section that
/// does not read (a zero or negative step) is reported and the defaults kept.
pub(crate) fn project_physics(project: &FileGuard) -> PhysicsSettings {
    let Some(project) = crate::project_identity::read_project_json(project) else {
        return PhysicsSettings::default();
    };
    match project.get("physics") {
        None => PhysicsSettings::default(),
        Some(section) => serde_json::from_value(section.clone()).unwrap_or_else(|error| {
            log::warn!("project.json: the `physics` section is ignored: {error}");
            PhysicsSettings::default()
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use khora_sdk::khora_core::lane::family::LaneFamily;
    use khora_sdk::khora_core::physics::PhysicsSettings;
    use khora_sdk::khora_core::sync::LockExt;
    use khora_sdk::khora_lanes::physics::PhysicsFamily;
    use khora_sdk::trust::{FileGuard, ProjectTrust, Refusals, Trust};

    use super::{apply_physics_step, physics_at, project_physics};

    /// 1/128 s: exact in binary, and no default.
    const A_128TH: std::time::Duration = std::time::Duration::from_nanos(7_812_500);

    /// **A project's physics is read at the trust decided of it, as Build Game reads it.**
    ///
    /// A `project.json` over the strict cap: trusted, its step is read;
    /// restricted, or nothing decided yet, it is refused and the defaults kept.
    #[test]
    fn a_projects_physics_is_read_at_the_trust_decided_of_it() {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-physics-at-trust-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the project folder");
        let mut json = br#"{ "physics": { "step": 0.0078125 } }"#.to_vec();
        json.resize((256 << 10) + 1, b' ');
        std::fs::write(root.join("project.json"), json).expect("writes");

        let trusted = physics_at(&root, Some(ProjectTrust::Trusted), Refusals::detached());
        let restricted = physics_at(&root, Some(ProjectTrust::Restricted), Refusals::detached());
        let undecided = physics_at(&root, None, Refusals::detached());
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(trusted.step, A_128TH, "a trusted project's step");
        assert_eq!(restricted, PhysicsSettings::default());
        assert_eq!(undecided, PhysicsSettings::default());
    }

    /// A game that does nothing, for an engine booted headless.
    struct Headless;

    impl khora_sdk::PhaseProvider for Headless {}
    impl khora_sdk::EngineApp for Headless {
        fn window_config() -> khora_sdk::WindowConfig {
            khora_sdk::WindowConfig::default()
        }
        fn new() -> Self {
            Headless
        }
        fn setup(&mut self, _world: &mut khora_sdk::GameWorld, _runtime: &khora_sdk::Runtime) {}
        fn update(&mut self, _world: &mut khora_sdk::GameWorld, _inputs: &[khora_sdk::InputEvent]) {
        }
    }

    /// **A physics step read after boot reaches the running physics family.**
    #[test]
    fn a_physics_step_read_after_boot_reaches_the_running_family() {
        let mut engine = khora_sdk::EngineCore::<Headless>::new();
        engine.bootstrap(Headless, khora_sdk::Runtime::new());
        let registry = (engine.runtime().resources)
            .get::<Arc<std::sync::Mutex<khora_sdk::AgentRegistry>>>()
            .cloned()
            .expect("the engine shares its agent registry");

        let set = apply_physics_step(
            &registry.lock_recovered(),
            PhysicsSettings { step: A_128TH },
        );
        let step = (registry.lock_recovered())
            .get_by_id(<PhysicsFamily as LaneFamily>::AGENT)
            .map(|agent| agent.lock_recovered().place())
            .and_then(|place| place.lock_recovered().fixed_step());
        engine.shutdown();

        assert!(set, "the physics family's step was set");
        assert_eq!(step, Some(A_128TH));
    }

    /// **A project's physics step is read through the guard**: under the
    /// strict cap, a `project.json` too large gives the defaults and is reported.
    #[test]
    fn a_projects_physics_step_is_read_through_the_guard() {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-project-physics-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the project folder");
        let path = root.join("project.json");
        std::fs::write(&path, r#"{ "physics": { "step": 0.0078125 } }"#).expect("writes");
        let refusals = Arc::new(Refusals::new(None, None));
        let strict = FileGuard::new(&root, Trust::Restricted, Arc::clone(&refusals));

        let read = project_physics(&strict);
        assert_ne!(read, PhysicsSettings::default(), "the project's step");
        assert!(refusals.quarantined().is_empty());

        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_len((256 << 10) + 1))
            .expect("the file grows past the strict cap");
        let oversized = project_physics(&strict);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(oversized, PhysicsSettings::default());
        assert!(
            refusals
                .quarantined()
                .iter()
                .any(|refusal| refusal.path.ends_with("project.json")),
            "{:?}",
            refusals.quarantined()
        );
    }
}
