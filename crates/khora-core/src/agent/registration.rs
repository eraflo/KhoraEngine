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

//! How a family's agent is registered, and what boot checks of all of them.
//!
//! A domain registers one agent per family with `register_agent!`: its
//! family, its phase, and the lanes it chooses between. The registrations are
//! checked together at boot, before the first frame, by
//! [`check_registrations`].

use std::any::TypeId;
use std::time::Duration;

pub use super::boot_error::BootError;
use super::execution_phase::ExecutionPhase;
use super::gorna::{AgentId, StrategyId};
use super::timing::AgentImportance;
use crate::gpu::DeviceCaps;
use crate::lane::erased::AnyLane;
use crate::lane::family::Semantics;
use crate::lane::guard::{SinkKind, SourceKind};
use crate::lane::io::FixedStep;
use crate::lane::level::Impact;
use crate::runtime::Runtime;

/// One family's agent, as `register_agent!` builds it.
///
/// Built in a `const` context, so every field is a constant: the lanes are
/// constructors, run at boot.
pub struct AgentRegistration {
    /// The family's type.
    pub family: TypeId,
    /// The family's [`NAME`](crate::lane::family::LaneFamily::NAME).
    pub family_name: &'static str,
    /// The family's [`AGENT`](crate::lane::family::LaneFamily::AGENT).
    pub agent: AgentId,
    /// The family's [`SEMANTICS`](crate::lane::family::LaneFamily::SEMANTICS).
    pub semantics: Semantics,
    /// The phase the agent runs in.
    pub phase: ExecutionPhase,
    /// How much the agent's work matters under budget pressure.
    pub importance: AgentImportance,
    /// The agents it runs after, each completed earlier in the frame.
    ///
    /// Of its phase or an earlier one, or, for a once-per-frame family,
    /// fixed-step ones of any phase. Boot refuses any other, and a cycle.
    pub after: &'static [AgentId],
    /// The fixed step it runs at, if it does not run every frame.
    pub fixed_step: Option<Duration>,
    /// The level the agent starts on, by strategy id: the first row of its
    /// menu with that id. `None` starts on the menu's first row.
    pub default_level: Option<StrategyId>,
    /// The module the registration is written in (`module_path!()`): where
    /// the family comes from, for tools to group agents by crate.
    pub module: &'static str,
    /// One constructor per lane of the family, each wrapping its lane in
    /// [`Erased`](crate::lane::erased::Erased).
    pub lanes: &'static [fn() -> Box<dyn AnyLane>],
}

/// One family's registration, as submitted to the engine's collection:
/// `inventory::submit! { RegisteredAgent(register_agent! { … }) }`.
pub struct RegisteredAgent(pub AgentRegistration);

inventory::collect!(RegisteredAgent);

/// Returns every registration submitted to the collection, in link order.
pub fn registered_agents() -> Vec<&'static AgentRegistration> {
    inventory::iter::<RegisteredAgent>
        .into_iter()
        .map(|registered| &registered.0)
        .collect()
}

/// Checks every registration against the others, the runtime and the device.
///
/// # Errors
///
/// The first [`BootError`] found: a family or agent registered twice, a
/// phase that never runs, a fixed step of zero, no supported lane or level, a
/// strategy id offered twice, an inexact level of an `Exact` family, a fixed
/// step the family lacks, an entry locked or a slot written twice, a resource or resident
/// the runtime lacks, a `#[deck]` read no earlier phase writes, a default
/// level none of the family's lanes offers, a `#[take]` with no mailbox, a
/// channel taken by two families, a `#[staged]` output in a frame phase, a
/// background lane reading or writing anything but what the background allows,
/// or an `after` the scheduler could never honour: a family unknown, the family
/// itself, one at the background, a once-per-frame family after which a
/// fixed-step one runs, one of a later phase, or a cycle.
pub fn check_registrations(
    registrations: &[&AgentRegistration],
    runtime: &Runtime,
    caps: &DeviceCaps,
) -> Result<(), BootError> {
    check_registrations_in(registrations, runtime, caps, ExecutionPhase::DEFAULT_ORDER)
}

/// Runs [`check_registrations`] for a scheduler running `phase_order`.
///
/// A `#[deck]` read and an `after` are checked against that order, a game's
/// own phases included; a phase absent from it never runs, so writes nothing.
///
/// # Errors
///
/// As [`check_registrations`].
pub fn check_registrations_in(
    registrations: &[&AgentRegistration],
    runtime: &Runtime,
    caps: &DeviceCaps,
    phase_order: &[ExecutionPhase],
) -> Result<(), BootError> {
    let mut families = Vec::with_capacity(registrations.len());
    let mut agents: Vec<(AgentId, &'static str)> = Vec::with_capacity(registrations.len());
    for registration in registrations {
        if families.contains(&registration.family) {
            return Err(BootError::TwoAgentsForOneFamily {
                family: registration.family_name,
            });
        }
        families.push(registration.family);
        if let Some((_, first)) = agents
            .iter()
            .find(|(agent, _)| *agent == registration.agent)
        {
            return Err(BootError::TwoFamiliesForOneAgent {
                agent: registration.agent,
                first,
                second: registration.family_name,
            });
        }
        agents.push((registration.agent, registration.family_name));
    }

    // Each family's lanes that run on this device, built once.
    let menus: Vec<Vec<Box<dyn AnyLane>>> = registrations
        .iter()
        .map(|registration| {
            registration
                .lanes
                .iter()
                .map(|make| make())
                .filter(|lane| lane.supports(caps))
                .collect()
        })
        .collect();

    super::background_checks::check_one_taker(registrations, &menus)?;
    for (registration, lanes) in registrations.iter().zip(&menus) {
        let family = registration.family_name;
        super::background_checks::check_takes(registration, lanes)?;
        let background = registration.phase == ExecutionPhase::BACKGROUND;
        if background {
            super::background_checks::check_background(family, lanes)?;
        } else {
            super::background_checks::check_nothing_staged(family, lanes)?;
        }
        if !background && !phase_order.contains(&registration.phase) {
            return Err(BootError::PhaseNotRun {
                family,
                phase: registration.phase,
            });
        }
        if registration.fixed_step.is_some_and(|step| step.is_zero()) {
            return Err(BootError::ZeroFixedStep { family });
        }
        if lanes.is_empty() {
            return Err(BootError::NoSupportedLane { family });
        }
        let mut ids = Vec::new();
        for level in lanes.iter().flat_map(|lane| lane.levels()) {
            if ids.contains(&level.id) {
                return Err(BootError::DuplicateStrategyId {
                    family,
                    id: level.id,
                });
            }
            if registration.semantics == Semantics::Exact && level.impact != Impact::None {
                return Err(BootError::ResultChangingLevel {
                    family,
                    level: level.name,
                });
            }
            ids.push(level.id);
        }
        if ids.is_empty() {
            return Err(BootError::NoLevel { family });
        }
        if let Some(level) = registration.default_level {
            if !ids.contains(&level) {
                return Err(BootError::UnknownDefaultLevel { family, level });
            }
        }
        for lane in lanes {
            let declaration = lane.declaration();
            let mut slots = Vec::new();
            for output in declaration.outputs() {
                if output.kind == SinkKind::Deck {
                    if slots.contains(&output.ty) {
                        return Err(BootError::SlotWrittenTwice {
                            family,
                            lane: lane.name(),
                            field: output.field,
                        });
                    }
                    slots.push(output.ty);
                }
            }
            let mut locks = Vec::new();
            for input in declaration.inputs() {
                if input.kind == SourceKind::Locked {
                    if locks.contains(&input.ty) {
                        return Err(BootError::SameLockTwice {
                            family,
                            lane: lane.name(),
                            field: input.field,
                        });
                    }
                    locks.push(input.ty);
                }
                if input.kind == SourceKind::Frame
                    && input.ty == TypeId::of::<FixedStep>()
                    && registration.fixed_step.is_none()
                {
                    return Err(BootError::StepWithoutFixedStep {
                        family,
                        lane: lane.name(),
                        field: input.field,
                    });
                }
                match input.kind {
                    SourceKind::Resource | SourceKind::Locked | SourceKind::Resident
                        if !runtime.holds(input.ty) =>
                    {
                        return Err(BootError::UnknownResource {
                            family,
                            lane: lane.name(),
                            field: input.field,
                            resource: input.type_name,
                        });
                    }
                    SourceKind::Deck
                        if !written_earlier(
                            input.ty,
                            registration,
                            registrations,
                            &menus,
                            phase_order,
                        ) =>
                    {
                        return Err(BootError::DeckReadWithoutEarlierWriter {
                            family,
                            lane: lane.name(),
                            field: input.field,
                            slot: input.type_name,
                        });
                    }
                    _ => {}
                }
            }
        }
    }
    super::after_checks::check_after(registrations, phase_order)
}

/// Returns whether a family that runs before `reader` writes the deck slot `slot`.
///
/// The scheduler runs the fixed-step agents' rounds before the once-per-frame
/// phase loop, each round in phase order. So a fixed-step reader sees a
/// fixed-step writer of a strictly earlier phase, and a once-per-frame reader
/// sees any fixed-step writer, or a once-per-frame writer of a strictly
/// earlier phase. Phases are ordered by their place in `phase_order`.
fn written_earlier(
    slot: TypeId,
    reader: &AgentRegistration,
    registrations: &[&AgentRegistration],
    menus: &[Vec<Box<dyn AnyLane>>],
    phase_order: &[ExecutionPhase],
) -> bool {
    let place = |phase: ExecutionPhase| phase_order.iter().position(|p| *p == phase);
    registrations
        .iter()
        .zip(menus)
        .filter(|(writer, _)| {
            let earlier_phase = match (place(writer.phase), place(reader.phase)) {
                (Some(writer), Some(reader)) => writer < reader,
                _ => false,
            };
            match (reader.fixed_step.is_some(), writer.fixed_step.is_some()) {
                (true, true) => earlier_phase,
                (true, false) => false,
                (false, true) => true,
                (false, false) => earlier_phase,
            }
        })
        .any(|(_, lanes)| {
            lanes.iter().any(|lane| {
                lane.declaration()
                    .outputs()
                    .iter()
                    .any(|output| output.kind == SinkKind::Deck && output.ty == slot)
            })
        })
}

/// Builds a family's [`AgentRegistration`], as a constant expression.
///
/// Every lane must name `family` as its `Lane::Family`: the lanes of one agent
/// share its family's outputs, and a lane of another family does not compile.
///
/// ```ignore
/// const AUDIO: AgentRegistration = register_agent! {
///     family: AudioFamily,
///     phase: ExecutionPhase::OUTPUT,
///     importance: AgentImportance::Important,
///     after: [],
///     fixed_step: None,
///     lanes: [SpatialMixingLane],
/// };
/// ```
///
/// A family that starts on a named level, rather than on its menu's first
/// row, says so after `fixed_step`: `default_level: StrategyId::Balanced,`.
#[macro_export]
macro_rules! register_agent {
    (@default_level) => {
        ::core::option::Option::None
    };
    (@default_level $default_level:expr) => {
        ::core::option::Option::Some($default_level)
    };
    (
        family: $family:ty,
        phase: $phase:expr,
        importance: $importance:expr,
        after: [$($after:expr),* $(,)?],
        fixed_step: $fixed_step:expr,
        $(default_level: $default_level:expr,)?
        lanes: [$($lane:ty),+ $(,)?] $(,)?
    ) => {{
        $(
            const _: () = $crate::lane::compile_check::lane_of_family::<$family, $lane>();
        )+
        $crate::agent::registration::AgentRegistration {
            family: ::core::any::TypeId::of::<$family>(),
            family_name: <$family as $crate::lane::family::LaneFamily>::NAME,
            agent: <$family as $crate::lane::family::LaneFamily>::AGENT,
            semantics: <$family as $crate::lane::family::LaneFamily>::SEMANTICS,
            phase: $phase,
            importance: $importance,
            after: &[$($after),*],
            fixed_step: $fixed_step,
            default_level: $crate::register_agent!(@default_level $($default_level)?),
            module: ::core::module_path!(),
            lanes: &[$(
                || -> ::std::boxed::Box<dyn $crate::lane::erased::AnyLane> {
                    ::std::boxed::Box::new($crate::lane::erased::Erased::new(
                        <$lane as ::core::default::Default>::default(),
                    ))
                }
            ),+],
        }
    }};
}
