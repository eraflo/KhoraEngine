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

//! The frame's target and clear colour reach a lane from the frame's
//! `FrameContext`: a lane declaring either reads that context, so its agent
//! may lend them; a lane declaring neither does not, and its wave is not
//! serialised behind every writer of the frame context for nothing.

use std::any::TypeId;
use std::sync::Arc;

use khora_core::lane::io::{Declaration, FixedStep, LaneInputs, Target};
use khora_core::lane::ClearColor;
use khora_core::renderer::api::frame::FrameContext;
use khora_macros::LaneInputs;

use crate::fixtures::{MixSettings, Positions};

/// What a lane drawing into the frame's target reads.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct DrawsIntoTheTarget<'f> {
    #[bus]
    pub view: &'f Positions,
    #[frame]
    pub target: Target,
}

/// What a lane clearing to the frame's colour reads.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct ClearsToTheFrameColour {
    #[frame]
    pub clear: ClearColor,
}

/// What a lane reading both the frame's target and its clear colour reads.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct DrawsAndClears {
    #[frame]
    pub target: Target,
    #[frame]
    pub clear: ClearColor,
}

/// What a lane with no frame input reads.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct NoFrameInput<'f> {
    #[bus]
    pub view: &'f Positions,
    #[resource]
    pub settings: &'f MixSettings,
}

/// What a fixed-step lane reads: the step its own agent hands it.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct StepsOnly {
    #[frame]
    pub step: FixedStep,
}

/// The type a lane reading the frame context declares it reads.
fn frame_context() -> TypeId {
    TypeId::of::<Arc<FrameContext>>()
}

fn declared<'f, I: LaneInputs<'f>>() -> Declaration {
    let mut declaration = Declaration::default();
    I::declare(&mut declaration);
    declaration
}

/// **The frame target reaches a lane that declares it.** A lane naming
/// `#[frame] Target` contends for a read of the `FrameContext` its target is
/// published in — a read, never a lock, so it shares a wave with every other
/// reader of the frame.
#[test]
fn a_frame_target_input_reads_the_frame_context() {
    let contention = declared::<DrawsIntoTheTarget<'static>>().contention();

    assert!(
        contention.reads.contains(&frame_context()),
        "the frame context is read: {:?}",
        contention.reads
    );
    assert!(
        !contention.writes.contains(&frame_context()),
        "and not locked: {:?}",
        contention.writes
    );
}

/// A lane naming `#[frame] ClearColor` reads the frame context too: the clear
/// colour is published beside the target.
#[test]
fn a_frame_clear_colour_input_reads_the_frame_context() {
    let contention = declared::<ClearsToTheFrameColour>().contention();

    assert!(
        contention.reads.contains(&frame_context()),
        "the frame context is read: {:?}",
        contention.reads
    );
}

/// Naming both the target and the clear colour reads the frame context once.
#[test]
fn the_target_and_the_clear_colour_read_the_frame_context_once() {
    let contention = declared::<DrawsAndClears>().contention();

    let reads = contention
        .reads
        .iter()
        .filter(|ty| **ty == frame_context())
        .count();
    assert_eq!(reads, 1, "one read: {:?}", contention.reads);
}

/// **A lane without frame inputs does not read the frame context.** Declaring
/// more than it touches would serialise its wave for nothing: its contention
/// is its resource and nothing else.
#[test]
fn a_lane_without_frame_inputs_does_not_read_the_frame_context() {
    let contention = declared::<NoFrameInput<'static>>().contention();

    assert_eq!(
        contention.reads,
        vec![TypeId::of::<MixSettings>()],
        "only the resource is read"
    );
}

/// The fixed step is a frame value its own agent hands over, not one
/// published in the frame context: a lane reading only the step does not read
/// the context.
#[test]
fn a_fixed_step_input_does_not_read_the_frame_context() {
    let contention = declared::<StepsOnly>().contention();

    assert!(
        !contention.reads.contains(&frame_context()),
        "the step does not come from the frame context: {:?}",
        contention.reads
    );
}
