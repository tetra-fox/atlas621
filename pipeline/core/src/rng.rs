use rand::prelude::*;

// box-muller, keeping one of the two normals it produces
pub fn normal(rng: &mut StdRng, sd: f64) -> f64 {
    let (u, v): (f64, f64) = (rng.random_range(f64::EPSILON..1.0), rng.random());
    sd * (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
}
