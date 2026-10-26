use khora_core::lane::io::Budget;
use khora_macros::{LaneInputs, LaneOutputs};

pub struct Positions(pub Vec<f32>);

pub struct Contacts(pub u32);

#[derive(Default)]
pub struct Mixed(pub u32);

#[derive(Default)]
pub struct Echoed(pub u32);

#[derive(LaneInputs)]
pub struct Inputs<'f> {
    #[bus]
    pub __khora_src: &'f Positions,
    #[deck]
    pub __khora_value: Option<&'f Contacts>,
    #[budget]
    pub budget: Budget,
}

#[derive(LaneOutputs)]
pub struct Outputs {
    #[deck]
    pub __khora_sink: Mixed,
    #[deck]
    pub echoed: Echoed,
}

fn main() {}
