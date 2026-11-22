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

//! What boot says about a family's `after`: the scheduler runs a family only
//! once every family it names there completed its run earlier in the same
//! frame, so boot refuses a name that could never have.

use std::any::TypeId;
use std::time::Duration;

use khora_core::agent::gorna::AgentId;
use khora_core::agent::registration::{
    check_registrations, check_registrations_in, AgentRegistration, BootError,
};
use khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::erased::{AnyLane, Erased};
use khora_core::lane::family::Semantics;
use khora_core::Runtime;

use crate::bakery::OffFrameLane;
use crate::fixtures::MixSettings;

const RENDER: AgentId = AgentId::new("render");
const SKY: AgentId = AgentId::new("sky");
const GRID: AgentId = AgentId::new("grid");
const STEER: AgentId = AgentId::new("steer");
const GHOST: AgentId = AgentId::new("ghost");

const STEP: Option<Duration> = Some(Duration::from_millis(8));

/// The families' types, one each, so no two registrations name one family.
struct Render;
struct Sky;
struct Grid;
struct Steer;

/// A lane every phase accepts, the background included: it reads a runtime
/// entry and its budget, and writes nothing.
fn quiet_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(OffFrameLane))
}

/// The registration of family `F`, negotiating as `agent`, in `phase`, run
/// after `after`, at `fixed_step`.
fn family<F: 'static>(
    agent: AgentId,
    phase: ExecutionPhase,
    after: &'static [AgentId],
    fixed_step: Option<Duration>,
) -> AgentRegistration {
    AgentRegistration {
        family: TypeId::of::<F>(),
        family_name: agent.name(),
        agent,
        semantics: Semantics::Presentation,
        phase,
        importance: AgentImportance::Important,
        after,
        fixed_step,
        default_level: None,
        module: module_path!(),
        lanes: &[quiet_lane],
    }
}

fn runtime() -> Runtime {
    let mut runtime = Runtime::new();
    runtime.resources.insert(MixSettings { gain: 1.0 });
    runtime
}

fn check(registrations: &[&AgentRegistration]) -> Result<(), BootError> {
    check_registrations(registrations, &runtime(), &DeviceCaps::default())
}

/// The control case: a family after another of its phase, and one after a
/// family of an earlier phase, boot. The scheduler's completion map spans the
/// frame, so an earlier phase's run counts.
#[test]
fn boot_accepts_an_after_within_the_phase_or_on_an_earlier_one() {
    let render = family::<Render>(RENDER, ExecutionPhase::OUTPUT, &[], None);
    let sky = family::<Sky>(SKY, ExecutionPhase::OUTPUT, &[RENDER], None);
    let steer = family::<Steer>(STEER, ExecutionPhase::OBSERVE, &[], None);
    let grid = family::<Grid>(GRID, ExecutionPhase::FINALIZE, &[STEER, SKY], None);

    assert_eq!(check(&[&render, &sky, &steer, &grid]), Ok(()));
}

/// **An `after` naming no registered family is refused.** The target never
/// completes a run, so the family would be skipped on every frame.
#[test]
fn boot_refuses_an_after_naming_no_registered_family() {
    let sky = family::<Sky>(SKY, ExecutionPhase::OUTPUT, &[GHOST], None);

    let checked = check(&[&sky]);

    assert_eq!(
        checked,
        Err(BootError::AfterUnknownFamily {
            family: "sky",
            target: GHOST,
        })
    );
    let message = checked.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(
        message.contains("`sky`") && message.contains("`ghost`"),
        "{message}"
    );
}

/// **A family after itself is refused.** It would wait on its own run, which
/// never comes first.
#[test]
fn boot_refuses_a_family_after_itself() {
    let sky = family::<Sky>(SKY, ExecutionPhase::OUTPUT, &[SKY], None);

    assert_eq!(
        check(&[&sky]),
        Err(BootError::AfterItself { family: "sky" })
    );
}

/// **An `after` naming a family of a later phase is refused.** That phase has
/// not run when the family's turn comes.
#[test]
fn boot_refuses_an_after_naming_a_family_of_a_later_phase() {
    let steer = family::<Steer>(STEER, ExecutionPhase::OBSERVE, &[RENDER], None);
    let render = family::<Render>(RENDER, ExecutionPhase::OUTPUT, &[], None);

    let checked = check(&[&steer, &render]);

    assert_eq!(
        checked,
        Err(BootError::AfterLaterPhase {
            family: "steer",
            phase: ExecutionPhase::OBSERVE,
            target: "render",
            target_phase: ExecutionPhase::OUTPUT,
        })
    );
    let message = checked.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(
        message.contains("`steer`") && message.contains("`render`"),
        "{message}"
    );
}

/// **A game's own phase is ordered where the scheduler's order puts it.** A
/// game's phases run after `OUTPUT` and before `FINALIZE`: a `FINALIZE` family
/// may run after one of them, an `OUTPUT` family may not.
#[test]
fn an_after_on_a_games_own_phase_follows_the_scheduler_order() {
    let late = ExecutionPhase::custom(6);
    let mut order = ExecutionPhase::DEFAULT_ORDER.to_vec();
    order.insert(5, late);
    let render = family::<Render>(RENDER, late, &[], None);
    let grid = family::<Grid>(GRID, ExecutionPhase::FINALIZE, &[RENDER], None);
    let sky = family::<Sky>(SKY, ExecutionPhase::OUTPUT, &[RENDER], None);

    assert_eq!(
        check_registrations_in(
            &[&render, &grid],
            &runtime(),
            &DeviceCaps::default(),
            &order
        ),
        Ok(())
    );
    assert_eq!(
        check_registrations_in(&[&render, &sky], &runtime(), &DeviceCaps::default(), &order),
        Err(BootError::AfterLaterPhase {
            family: "sky",
            phase: ExecutionPhase::OUTPUT,
            target: "render",
            target_phase: late,
        })
    );
}

/// **A fixed-step family after a once-per-frame one is refused.** The
/// fixed-step rounds run before the once-per-frame phases, so the target has
/// not run at any of the family's steps — even in the same phase.
#[test]
fn boot_refuses_a_fixed_step_family_after_a_once_per_frame_one() {
    let render = family::<Render>(RENDER, ExecutionPhase::TRANSFORM, &[], None);
    let steer = family::<Steer>(STEER, ExecutionPhase::TRANSFORM, &[RENDER], STEP);

    assert_eq!(
        check(&[&render, &steer]),
        Err(BootError::AfterOncePerFrameFamily {
            family: "steer",
            target: "render",
        })
    );
}

/// **A once-per-frame family may run after a fixed-step one of any phase.**
/// Every fixed-step round runs before the once-per-frame phases, and a
/// fixed-step family owed no step counts as done: the scheduler honours it.
#[test]
fn a_once_per_frame_family_after_a_fixed_step_one_of_a_later_phase_boots() {
    let sky = family::<Sky>(SKY, ExecutionPhase::OBSERVE, &[STEER], None);
    let steer = family::<Steer>(STEER, ExecutionPhase::TRANSFORM, &[], STEP);

    assert_eq!(check(&[&sky, &steer]), Ok(()));
}

/// **Two fixed-step families of one phase may order each other.** Each round
/// runs the phase's fixed-step families in their `after` order.
#[test]
fn a_fixed_step_family_after_another_of_its_phase_boots() {
    let steer = family::<Steer>(STEER, ExecutionPhase::TRANSFORM, &[], STEP);
    let render = family::<Render>(RENDER, ExecutionPhase::TRANSFORM, &[STEER], STEP);

    assert_eq!(check(&[&steer, &render]), Ok(()));
}

/// **An `after` naming a family at the background is refused.** A background
/// run is off the frame: it never completes a run the frame could wait on.
#[test]
fn boot_refuses_an_after_naming_a_family_at_the_background() {
    let render = family::<Render>(RENDER, ExecutionPhase::BACKGROUND, &[], None);
    let sky = family::<Sky>(SKY, ExecutionPhase::OUTPUT, &[RENDER], None);

    assert_eq!(
        check(&[&render, &sky]),
        Err(BootError::AfterBackgroundFamily {
            family: "sky",
            target: "render",
        })
    );
}

/// **A family at the background with an `after` is refused.** Its run starts
/// off the frame when it has mail: the order it asks for is never kept.
#[test]
fn boot_refuses_an_after_from_a_family_at_the_background() {
    let render = family::<Render>(RENDER, ExecutionPhase::OUTPUT, &[], None);
    let sky = family::<Sky>(SKY, ExecutionPhase::BACKGROUND, &[RENDER], None);

    assert_eq!(
        check(&[&render, &sky]),
        Err(BootError::AfterAtBackground {
            family: "sky",
            target: "render",
        })
    );
}

/// **Two families of one phase naming each other are refused.** Each waits on
/// the other, so neither would ever run.
#[test]
fn boot_refuses_two_families_of_one_phase_after_each_other() {
    let render = family::<Render>(RENDER, ExecutionPhase::OUTPUT, &[SKY], None);
    let sky = family::<Sky>(SKY, ExecutionPhase::OUTPUT, &[RENDER], None);

    let checked = check(&[&render, &sky]);

    assert_eq!(
        checked,
        Err(BootError::AfterCycle {
            cycle: vec!["render", "sky", "render"],
        })
    );
    let message = checked.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(
        message.contains("`render` after `sky` after `render`"),
        "{message}"
    );
}

/// **A longer cycle is refused, named in full.**
#[test]
fn boot_refuses_a_cycle_through_three_families_of_one_phase() {
    let render = family::<Render>(RENDER, ExecutionPhase::OUTPUT, &[], None);
    let sky = family::<Sky>(SKY, ExecutionPhase::OUTPUT, &[GRID, RENDER], None);
    let grid = family::<Grid>(GRID, ExecutionPhase::OUTPUT, &[STEER], None);
    let steer = family::<Steer>(STEER, ExecutionPhase::OUTPUT, &[SKY], None);

    assert_eq!(
        check(&[&render, &sky, &grid, &steer]),
        Err(BootError::AfterCycle {
            cycle: vec!["sky", "grid", "steer", "sky"],
        })
    );
}

/// **A fixed-step family after a fixed-step one of a later phase is refused.**
/// Each fixed-step round runs phase by phase, so the target has not run when
/// the family's first step comes.
#[test]
fn boot_refuses_a_fixed_step_family_after_a_fixed_step_one_of_a_later_phase() {
    let steer = family::<Steer>(STEER, ExecutionPhase::OBSERVE, &[RENDER], STEP);
    let render = family::<Render>(RENDER, ExecutionPhase::TRANSFORM, &[], STEP);

    assert_eq!(
        check(&[&steer, &render]),
        Err(BootError::AfterLaterPhase {
            family: "steer",
            phase: ExecutionPhase::OBSERVE,
            target: "render",
            target_phase: ExecutionPhase::TRANSFORM,
        })
    );
}
