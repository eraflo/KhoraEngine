use khora_macros::LaneInputs;

pub struct Counter(pub u32);

#[derive(LaneInputs)]
pub struct CountInputs<'f> {
    #[locked]
    pub counter: &'f Counter,
}

fn main() {}
