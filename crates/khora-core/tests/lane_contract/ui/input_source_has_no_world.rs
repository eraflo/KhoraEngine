use khora_core::lane::io::InputSource;

fn the_world_by_field(source: &InputSource<'_>) {
    let _ = &source.world;
}

fn the_world_by_method(source: &InputSource<'_>) {
    let _ = source.world();
}

fn main() {
    let _ = the_world_by_field;
    let _ = the_world_by_method;
}
