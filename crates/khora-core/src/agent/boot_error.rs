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

//! Why boot refuses a set of registrations.

use super::execution_phase::ExecutionPhase;
use super::gorna::{AgentId, StrategyId};
use crate::lane::guard::{SinkKind, SourceKind};

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
    /// A family registers a fixed step of zero.
    ///
    /// A run at that step advances no simulated time, and the scheduler owes it
    /// no sub-step: a fixed step must be longer than zero.
    ZeroFixedStep {
        /// The family.
        family: &'static str,
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
    /// The registration names a default level no supported lane of the
    /// family offers.
    UnknownDefaultLevel {
        /// The family.
        family: &'static str,
        /// The level it names.
        level: StrategyId,
    },
    /// A lane of a [`ExecutionPhase::BACKGROUND`] family reads a source of one frame.
    ///
    /// A background run spans frames: it may read only `#[take]`,
    /// `#[resource]` and `#[budget]`.
    BackgroundInput {
        /// The family.
        family: &'static str,
        /// The lane, by type name.
        lane: &'static str,
        /// The field reading it.
        field: &'static str,
        /// Where the field reads from.
        source: SourceKind,
    },
    /// A lane of a [`ExecutionPhase::BACKGROUND`] family writes a slot of one frame.
    ///
    /// A background run spans frames: what it hands on is `#[staged]`, kept
    /// in the reserve until a frame's boundary.
    BackgroundOutput {
        /// The family.
        family: &'static str,
        /// The lane, by type name.
        lane: &'static str,
        /// The field writing it.
        field: &'static str,
        /// Where the field writes.
        sink: SinkKind,
    },
    /// A lane of a frame-phase family writes a `#[staged]` output.
    ///
    /// Staging is for runs off the frame; a frame's lane writes the frame's deck.
    StagedInFrame {
        /// The family.
        family: &'static str,
        /// The lane, by type name.
        lane: &'static str,
        /// The field staging it.
        field: &'static str,
    },
    /// Two families take one channel.
    ///
    /// Two `Take` readers would split its requests by timing alone, one of
    /// them off the frame where no wave orders it against the other.
    ChannelTakenTwice {
        /// The channel's type name.
        channel: &'static str,
        /// The family that takes it first, in registration order.
        first: &'static str,
        /// The other.
        second: &'static str,
    },
    /// A `#[take]` field drains a channel no `Take` mailbox of the family declares.
    ///
    /// The [`Reader::Take`](crate::event::Reader::Take) mailbox is what says
    /// the family is the channel's one reader.
    TakeWithoutMailbox {
        /// The family.
        family: &'static str,
        /// The lane, by type name.
        lane: &'static str,
        /// The field taking it.
        field: &'static str,
        /// The channel's type name.
        channel: &'static str,
    },
    /// A family runs `after` an agent no registration negotiates as.
    AfterUnknownFamily {
        /// The family.
        family: &'static str,
        /// The agent it names.
        target: AgentId,
    },
    /// A family names itself in its `after`.
    AfterItself {
        /// The family.
        family: &'static str,
    },
    /// A family at the [`ExecutionPhase::BACKGROUND`] names another in its `after`.
    ///
    /// A background run starts off the frame, when its family has mail: no
    /// `after` orders it.
    AfterAtBackground {
        /// The family.
        family: &'static str,
        /// The family it names.
        target: &'static str,
    },
    /// A family runs `after` one at the [`ExecutionPhase::BACKGROUND`].
    ///
    /// A background run completes no run of the frame, so the family would
    /// be skipped on every frame.
    AfterBackgroundFamily {
        /// The family.
        family: &'static str,
        /// The background family it names.
        target: &'static str,
    },
    /// A fixed-step family runs `after` a once-per-frame one.
    ///
    /// The fixed-step rounds run before the once-per-frame phases, so the
    /// family would be skipped on every frame.
    AfterOncePerFrameFamily {
        /// The fixed-step family.
        family: &'static str,
        /// The once-per-frame family it names.
        target: &'static str,
    },
    /// A family runs `after` one of a later phase of the scheduler's order.
    ///
    /// The later phase has not run when the family's turn comes, so the family
    /// would be skipped on every frame.
    AfterLaterPhase {
        /// The family.
        family: &'static str,
        /// Its phase.
        phase: ExecutionPhase,
        /// The family it names.
        target: &'static str,
        /// The target's phase, later in the order.
        target_phase: ExecutionPhase,
    },
    /// Families of one phase name one another in their `after`, round to the
    /// first: none of them could ever run.
    AfterCycle {
        /// The families of the cycle, each running after the next; the first
        /// is named again last.
        cycle: Vec<&'static str>,
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
            Self::ZeroFixedStep { family } => write!(
                f,
                "the family `{family}` runs at a fixed step of zero; a fixed step must be longer than zero"
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
            Self::UnknownDefaultLevel { family, level } => write!(
                f,
                "the family `{family}` starts on {level:?}, which none of its lanes offers"
            ),
            Self::BackgroundInput {
                family,
                lane,
                field,
                source,
            } => write!(
                f,
                "`{lane}` of `{family}` runs in the background, yet reads `{field}` from {source:?}: a background lane reads only `#[take]`, `#[resource]` and `#[budget]`"
            ),
            Self::BackgroundOutput {
                family,
                lane,
                field,
                sink,
            } => write!(
                f,
                "`{lane}` of `{family}` runs in the background, yet writes `{field}` to {sink:?}: a background lane hands on only `#[staged]` outputs"
            ),
            Self::StagedInFrame {
                family,
                lane,
                field,
            } => write!(
                f,
                "`{lane}` of `{family}` stages `{field}`, yet runs in a frame phase: only a lane at the background stages its outputs"
            ),
            Self::ChannelTakenTwice {
                channel,
                first,
                second,
            } => write!(
                f,
                "`{first}` and `{second}` both take `{channel}`: a channel has one taker"
            ),
            Self::TakeWithoutMailbox {
                family,
                lane,
                field,
                channel,
            } => write!(
                f,
                "`{lane}` of `{family}` takes `{channel}` in `{field}`, which no `Take` mailbox of `{family}` declares"
            ),
            Self::AfterUnknownFamily { family, target } => write!(
                f,
                "the family `{family}` runs after `{target}`, which no registered family negotiates as"
            ),
            Self::AfterItself { family } => {
                write!(f, "the family `{family}` runs after itself")
            }
            Self::AfterAtBackground { family, target } => write!(
                f,
                "the family `{family}` runs at the background, off the frame, where nothing orders it after `{target}`"
            ),
            Self::AfterBackgroundFamily { family, target } => write!(
                f,
                "the family `{family}` runs after `{target}`, which runs at the background and never completes a run of the frame"
            ),
            Self::AfterOncePerFrameFamily { family, target } => write!(
                f,
                "the family `{family}` runs at a fixed step after `{target}`, which runs once per frame, after every fixed step"
            ),
            Self::AfterLaterPhase {
                family,
                phase,
                target,
                target_phase,
            } => write!(
                f,
                "the family `{family}` in {phase} runs after `{target}`, which runs later, in {target_phase}"
            ),
            Self::AfterCycle { cycle } => {
                let names: Vec<String> = cycle.iter().map(|name| format!("`{name}`")).collect();
                write!(
                    f,
                    "the families run after one another in a cycle: {}",
                    names.join(" after ")
                )
            }
        }
    }
}

impl std::error::Error for BootError {}
