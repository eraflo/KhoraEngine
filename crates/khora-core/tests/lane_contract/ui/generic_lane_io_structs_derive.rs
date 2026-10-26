use std::any::Any;

use khora_core::lane::io::{LaneInputs, LaneOutputs};
use khora_macros::{LaneInputs, LaneOutputs};

pub struct Positions(pub Vec<f32>);

#[derive(LaneInputs)]
pub struct ViewOf<'f, V: Any + Send + Sync> {
    #[bus]
    pub view: &'f V,
}

#[derive(LaneInputs)]
pub struct EntryOf<'f, T>
where
    T: Any + Send + Sync,
{
    #[resource]
    pub entry: &'f T,
}

#[derive(LaneOutputs)]
pub struct SlotOf<T: Default + Any + Send + Sync> {
    #[deck]
    pub slot: T,
}

#[derive(LaneInputs)]
pub struct NoInputs;

#[derive(LaneOutputs)]
pub struct NoOutputs {}

fn inputs<'f, I: LaneInputs<'f>>() {}
fn outputs<'f, O: LaneOutputs<'f>>() {}

fn main() {
    inputs::<ViewOf<'_, Positions>>();
    inputs::<EntryOf<'_, u32>>();
    inputs::<NoInputs>();
    outputs::<SlotOf<u64>>();
    outputs::<NoOutputs>();
}
