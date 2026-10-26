use khora_core::lane::io::Budget;
use khora_macros::{LaneInputs, LaneOutputs};

pub struct Positions(pub Vec<f32>);

pub struct Contacts(pub u32);

#[derive(Default)]
pub struct Mixed(pub u32);

#[derive(LaneInputs)]
pub struct Inputs<'f> {
    #[bus]
    pub src: &'f Positions,
    #[deck]
    pub d: Option<&'f Contacts>,
    #[budget]
    pub value: Budget,
}

#[derive(LaneOutputs)]
pub struct Outputs {
    #[deck]
    pub sink: Mixed,
}

fn main() {}
