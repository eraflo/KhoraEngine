use khora_macros::LaneInputs;

pub struct Positions(pub Vec<f32>);

#[derive(LaneInputs)]
pub struct PositionInputs<'f> {
    pub positions: &'f Positions,
}

fn main() {}
