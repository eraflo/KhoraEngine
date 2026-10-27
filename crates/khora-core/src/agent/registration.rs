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
//! A domain registers one agent per family with [`register_agent!`]: its
//! family, its phase, and the lanes it chooses between. The registrations are
//! checked together at boot, before the first frame, by
//! [`check_registrations`].

use std::any::TypeId;
use std::time::Duration;

use super::execution_phase::ExecutionPhase;
use super::gorna::{AgentId, StrategyId};
use super::mode::EngineMode;
use super::timing::AgentImportance;
use crate::gpu::DeviceCaps;
use crate::lane::erased::AnyLane;
use crate::lane::family::Semantics;
use crate::lane::guard::{SinkKind, SourceKind};
use crate::lane::io::FixedStep;
use crate::lane::level::Impact;
use crate::lane::LaneKind;
use crate::runtime::Runtime;

/// One family's agent, as [`register_agent!`] builds it.
///
/// Built in a `const` context, so every field is a constant: the lanes are
/// constructors, run at boot.
pub struct AgentRegistration {
    /// The family's type.
    pub family: TypeId,
    /// The family's [`NAME`](crate::lane::family::LaneFamily::NAME).
    pub family_name: &'static str,
    /// The family's [`KIND`](crate::lane::family::LaneFamily::KIND).
    pub kind: LaneKind,
    /// The family's [`AGENT`](crate::lane::family::LaneFamily::AGENT).
    pub agent: AgentId,
    /// The family's [`SEMANTICS`](crate::lane::family::LaneFamily::SEMANTICS).
    pub semantics: Semantics,
    /// The phase the agent runs in.
    pub phase: ExecutionPhase,
    /// How much the agent's work matters under budget pressure.
    pub importance: AgentImportance,
    /// The agents it runs after within its phase.
    pub after: &'static [AgentId],
    /// The modes it runs in; empty means every mode.
    pub modes: &'static [EngineMode],
    /// The fixed step it runs at, if it does not run every frame.
    pub fixed_step: Option<Duration>,
    /// One constructor per lane of the family, each wrapping its lane in
    /// [`Erased`](crate::lane::erased::Erased).
    pub lanes: &'static [fn() -> Box<dyn AnyLane>],
}

/// Why boot refused the registrations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootError {
    /// Two registrations name the same family.
    TwoAgentsForOneFamily {
        /// The family.
        family: &'static str,
    },
    /// Two families negotiate as the same agent, so they would share its
    /// budget, its cost model and its decisions.
    TwoFamiliesForOneAgent {
        /// The agent both name.
        agent: AgentId,
        /// The family registered first.
        first: &'static str,
        /// The family registered second.
        second: &'static str,
    },
    /// The family's supported lanes offer no level, so it has no row to run.
    NoLevel {
        /// The family.
        family: &'static str,
    },
    /// An `Exact` family offers a level whose impact is not `None`: a cheaper
    /// level would change the simulation's result, not only its cost.
    ResultChangingLevel {
        /// The family.
        family: &'static str,
        /// The level, by name.
        level: &'static str,
    },
    /// A lane declares two locks of one runtime entry, which one fetch could
    /// never take both of.
    SameLockTwice {
        /// The family.
        family: &'static str,
        /// The lane, by type name.
        lane: &'static str,
        /// The second field declaring it.
        field: &'static str,
    },
    /// A family's outputs name one deck slot twice, so every frame would
    /// refuse the second write.
    SlotWrittenTwice {
        /// The family.
        family: &'static str,
        /// The lane, by type name.
        lane: &'static str,
        /// The second field writing it.
        field: &'static str,
    },
    /// The family's phase is not in the order the scheduler runs, so its
    /// agent would never execute.
    PhaseNotRun {
        /// The family.
        family: &'static str,
        /// Its phase.
        phase: ExecutionPhase,
    },
    /// A lane reads the fixed step of a family that runs every frame, which
    /// no agent gives it: it would be refused on every frame.
    StepWithoutFixedStep {
        /// The family.
        family: &'static str,
        /// The lane, by type name.
        lane: &'static str,
        /// The field reading the step.
        field: &'static str,
    },
    /// No lane of the family runs on this device.
    NoSupportedLane {
        /// The family.
        family: &'static str,
    },
    /// Two levels of one family share a strategy id.
    DuplicateStrategyId {
        /// The family.
        family: &'static str,
        /// The id offered twice.
        id: StrategyId,
    },
    /// A lane declares a runtime entry the runtime does not hold.
    UnknownResource {
        /// The family.
        family: &'static str,
        /// The lane, by type name.
        lane: &'static str,
        /// The field declaring it.
        field: &'static str,
        /// The entry's type name.
        resource: &'static str,
    },
    /// A lane reads a deck slot no agent of an earlier phase writes.
    DeckReadWithoutEarlierWriter {
        /// The family.
        family: &'static str,
        /// The lane, by type name.
        lane: &'static str,
        /// The field reading it.
        field: &'static str,
        /// The slot's type name.
        slot: &'static str,
    },
}

impl std::fmt::Display for BootError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TwoAgentsForOneFamily { family } => {
                write!(f, "the family `{family}` is registered twice")
            }
            Self::TwoFamiliesForOneAgent {
                agent,
                first,
                second,
            } => write!(
                f,
                "the families `{first}` and `{second}` both negotiate as {agent:?}"
            ),
            Self::NoLevel { family } => write!(f, "the family `{family}` offers no level"),
            Self::ResultChangingLevel { family, level } => write!(
                f,
                "the family `{family}` is exact, yet its level `{level}` changes the result"
            ),
            Self::SameLockTwice {
                family,
                lane,
                field,
            } => write!(
                f,
                "`{lane}` of `{family}` locks one entry twice (again in `{field}`)"
            ),
            Self::SlotWrittenTwice {
                family,
                lane,
                field,
            } => write!(
                f,
                "`{lane}` of `{family}` writes one deck slot twice (again in `{field}`)"
            ),
            Self::PhaseNotRun { family, phase } => write!(
                f,
                "the family `{family}` runs in the phase {phase}, which the scheduler never runs"
            ),
            Self::StepWithoutFixedStep {
                family,
                lane,
                field,
            } => write!(
                f,
                "`{lane}` of `{family}` reads the fixed step in `{field}`, yet `{family}` runs every frame"
            ),
            Self::NoSupportedLane { family } => {
                write!(f, "no lane of the family `{family}` runs on this device")
            }
            Self::DuplicateStrategyId { family, id } => {
                write!(f, "the family `{family}` offers {id:?} twice")
            }
            Self::UnknownResource {
                family,
                lane,
                field,
                resource,
            } => write!(
                f,
                "`{lane}` of `{family}` reads `{resource}` in `{field}`, which the runtime does not hold"
            ),
            Self::DeckReadWithoutEarlierWriter {
                family,
                lane,
                field,
                slot,
            } => write!(
                f,
                "`{lane}` of `{family}` reads the slot `{slot}` in `{field}`, which no earlier phase writes"
            ),
        }
    }
}

impl std::error::Error for BootError {}

/// Checks every registration against the others, the runtime and the device:
/// one agent per family and one family per agent, every family in a phase that
/// runs, at least one supported lane
/// and one level per family, unique strategy ids within a family, no level of
/// an `Exact` family that changes the result, no lane reading a fixed step its
/// family does not have, no lane locking one entry twice
/// nor a family writing one slot twice,
/// every declared resource and resident held by the runtime, every `#[deck]`
/// read written by an earlier phase.
pub fn check_registrations(
    registrations: &[&AgentRegistration],
    runtime: &Runtime,
    caps: &DeviceCaps,
) -> Result<(), BootError> {
    check_registrations_in(registrations, runtime, caps, ExecutionPhase::DEFAULT_ORDER)
}

/// [`check_registrations`] for a scheduler running `phase_order`: a `#[deck]`
/// read is checked against the order the phases really run in, a game's own
/// phases included. A phase absent from the order never runs, so it writes
/// nothing a reader could see.
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

    for (registration, lanes) in registrations.iter().zip(&menus) {
        let family = registration.family_name;
        if !phase_order.contains(&registration.phase) {
            return Err(BootError::PhaseNotRun {
                family,
                phase: registration.phase,
            });
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
    Ok(())
}

/// Whether a family that runs before `reader` writes the deck slot `slot`.
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
///     modes: [],
///     fixed_step: None,
///     lanes: [SpatialMixingLane],
/// };
/// ```
#[macro_export]
macro_rules! register_agent {
    (
        family: $family:ty,
        phase: $phase:expr,
        importance: $importance:expr,
        after: [$($after:expr),* $(,)?],
        modes: [$($mode:expr),* $(,)?],
        fixed_step: $fixed_step:expr,
        lanes: [$($lane:ty),+ $(,)?] $(,)?
    ) => {{
        $(
            const _: () = $crate::lane::compile_check::lane_of_family::<$family, $lane>();
        )+
        $crate::agent::registration::AgentRegistration {
            family: ::core::any::TypeId::of::<$family>(),
            family_name: <$family as $crate::lane::family::LaneFamily>::NAME,
            kind: <$family as $crate::lane::family::LaneFamily>::KIND,
            agent: <$family as $crate::lane::family::LaneFamily>::AGENT,
            semantics: <$family as $crate::lane::family::LaneFamily>::SEMANTICS,
            phase: $phase,
            importance: $importance,
            after: &[$($after),*],
            modes: &[$($mode),*],
            fixed_step: $fixed_step,
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
