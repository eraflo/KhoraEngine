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

//! What `check_registrations` refuses at boot, before the first frame.

use std::any::TypeId;

use khora_core::agent::gorna::StrategyId;
use khora_core::agent::registration::{check_registrations, AgentRegistration, BootError};
use khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::erased::{AnyLane, Erased};
use khora_core::lane::family::LaneFamily;
use khora_core::Runtime;

use crate::fixtures::{
    CheapMixLane, EchoFamily, EchoLane, MixFamily, MixLane, MixSettings, OtherBalancedMixLane,
};

type LaneConstructor = fn() -> Box<dyn AnyLane>;

fn mix_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(MixLane))
}

fn cheap_mix_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(CheapMixLane))
}

fn other_balanced_mix_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(OtherBalancedMixLane))
}

fn echo_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(EchoLane))
}

/// The registration `register_agent!` would build for family `F`.
fn registration<F: LaneFamily>(
    phase: ExecutionPhase,
    lanes: &'static [LaneConstructor],
) -> AgentRegistration {
    AgentRegistration {
        family: TypeId::of::<F>(),
        family_name: F::NAME,
        kind: F::KIND,
        agent: F::AGENT,
        semantics: F::SEMANTICS,
        phase,
        importance: AgentImportance::Important,
        after: &[],
        modes: &[],
        fixed_step: None,
        lanes,
    }
}

/// A runtime holding what the mix lanes read.
fn runtime_with_mix_settings() -> Runtime {
    let mut runtime = Runtime::new();
    runtime.resources.insert(MixSettings { gain: 1.0 });
    runtime
}

/// The control case: a family that reads what the runtime holds, and one
/// that reads a slot an earlier phase writes, boot.
#[test]
fn boot_accepts_registrations_that_hold_together() {
    let mix = registration::<MixFamily>(ExecutionPhase::TRANSFORM, &[mix_lane, cheap_mix_lane]);
    let echo = registration::<EchoFamily>(ExecutionPhase::OUTPUT, &[echo_lane]);

    let checked = check_registrations(
        &[&mix, &echo],
        &runtime_with_mix_settings(),
        &DeviceCaps::default(),
    );

    assert_eq!(checked, Ok(()));
}

/// One family has one agent: two would each pick a lane and write the same
/// outputs.
#[test]
fn boot_refuses_two_agents_for_one_family() {
    let first = registration::<MixFamily>(ExecutionPhase::OUTPUT, &[mix_lane]);
    let second = registration::<MixFamily>(ExecutionPhase::OUTPUT, &[cheap_mix_lane]);

    let checked = check_registrations(
        &[&first, &second],
        &runtime_with_mix_settings(),
        &DeviceCaps::default(),
    );

    assert_eq!(
        checked,
        Err(BootError::TwoAgentsForOneFamily { family: "mix" })
    );
}

/// A lane that reads a resource the runtime does not hold would be refused on
/// every frame; boot says so once, naming the field and the type.
#[test]
fn boot_refuses_a_lane_reading_an_unknown_resource() {
    let mix = registration::<MixFamily>(ExecutionPhase::OUTPUT, &[mix_lane]);

    let checked = check_registrations(&[&mix], &Runtime::new(), &DeviceCaps::default());

    match checked {
        Err(BootError::UnknownResource {
            family,
            lane,
            field,
            resource,
        }) => {
            assert_eq!(family, "mix");
            assert!(lane.ends_with("MixLane"), "the lane is named: {lane}");
            assert_eq!(field, "settings");
            assert!(
                resource.ends_with("MixSettings"),
                "the resource is named: {resource}"
            );
        }
        other => panic!("expected UnknownResource, got {other:?}"),
    }
}

/// A `#[deck]` input reads what an earlier phase wrote. Its writer running in
/// a later phase means the slot is always empty when the lane reads it.
#[test]
fn boot_refuses_a_deck_read_without_earlier_writer() {
    let echo = registration::<EchoFamily>(ExecutionPhase::OUTPUT, &[echo_lane]);
    let late_mix = registration::<MixFamily>(ExecutionPhase::FINALIZE, &[mix_lane]);
    let runtime = runtime_with_mix_settings();

    for registrations in [vec![&echo], vec![&echo, &late_mix]] {
        let checked = check_registrations(&registrations, &runtime, &DeviceCaps::default());
        match checked {
            Err(BootError::DeckReadWithoutEarlierWriter {
                family,
                lane,
                field,
                slot,
            }) => {
                assert_eq!(family, "echo");
                assert!(lane.ends_with("EchoLane"), "the lane is named: {lane}");
                assert_eq!(field, "mixed");
                assert!(slot.ends_with("MixedFrames"), "the slot is named: {slot}");
            }
            other => panic!(
                "expected DeckReadWithoutEarlierWriter with {} registration(s), got {other:?}",
                registrations.len()
            ),
        }
    }
}

/// GORNA names a menu row by its strategy id, so two rows of one family with
/// the same id could not be told apart.
#[test]
fn boot_refuses_a_family_with_duplicate_strategy_ids() {
    let mix =
        registration::<MixFamily>(ExecutionPhase::OUTPUT, &[mix_lane, other_balanced_mix_lane]);

    let checked = check_registrations(
        &[&mix],
        &runtime_with_mix_settings(),
        &DeviceCaps::default(),
    );

    assert_eq!(
        checked,
        Err(BootError::DuplicateStrategyId {
            family: "mix",
            id: StrategyId::Balanced,
        })
    );
}

// ─── The agent a family negotiates as, and the device ───────────────────────

use khora_core::agent::gorna::AgentId;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::Cost;
use khora_core::lane::family::Semantics;
use khora_core::lane::level::Level;
use khora_core::lane::{LaneError, LaneKind};
use khora_macros::LaneInputs;

use crate::fixtures::{level, Counter, EchoOutputs, Echoes, MixInputs, MixOutputs, MixedFrames};

/// A second family that negotiates as the mix family's agent.
pub struct AlsoAudioFamily;

impl LaneFamily for AlsoAudioFamily {
    const KIND: LaneKind = LaneKind::Audio;
    const NAME: &'static str = "also audio";
    const AGENT: AgentId = AgentId::Audio;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = EchoOutputs;
}

const ALSO_AUDIO_LEVELS: &[Level] = &[level(StrategyId::Balanced, "also audio")];

#[derive(Default)]
pub struct AlsoAudioLane;

impl Lane for AlsoAudioLane {
    type Family = AlsoAudioFamily;
    type Inputs<'f> = MixInputs<'f>;

    fn levels(&self) -> &[Level] {
        ALSO_AUDIO_LEVELS
    }
    fn cost(&self, _level: usize, _inputs: &MixInputs<'_>) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut MixInputs<'_>,
        out: &mut EchoOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.echoes = Echoes(0);
        Ok(LaneReport::default())
    }
}

fn also_audio_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(AlsoAudioLane))
}

/// The DCC keys an agent's budget, cost model and hints by its `AgentId`. Two
/// families declaring the same `AGENT` would boot as two agents under one id:
/// one budget for both, one cost model fed by both. Boot refuses it, as it
/// refuses two agents for one family.
#[test]
fn boot_refuses_two_families_negotiating_as_one_agent() {
    let mix = registration::<MixFamily>(ExecutionPhase::OUTPUT, &[mix_lane]);
    let also = registration::<AlsoAudioFamily>(ExecutionPhase::OUTPUT, &[also_audio_lane]);

    let checked = check_registrations(
        &[&mix, &also],
        &runtime_with_mix_settings(),
        &DeviceCaps::default(),
    );

    assert!(
        checked.is_err(),
        "`mix` and `also audio` both negotiate as {:?}, and boot accepted them",
        AgentId::Audio
    );
}

/// What the storage-buffer mix lane reads: a resource no runtime here holds.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct StorageMixInputs<'f> {
    #[resource]
    pub counter: &'f Counter,
}

const STORAGE_MIX_LEVELS: &[Level] = &[
    level(StrategyId::HighPerformance, "storage mix"),
    level(StrategyId::HighPerformance, "storage mix again"),
];

/// Mixes with storage buffers; offers two rows with one id, and reads what
/// the runtime does not hold — none of which matters on a device it does not
/// run on.
#[derive(Default)]
pub struct StorageMixLane;

impl Lane for StorageMixLane {
    type Family = MixFamily;
    type Inputs<'f> = StorageMixInputs<'f>;

    fn levels(&self) -> &[Level] {
        STORAGE_MIX_LEVELS
    }
    fn supports(&self, caps: &DeviceCaps) -> bool {
        caps.storage_buffers
    }
    fn cost(&self, _level: usize, _inputs: &StorageMixInputs<'_>) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut StorageMixInputs<'_>,
        out: &mut MixOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.mixed = MixedFrames(3);
        Ok(LaneReport::default())
    }
}

fn storage_mix_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(StorageMixLane))
}

/// A behaviour the device cannot run leaves the menu at boot: it is not
/// checked, and a family left with no lane is refused.
#[test]
fn an_unsupported_behaviour_leaves_the_menu() {
    let runtime = runtime_with_mix_settings();
    let no_storage = DeviceCaps {
        storage_buffers: false,
    };

    let both = registration::<MixFamily>(ExecutionPhase::OUTPUT, &[mix_lane, storage_mix_lane]);
    assert_eq!(
        check_registrations(&[&both], &runtime, &no_storage),
        Ok(()),
        "the storage lane's duplicate rows and unknown resource do not count off the menu"
    );

    let only_storage = registration::<MixFamily>(ExecutionPhase::OUTPUT, &[storage_mix_lane]);
    assert_eq!(
        check_registrations(&[&only_storage], &runtime, &no_storage),
        Err(BootError::NoSupportedLane { family: "mix" })
    );
}

/// Offers no level at all: a menu entry with no row.
#[derive(Default)]
pub struct LevellessMixLane;

impl Lane for LevellessMixLane {
    type Family = MixFamily;
    type Inputs<'f> = MixInputs<'f>;

    fn levels(&self) -> &[Level] {
        &[]
    }
    fn cost(&self, _level: usize, _inputs: &MixInputs<'_>) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut MixInputs<'_>,
        out: &mut MixOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.mixed = MixedFrames(4);
        Ok(LaneReport::default())
    }
}

fn levelless_mix_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(LevellessMixLane))
}

/// A family whose only supported lane offers no level has no menu row for
/// GORNA to pick, and every run of it fails (`level 0 of 0`). Boot refuses it
/// as it refuses a family with no supported lane.
#[test]
fn boot_refuses_a_family_with_no_level_to_offer() {
    let mix = registration::<MixFamily>(ExecutionPhase::OUTPUT, &[levelless_mix_lane]);

    let checked = check_registrations(
        &[&mix],
        &runtime_with_mix_settings(),
        &DeviceCaps::default(),
    );

    assert!(
        checked.is_err(),
        "the mix family has no row to run, and boot accepted it"
    );
}

/// A simulation family whose cheaper level changes the result.
pub struct DriftingSpeedFamily;

impl LaneFamily for DriftingSpeedFamily {
    const KIND: LaneKind = LaneKind::Physics;
    const NAME: &'static str = "drifting speed";
    const AGENT: AgentId = AgentId::Physics;
    const SEMANTICS: Semantics = Semantics::Exact;
    type Outputs<'f> = MixOutputs;
}

const DRIFTING_LEVELS: &[Level] = &[
    level(StrategyId::Balanced, "exact"),
    Level {
        impact: khora_core::lane::level::Impact::Major,
        ..level(StrategyId::LowPower, "coarse")
    },
];

#[derive(Default)]
pub struct DriftingSpeedLane;

impl Lane for DriftingSpeedLane {
    type Family = DriftingSpeedFamily;
    type Inputs<'f> = MixInputs<'f>;

    fn levels(&self) -> &[Level] {
        DRIFTING_LEVELS
    }
    fn cost(&self, _level: usize, _inputs: &MixInputs<'_>) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        level: usize,
        _inputs: &mut MixInputs<'_>,
        out: &mut MixOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.mixed = MixedFrames(level as u32);
        Ok(LaneReport::default())
    }
}

fn drifting_speed_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(DriftingSpeedLane))
}

/// Adapt the HOW, never the WHAT (RULES §3): a `Semantics::Exact` family may
/// only offer result-preserving levels. A level whose `impact` is not `None`
/// would let GORNA change the simulation; boot refuses it.
#[test]
fn boot_refuses_an_exact_family_offering_a_level_that_changes_the_result() {
    let drifting =
        registration::<DriftingSpeedFamily>(ExecutionPhase::TRANSFORM, &[drifting_speed_lane]);

    let checked = check_registrations(
        &[&drifting],
        &runtime_with_mix_settings(),
        &DeviceCaps::default(),
    );

    assert!(
        checked.is_err(),
        "an Exact family offers an `Impact::Major` level, and boot accepted it"
    );
}

// ─── Levels a presentation family may offer, locks a lane may take ──────────

use std::sync::{Arc, Mutex};

use crate::guards::TwoLocksOfOneCounter;

const ROUGH_MIX_LEVELS: &[Level] = &[
    level(StrategyId::Balanced, "full mix"),
    Level {
        impact: khora_core::lane::level::Impact::Major,
        ..level(StrategyId::LowPower, "rough mix")
    },
];

/// Mixes at two levels, the cheaper one audibly worse.
#[derive(Default)]
pub struct RoughMixLane;

impl Lane for RoughMixLane {
    type Family = MixFamily;
    type Inputs<'f> = MixInputs<'f>;

    fn levels(&self) -> &[Level] {
        ROUGH_MIX_LEVELS
    }
    fn cost(&self, _level: usize, _inputs: &MixInputs<'_>) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        level: usize,
        _inputs: &mut MixInputs<'_>,
        out: &mut MixOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.mixed = MixedFrames(level as u32);
        Ok(LaneReport::default())
    }
}

fn rough_mix_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(RoughMixLane))
}

/// The `Exact` rule is the simulation's alone: a `Presentation` family's
/// cheaper level may look or sound worse, and boot accepts it.
#[test]
fn a_presentation_family_may_offer_a_level_that_looks_worse() {
    let mix = registration::<MixFamily>(ExecutionPhase::OUTPUT, &[rough_mix_lane]);

    assert_eq!(
        check_registrations(
            &[&mix],
            &runtime_with_mix_settings(),
            &DeviceCaps::default()
        ),
        Ok(())
    );
}

/// Locks one counter twice in one fetch.
#[derive(Default)]
pub struct DoubleLockMixLane;

impl Lane for DoubleLockMixLane {
    type Family = MixFamily;
    type Inputs<'f> = TwoLocksOfOneCounter<'f>;

    fn levels(&self) -> &[Level] {
        MIX_LEVELS_FOR_DOUBLE_LOCK
    }
    fn cost(&self, _level: usize, _inputs: &TwoLocksOfOneCounter<'_>) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut TwoLocksOfOneCounter<'_>,
        out: &mut MixOutputs,
    ) -> Result<LaneReport, LaneError> {
        out.mixed = MixedFrames(5);
        Ok(LaneReport::default())
    }
}

const MIX_LEVELS_FOR_DOUBLE_LOCK: &[Level] = &[level(StrategyId::Balanced, "double lock")];

fn double_lock_mix_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(DoubleLockMixLane))
}

/// A lane declaring two locks of one entry could never take the second: boot
/// refuses it, naming the lane and the second field, before any frame waits
/// on it.
#[test]
fn boot_refuses_a_lane_locking_one_entry_twice() {
    let mut runtime = Runtime::new();
    let counter: Arc<Mutex<Counter>> = Arc::new(Mutex::new(Counter(0)));
    runtime.resources.insert(counter);
    let mix = registration::<MixFamily>(ExecutionPhase::OUTPUT, &[double_lock_mix_lane]);

    assert_eq!(
        check_registrations(&[&mix], &runtime, &DeviceCaps::default()),
        Err(BootError::SameLockTwice {
            family: "mix",
            lane: std::any::type_name::<DoubleLockMixLane>(),
            field: "second",
        })
    );
}

/// Each refusal names what it refuses: the levelless family, the drifting
/// family's offending level, the agent two families share.
#[test]
fn boot_refusals_name_the_family_level_and_agent() {
    let runtime = runtime_with_mix_settings();
    let caps = DeviceCaps::default();

    let levelless = registration::<MixFamily>(ExecutionPhase::OUTPUT, &[levelless_mix_lane]);
    assert_eq!(
        check_registrations(&[&levelless], &runtime, &caps),
        Err(BootError::NoLevel { family: "mix" })
    );

    let drifting =
        registration::<DriftingSpeedFamily>(ExecutionPhase::TRANSFORM, &[drifting_speed_lane]);
    assert_eq!(
        check_registrations(&[&drifting], &runtime, &caps),
        Err(BootError::ResultChangingLevel {
            family: "drifting speed",
            level: "coarse",
        })
    );

    let mix = registration::<MixFamily>(ExecutionPhase::OUTPUT, &[mix_lane]);
    let also = registration::<AlsoAudioFamily>(ExecutionPhase::OUTPUT, &[also_audio_lane]);
    assert_eq!(
        check_registrations(&[&mix, &also], &runtime, &caps),
        Err(BootError::TwoFamiliesForOneAgent {
            agent: AgentId::Audio,
            first: "mix",
            second: AlsoAudioFamily::NAME,
        })
    );
}

// ─── A family writing one slot twice ────────────────────────────────────────

use crate::outputs::TwiceEchoes;

/// A family whose outputs name one deck slot twice.
pub struct TwiceEchoFamily;

impl LaneFamily for TwiceEchoFamily {
    const KIND: LaneKind = LaneKind::Ui;
    const NAME: &'static str = "twice echo";
    const AGENT: AgentId = AgentId::Ui;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = TwiceEchoes;
}

const TWICE_ECHO_LEVELS: &[Level] = &[level(StrategyId::Balanced, "twice echo")];

#[derive(Default)]
pub struct TwiceEchoLane;

impl Lane for TwiceEchoLane {
    type Family = TwiceEchoFamily;
    type Inputs<'f> = MixInputs<'f>;

    fn levels(&self) -> &[Level] {
        TWICE_ECHO_LEVELS
    }
    fn cost(&self, _level: usize, _inputs: &MixInputs<'_>) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut MixInputs<'_>,
        out: &mut TwiceEchoes,
    ) -> Result<LaneReport, LaneError> {
        out.first = Echoes(1);
        out.second = Echoes(2);
        Ok(LaneReport::default())
    }
}

fn twice_echo_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(TwiceEchoLane))
}

/// Two output fields of one slot type can never both commit: every frame the
/// family runs ends `OutputRefused(WrittenTwice)`. Like two locks of one
/// entry, the declaration says so before the first frame, and boot refuses it.
#[test]
fn boot_refuses_a_family_writing_one_slot_twice() {
    let twice = registration::<TwiceEchoFamily>(ExecutionPhase::OUTPUT, &[twice_echo_lane]);

    let checked = check_registrations(
        &[&twice],
        &runtime_with_mix_settings(),
        &DeviceCaps::default(),
    );

    assert!(
        checked.is_err(),
        "`twice echo` declares `Echoes` as two deck outputs, and boot accepted it"
    );
}
