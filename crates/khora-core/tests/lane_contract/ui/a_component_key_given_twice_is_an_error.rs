use khora_macros::Component;

#[derive(Default, Component)]
#[component(domain = Physics)]
#[component(domain = Render)]
pub struct Mass {
    pub kilograms: f32,
}

fn main() {}
