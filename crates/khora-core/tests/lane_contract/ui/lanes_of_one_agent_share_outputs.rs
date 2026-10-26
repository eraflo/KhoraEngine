use khora_core::agent::gorna::AgentId;
use khora_core::agent::registration::AgentRegistration;
use khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::Cost;
use khora_core::lane::family::{LaneFamily, Semantics};
use khora_core::lane::guard::{InputRefusal, OutputRefusal};
use khora_core::lane::io::{Declaration, InputSource, LaneInputs, LaneOutputs, OutputSink};
use khora_core::lane::level::Level;
use khora_core::lane::{LaneError, LaneKind};

pub struct NoInputs;

impl<'f> LaneInputs<'f> for NoInputs {
    fn declare(_d: &mut Declaration) {}
    fn fetch(_src: &mut InputSource<'f>) -> Result<Self, InputRefusal> {
        Ok(NoInputs)
    }
}

pub struct NoOutputs;

impl<'f> LaneOutputs<'f> for NoOutputs {
    fn declare(_d: &mut Declaration) {}
    fn open(_sink: &mut OutputSink<'f>) -> Result<Self, LaneError> {
        Ok(NoOutputs)
    }
    fn commit(self, _sink: &mut OutputSink<'f>) -> Result<(), OutputRefusal> {
        Ok(())
    }
}

pub struct AudioFamily;

impl LaneFamily for AudioFamily {
    const KIND: LaneKind = LaneKind::Audio;
    const NAME: &'static str = "audio";
    const AGENT: AgentId = AgentId::Audio;
    const SEMANTICS: Semantics = Semantics::Presentation;
    type Outputs<'f> = NoOutputs;
}

pub struct UiFamily;

impl LaneFamily for UiFamily {
    const KIND: LaneKind = LaneKind::Ui;
    const NAME: &'static str = "ui";
    const AGENT: AgentId = AgentId::Ui;
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

#[derive(Default)]
pub struct LayoutLane;

impl Lane for LayoutLane {
    type Family = UiFamily;
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

const AUDIO: AgentRegistration = khora_core::register_agent! {
    family: AudioFamily,
    phase: ExecutionPhase::OUTPUT,
    importance: AgentImportance::Important,
    after: [],
    modes: [],
    fixed_step: None,
    lanes: [MixLane, LayoutLane],
};

fn main() {
    let _ = &AUDIO;
}
