use khora_macros::ergon_fn;

#[ergon_fn(cost = 4, cost = 8)]
fn distance(a: f32, b: f32) -> f32 {
    (a - b).abs()
}

fn main() {}
