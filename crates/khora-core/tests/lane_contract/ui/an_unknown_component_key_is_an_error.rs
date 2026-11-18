use khora_macros::Component;

#[derive(Default, Component)]
#[component(domian = Physics)]
pub struct Mass {
    pub kilograms: f32,
}

fn main() {}
