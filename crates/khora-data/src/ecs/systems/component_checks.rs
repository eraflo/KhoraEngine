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

//! The components' own checks, run on the world as it is now.
//!
//! A component that can hold values the engine cannot use declares a check
//! (`#[component(check = …)]`); its registration carries it. Each tick of
//! [`TickPhase::Maintenance`], after the script commands land, this system
//! runs every registration's check over the world and reports what they find
//! to the shared [`Problems`] as [`ProblemSource::Data`]: one problem per
//! field, located by entity, component and field. The report replaces the
//! previous one, so a fixed value's problem leaves; reporting the same list
//! again changes nothing. A problem appearing or leaving is logged once,
//! never every frame. No value is rewritten.
//!
//! [`Problems`]: khora_core::problems::Problems
//! [`ProblemSource::Data`]: khora_core::problems::ProblemSource::Data
//! [`TickPhase::Maintenance`]: crate::ecs::TickPhase::Maintenance

use crate::ecs::SystemContext;
use std::sync::Arc;

use khora_core::lane::OutputDeck;
use khora_core::problems::{Problem, ProblemSource, Problems};
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, TickPhase, World};
use crate::scene::ComponentRegistration;

/// Runs every registered component check over `world` and reports what they
/// find to the runtime's `Arc<Problems>` under `ProblemSource::Data`. A
/// runtime without `Problems` has nowhere to report: the system returns.
pub(crate) fn component_checks(
    world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    let Some(problems) = runtime.resources.get::<Arc<Problems>>() else {
        return;
    };
    let mut found = Vec::new();
    for registration in inventory::iter::<ComponentRegistration> {
        let Some(check) = registration.check else {
            continue;
        };
        for (entity, problem) in check(world) {
            found.push(Problem {
                severity: problem.severity,
                module: None,
                line: 0,
                column: 0,
                message: problem.message,
                note: None,
                entity: Some(entity),
                behavior: None,
                component: Some(registration.type_name.to_owned()),
                field: Some(problem.field.to_owned()),
            });
        }
    }
    // One order whatever the world's layout, so an unchanged world reports
    // the same list and changes nothing.
    found.sort_by(|a, b| {
        (
            &a.component,
            a.entity.map(|e| (e.index, e.generation)),
            &a.field,
            &a.message,
        )
            .cmp(&(
                &b.component,
                b.entity.map(|e| (e.index, e.generation)),
                &b.field,
                &b.message,
            ))
    });

    let before = problems.of_source(&ProblemSource::Data);
    if before == found {
        return;
    }
    // Logged when a field becomes unusable and when it stops being: a value
    // changing from one unusable number to another is the same problem, not
    // a fix and a new one.
    let place = |p: &Problem| (p.component.clone(), p.entity, p.field.clone());
    for problem in found
        .iter()
        .filter(|p| !before.iter().any(|b| place(b) == place(p)))
    {
        log::warn!("{}", describe(problem));
    }
    for problem in before
        .iter()
        .filter(|p| !found.iter().any(|f| place(f) == place(p)))
    {
        log::info!("fixed: {}", describe(problem));
    }
    problems.report(ProblemSource::Data, found);
}

/// `Camera.z_near on entity 3v0: near must be greater than 0 (it is 0)`.
fn describe(problem: &Problem) -> String {
    format!(
        "{}.{} on entity {}: {}",
        problem.component.as_deref().unwrap_or("?"),
        problem.field.as_deref().unwrap_or("?"),
        problem.entity.map_or_else(
            || "?".to_owned(),
            |e| format!("{}v{}", e.index, e.generation)
        ),
        problem.message
    )
}

inventory::submit! {
    DataSystemRegistration {
        name: "component_checks",
        phase: TickPhase::Maintenance,
        run: component_checks,
        order_hint: 30,
        // After the frame's script writes land: what it checks is the
        // frame's last word.
        runs_after: &["script_commands"],
        observes: &[],
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use khora_core::ecs::entity::EntityId;
    use khora_core::problems::{Problem, ProblemSeverity, ProblemSource, Problems};

    use super::*;
    use crate::ecs::Camera;

    /// A runtime holding a fresh `Arc<Problems>`, and that `Problems`.
    fn wired() -> (Runtime, Arc<Problems>) {
        let problems = Arc::new(Problems::default());
        let mut runtime = Runtime::default();
        runtime.resources.insert(problems.clone());
        (runtime, problems)
    }

    fn run(world: &mut World, runtime: &Runtime) {
        component_checks(
            world,
            runtime,
            &mut OutputDeck::new(),
            crate::ecs::SystemContext {
                since: crate::ecs::Tick(0),
            },
        );
    }

    /// The problems found in the scene's data: no module, a component.
    fn data_problems(problems: &Problems) -> Vec<Problem> {
        problems
            .all()
            .into_iter()
            .filter(|problem| problem.module.is_none() && problem.component.is_some())
            .collect()
    }

    /// A camera whose near plane sits at its eye.
    fn broken_camera() -> Camera {
        Camera::new_perspective(1.0, 0.0, 100.0)
    }

    /// A problem the running instances reported.
    fn running_problem() -> Problem {
        Problem {
            severity: ProblemSeverity::Error,
            module: Some("guard.erg".to_owned()),
            line: 3,
            column: 5,
            message: "faulted".to_owned(),
            note: None,
            entity: None,
            behavior: Some("Guard".to_owned()),
            component: None,
            field: None,
        }
    }

    /// A world with a usable camera and one whose near plane is 0: one
    /// `Data` problem, an error on that camera's `z_near`, naming the value.
    #[test]
    fn component_checks_reports_a_broken_camera_with_its_entity() {
        let (runtime, problems) = wired();
        let mut world = World::new();
        world.spawn(Camera::default_perspective());
        let broken = world.spawn(broken_camera());

        run(&mut world, &runtime);

        let found = data_problems(&problems);
        assert_eq!(found.len(), 1, "{found:?}");
        let problem = &found[0];
        assert_eq!(problem.severity, ProblemSeverity::Error);
        assert_eq!(problem.entity, Some(broken));
        assert_eq!(problem.component.as_deref(), Some("Camera"));
        assert_eq!(problem.field.as_deref(), Some("z_near"));
        assert!(problem.message.contains('0'), "{}", problem.message);
        assert_eq!(
            world.get::<Camera>(broken),
            Some(&broken_camera()),
            "the value is kept"
        );
    }

    /// The near plane moved to 0.1: the next run holds no `Data` problem.
    #[test]
    fn component_checks_clears_the_problem_once_fixed() {
        let (runtime, problems) = wired();
        let mut world = World::new();
        let camera = world.spawn(broken_camera());
        run(&mut world, &runtime);
        assert_eq!(data_problems(&problems).len(), 1, "the premise");

        world.get_mut::<Camera>(camera).expect("the camera").z_near = 0.1;
        run(&mut world, &runtime);

        assert!(problems.all().is_empty(), "{:?}", problems.all());
    }

    /// A despawned broken camera takes its problem with it.
    #[test]
    fn component_checks_forgets_a_despawned_camera() {
        let (runtime, problems) = wired();
        let mut world = World::new();
        let camera = world.spawn(broken_camera());
        run(&mut world, &runtime);
        assert_eq!(data_problems(&problems).len(), 1, "the premise");

        world.despawn(camera);
        run(&mut world, &runtime);

        assert!(problems.all().is_empty(), "{:?}", problems.all());
    }

    /// What another source reported stays, whether the checks find a problem
    /// or clear theirs.
    #[test]
    fn component_checks_leaves_other_sources_alone() {
        let (runtime, problems) = wired();
        problems.report(ProblemSource::Running, vec![running_problem()]);
        let mut world = World::new();
        let camera = world.spawn(broken_camera());

        run(&mut world, &runtime);
        let all = problems.all();
        assert!(all.contains(&running_problem()), "{all:?}");
        assert_eq!(all.len(), 2, "{all:?}");

        world.get_mut::<Camera>(camera).expect("the camera").z_near = 0.1;
        run(&mut world, &runtime);
        assert_eq!(problems.all(), vec![running_problem()]);
    }

    /// The same world checked again reports the same list: nothing changes,
    /// and the generation a tool polls stays put.
    #[test]
    fn component_checks_on_an_unchanged_world_changes_nothing() {
        let (runtime, problems) = wired();
        let mut world = World::new();
        world.spawn(broken_camera());
        run(&mut world, &runtime);
        let generation = problems.generation();
        assert_ne!(generation, 0, "the premise: a first report");

        run(&mut world, &runtime);
        run(&mut world, &runtime);

        assert_eq!(problems.generation(), generation);
        assert_eq!(data_problems(&problems).len(), 1);
    }

    /// A clean world reports nothing.
    #[test]
    fn component_checks_of_a_clean_world_reports_nothing() {
        let (runtime, problems) = wired();
        let mut world = World::new();
        world.spawn(Camera::default_perspective());
        world.spawn(Camera::default_orthographic());

        run(&mut world, &runtime);

        assert!(problems.all().is_empty(), "{:?}", problems.all());
        assert_eq!(problems.generation(), 0);
    }

    /// No `Arc<Problems>` in the runtime: the checks have nowhere to report,
    /// and the system returns without panicking, the world untouched.
    #[test]
    fn a_world_without_problems_resource_is_fine() {
        let runtime = Runtime::default();
        let mut world = World::new();
        let camera = world.spawn(broken_camera());

        run(&mut world, &runtime);

        assert_eq!(world.get::<Camera>(camera), Some(&broken_camera()));
    }

    /// The checks are a `Maintenance` data system that runs after the script
    /// commands land, so what a script wrote this frame is what is checked.
    #[test]
    fn component_checks_run_at_maintenance_after_the_script_commands() {
        let system = inventory::iter::<crate::ecs::DataSystemRegistration>
            .into_iter()
            .find(|system| system.name == "component_checks")
            .expect("a `component_checks` data system is registered");

        assert_eq!(system.phase, crate::ecs::TickPhase::Maintenance);
        assert!(
            system.runs_after.contains(&"script_commands"),
            "runs after the script commands: {:?}",
            system.runs_after
        );
    }

    /// Two broken cameras: each problem carries its own entity.
    #[test]
    fn component_checks_locates_each_problem_on_its_own_entity() {
        let (runtime, problems) = wired();
        let mut world = World::new();
        let first = world.spawn(broken_camera());
        let second = world.spawn(broken_camera());

        run(&mut world, &runtime);

        let mut entities: Vec<EntityId> = data_problems(&problems)
            .into_iter()
            .filter_map(|problem| problem.entity)
            .collect();
        entities.sort_by_key(|e| (e.index, e.generation));
        let mut expected = vec![first, second];
        expected.sort_by_key(|e| (e.index, e.generation));
        assert_eq!(entities, expected);
    }

    /// A light checks itself too: a spot light whose range is 0 is one
    /// `Data` error on that light's `light_type.Spot.range`, its value kept.
    #[test]
    fn a_light_problem_reaches_the_problems_list() {
        use crate::ecs::Light;
        use khora_core::renderer::light::{LightType, SpotLight};

        let (runtime, problems) = wired();
        let mut world = World::new();
        world.spawn(Light::spot());
        let unlit = Light::new(LightType::Spot(SpotLight {
            range: 0.0,
            ..SpotLight::default()
        }));
        let broken = world.spawn(unlit.clone());

        run(&mut world, &runtime);

        let found = problems.of_source(&ProblemSource::Data);
        assert_eq!(found.len(), 1, "{found:?}");
        let problem = &found[0];
        assert_eq!(problem.severity, ProblemSeverity::Error);
        assert_eq!(problem.entity, Some(broken));
        assert_eq!(problem.component.as_deref(), Some("Light"));
        assert_eq!(problem.field.as_deref(), Some("light_type.Spot.range"));
        assert_eq!(
            world.get::<Light>(broken).map(|light| light.light_type),
            Some(unlit.light_type),
            "the value is kept"
        );
    }

    /// A disabled light shines nothing, but its values are still broken —
    /// enabling it would not light: a disabled spot whose range is 0 is still
    /// one `Data` problem, on that light's range.
    #[test]
    fn a_disabled_broken_light_is_still_reported() {
        use crate::ecs::Light;
        use khora_core::renderer::light::{LightType, SpotLight};

        let (runtime, problems) = wired();
        let mut world = World::new();
        let mut unlit = Light::new(LightType::Spot(SpotLight {
            range: 0.0,
            ..SpotLight::default()
        }));
        unlit.enabled = false;
        let broken = world.spawn(unlit);

        run(&mut world, &runtime);

        let found = problems.of_source(&ProblemSource::Data);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].severity, ProblemSeverity::Error);
        assert_eq!(found[0].entity, Some(broken));
        assert_eq!(found[0].component.as_deref(), Some("Light"));
        assert_eq!(found[0].field.as_deref(), Some("light_type.Spot.range"));
    }
}
