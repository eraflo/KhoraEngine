use khora_macros::Component;

#[derive(Default, Component)]
#[component(no_serializable, provenance = Bogus)]
pub struct Mass {
    pub kilograms: f32,
}

fn main() {}
