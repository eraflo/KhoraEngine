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

//! The boot checks of work that spans frames: who takes a channel, what a
//! lane at the background may read and hand on, and that no frame's lane
//! stages an output.

use std::any::TypeId;

use super::registration::{AgentRegistration, BootError};
use crate::event::{Mailbox, Reader};
use crate::lane::erased::AnyLane;
use crate::lane::guard::{SinkKind, SourceKind};

/// Refuses a `#[take]` field no `Take` mailbox of the family declares: the
/// mailbox is what says the family is the one reader draining that channel.
pub(super) fn check_takes(
    registration: &AgentRegistration,
    lanes: &[Box<dyn AnyLane>],
) -> Result<(), BootError> {
    for lane in lanes {
        for input in lane.declaration().inputs() {
            if input.kind != SourceKind::Take {
                continue;
            }
            let declared = inventory::iter::<Mailbox>.into_iter().any(|mailbox| {
                mailbox.family == registration.agent
                    && mailbox.reader == Reader::Take
                    && (mailbox.channel)() == input.ty
            });
            if !declared {
                return Err(BootError::TakeWithoutMailbox {
                    family: registration.family_name,
                    lane: lane.name(),
                    field: input.field,
                    channel: input.type_name,
                });
            }
        }
    }
    Ok(())
}

/// Refuses two families taking one channel: a channel has one taker.
pub(super) fn check_one_taker(
    registrations: &[&AgentRegistration],
    menus: &[Vec<Box<dyn AnyLane>>],
) -> Result<(), BootError> {
    let mut takers: Vec<(TypeId, &'static str, &'static str)> = Vec::new();
    for (registration, lanes) in registrations.iter().zip(menus) {
        for lane in lanes {
            for input in lane.declaration().inputs() {
                if input.kind != SourceKind::Take {
                    continue;
                }
                match takers.iter().find(|(channel, _, _)| *channel == input.ty) {
                    Some((_, _, first)) if *first != registration.family_name => {
                        return Err(BootError::ChannelTakenTwice {
                            channel: input.type_name,
                            first,
                            second: registration.family_name,
                        });
                    }
                    Some(_) => {}
                    None => takers.push((input.ty, input.type_name, registration.family_name)),
                }
            }
        }
    }
    Ok(())
}

/// Refuses a lane of a family in a frame phase staging an output: what a frame
/// hands on goes into its deck, and a staging nobody collects would grow
/// forever.
pub(super) fn check_nothing_staged(
    family: &'static str,
    lanes: &[Box<dyn AnyLane>],
) -> Result<(), BootError> {
    for lane in lanes {
        if let Some(output) = lane
            .declaration()
            .outputs()
            .iter()
            .find(|output| output.kind == SinkKind::Staged)
        {
            return Err(BootError::StagedInFrame {
                family,
                lane: lane.name(),
                field: output.field,
            });
        }
    }
    Ok(())
}

/// Refuses a lane of a background family reading what lives inside one frame,
/// or writing to one: a run spans frames, so it reads only `#[take]`,
/// `#[resource]` and `#[budget]`, and hands on only `#[staged]` outputs.
pub(super) fn check_background(
    family: &'static str,
    lanes: &[Box<dyn AnyLane>],
) -> Result<(), BootError> {
    for lane in lanes {
        let declaration = lane.declaration();
        for input in declaration.inputs() {
            if !matches!(
                input.kind,
                SourceKind::Take | SourceKind::Resource | SourceKind::Budget
            ) {
                return Err(BootError::BackgroundInput {
                    family,
                    lane: lane.name(),
                    field: input.field,
                    source: input.kind,
                });
            }
        }
        for output in declaration.outputs() {
            if output.kind != SinkKind::Staged {
                return Err(BootError::BackgroundOutput {
                    family,
                    lane: lane.name(),
                    field: output.field,
                    sink: output.kind,
                });
            }
        }
    }
    Ok(())
}
