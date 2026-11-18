use khora_macros::Component;

#[derive(Default, Component)]
#[component(domain = Physics)]
pub struct Mass {
    pub kilograms: f32,
    #[component(skp)]
    pub cache: u64,
}

fn main() {}
