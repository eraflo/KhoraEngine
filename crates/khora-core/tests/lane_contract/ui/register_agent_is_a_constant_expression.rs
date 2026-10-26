use khora_core::agent::gorna::AgentId;
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{AgentImportance, EngineMode, ExecutionPhase};
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::Cost;
use khora_core::lane::erased::AnyLane;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::level::Level;
use khora_core::lane::{LaneError, LaneKind};
use khora_macros::{LaneInputs, LaneOutputs};

#[derive(LaneInputs)]
pub struct NoInputs;

#[derive(LaneOutputs)]
pub struct NoOutputs;

pub struct AudioFamily;

impl LaneFamily for AudioFamily {
    const KIND: LaneKind = LaneKind::Audio;
    const NAME: &'static str = "audio";
    const AGENT: AgentId = AgentId::Audio;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = NoOutputs;
}

#[derive(Default)]
pub struct MixLane;

impl Lane for MixLane {
    type Family = AudioFamily;
    type Inputs<'f> = NoInputs;

    fn levels(&self) -> &[Level] {
        &[]
    }
    fn cost(&self, _level: usize, _inputs: &NoInputs) -> Cost {
        Cost::default()
    }
    fn record(
        &mut self,
        _level: usize,
        _inputs: &mut NoInputs,
        _out: &mut NoOutputs,
    ) -> Result<LaneReport, LaneError> {
        Ok(LaneReport::default())
    }
}

// What `inventory::submit!` builds: a `static`, and a reference to one.
static AUDIO: AgentRegistration = khora_core::register_agent! {
    family: AudioFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [AgentId::Physics],
    modes: [EngineMode::Playing],
    fixed_step: None,
    lanes: [MixLane],
};

static AUDIO_REF: &AgentRegistration = &khora_core::register_agent! {
    family: AudioFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: Some(std::time::Duration::from_millis(16)),
    lanes: [MixLane,],
};

fn main() {
    assert_eq!(AUDIO.lanes.len(), 1);
    assert_eq!(AUDIO_REF.lanes.len(), 1);
    assert_eq!((AUDIO.lanes[0])().family(), std::any::TypeId::of::<AudioFamily>());
}
