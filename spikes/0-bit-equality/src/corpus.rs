/// One corpus sample: the inputs a single probe evaluation takes.
pub struct Input {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub seed: u64,
}

pub fn input_at(_index: u64) -> Input {
    Input { x: 0.0, y: 0.0, z: 1.0, seed: 1 }
}
