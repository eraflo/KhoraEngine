use khora_macros::ergon_fn;

#[ergon_fn(nmae = "Distance")]
fn distance(a: f32, b: f32) -> f32 {
    (a - b).abs()
}

fn main() {}
