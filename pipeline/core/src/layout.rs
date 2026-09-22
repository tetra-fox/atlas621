use std::time::Instant;

use tracing::{debug, info};
use rand::prelude::*;
use rayon::prelude::*;

use crate::csr::Csr;
use crate::edges::Edges;
use crate::quadtree::{NONE, QuadTree};

pub struct LayoutParams {
    pub iterations: usize,
    pub scaling: f32,
    pub gravity: f32,
    pub strong_gravity: bool,
    pub lin_log: bool,
    pub theta: f32,
    pub jitter_tolerance: f32,
    pub community_gravity: f32,
    pub weight_exponent: f32,
    pub seed: u64,
}

fn known(p: &[f32; 2]) -> bool {
    !p[0].is_nan() && !p[1].is_nan()
}

fn recenter(pos: &mut [[f32; 2]]) {
    let n = pos.len() as f64;
    let (mx, my) = pos.iter().fold((0.0f64, 0.0f64), |(x, y), p| {
        (x + p[0] as f64, y + p[1] as f64)
    });
    let (mx, my) = ((mx / n) as f32, (my / n) as f32);
    for p in pos {
        p[0] -= mx;
        p[1] -= my;
    }
}

fn repulsion(
    tree: &QuadTree,
    i: usize,
    p: [f32; 2],
    m: f32,
    theta: f32,
    k: f32,
    stack: &mut Vec<u32>,
) -> [f32; 2] {
    let mut f = [0f32; 2];
    stack.clear();
    stack.push(0);
    while let Some(ci) = stack.pop() {
        let cell = &tree.cells[ci as usize];
        if cell.count == 0 || (cell.count == 1 && cell.body == i as u32) {
            continue;
        }
        let com = cell.center_of_mass();
        let d = [p[0] - com[0], p[1] - com[1]];
        let dist2 = (d[0] * d[0] + d[1] * d[1]).max(1e-4);
        let leaf = cell.is_leaf();
        if leaf || (cell.half * 2.0) * (cell.half * 2.0) < theta * theta * dist2 {
            let factor = k * m * cell.mass / dist2;
            f[0] += d[0] * factor;
            f[1] += d[1] * factor;
        } else {
            stack.extend_from_slice(&cell.child);
        }
    }
    f
}

pub struct Layout {
    pub pos: Vec<[f32; 2]>,
    disp: Vec<[f32; 2]>,
    old_disp: Vec<[f32; 2]>,
    mass: Vec<f32>,
    adj: Csr<f32>,
    community: Vec<u32>,
    n_communities: usize,
    speed: f32,
    speed_efficiency: f32,
    params: LayoutParams,
}

impl Layout {
    pub fn new(
        n: usize,
        edges: &Edges,
        community: &[u32],
        initial: Option<&[[f32; 2]]>,
        params: LayoutParams,
    ) -> Layout {
        let mut rng = StdRng::seed_from_u64(params.seed);
        let adj = Csr::symmetric(
            n,
            edges
                .iter()
                .map(|e| (e.a, e.b, e.weight.powf(params.weight_exponent))),
        );
        let best: Vec<u32> = (0..n)
            .map(|i| {
                adj.row(i)
                    .max_by(|x, y| x.1.total_cmp(&y.1))
                    .map_or(NONE, |(j, _)| j)
            })
            .collect();
        let radius = (n as f32).sqrt();
        let random_point = |rng: &mut StdRng| {
            let r = radius * rng.random::<f32>().sqrt();
            let t = rng.random::<f32>() * std::f32::consts::TAU;
            [r * t.cos(), r * t.sin()]
        };
        let mut pos: Vec<[f32; 2]> = match initial {
            Some(init) => init.to_vec(),
            None => (0..n).map(|_| random_point(&mut rng)).collect(),
        };
        for _ in 0..4 {
            for i in 0..n {
                if known(&pos[i]) {
                    continue;
                }
                let j = best[i];
                if j != NONE && known(&pos[j as usize]) {
                    let [x, y] = pos[j as usize];
                    pos[i] = [
                        x + rng.random_range(-1.0..1.0),
                        y + rng.random_range(-1.0..1.0),
                    ];
                }
            }
        }
        let mut placed_randomly = 0;
        for p in pos.iter_mut() {
            if !known(p) {
                *p = random_point(&mut rng);
                placed_randomly += 1;
            }
        }
        if initial.is_some() {
            info!(
                "warm start: {placed_randomly} nodes had no seeded neighbor and were placed at random"
            );
        }
        let n_communities = community
            .iter()
            .copied()
            .max()
            .map_or(0, |c| c as usize + 1);
        Layout {
            pos,
            disp: vec![[0.0; 2]; n],
            old_disp: vec![[0.0; 2]; n],
            mass: (0..n).map(|i| adj.degree(i) as f32 + 1.0).collect(),
            adj,
            community: community.to_vec(),
            n_communities,
            speed: 1.0,
            speed_efficiency: 1.0,
            params,
        }
    }

    fn apply_attraction(&mut self) {
        let lin_log = self.params.lin_log;
        let (pos, adj) = (&self.pos, &self.adj);
        self.disp.par_iter_mut().enumerate().for_each(|(i, disp)| {
            let pi = pos[i];
            let mut f = [0f32; 2];
            for (j, w) in adj.row(i) {
                let pj = pos[j as usize];
                let d = [pj[0] - pi[0], pj[1] - pi[1]];
                let factor = if lin_log {
                    let dist = (d[0] * d[0] + d[1] * d[1]).sqrt();
                    if dist > 0.0 {
                        w * dist.ln_1p() / dist
                    } else {
                        0.0
                    }
                } else {
                    w
                };
                f[0] += d[0] * factor;
                f[1] += d[1] * factor;
            }
            disp[0] += f[0];
            disp[1] += f[1];
        });
    }

    fn apply_repulsion_and_gravity(&mut self) {
        let tree = QuadTree::build(&self.pos, &self.mass);
        let (theta, k, kg, strong) = (
            self.params.theta,
            self.params.scaling,
            self.params.gravity,
            self.params.strong_gravity,
        );
        let (pos, mass) = (&self.pos, &self.mass);
        self.disp.par_iter_mut().enumerate().for_each_init(
            || Vec::with_capacity(256),
            |stack, (i, disp)| {
                let (p, m) = (pos[i], mass[i]);
                let r = repulsion(&tree, i, p, m, theta, k, stack);
                disp[0] += r[0];
                disp[1] += r[1];
                let dist = (p[0] * p[0] + p[1] * p[1]).sqrt();
                if dist > 0.0 {
                    let g = if strong { kg * m } else { kg * m / dist };
                    disp[0] -= p[0] * g;
                    disp[1] -= p[1] * g;
                }
            },
        );
    }

    fn apply_community_gravity(&mut self) {
        let kc = self.params.community_gravity;
        if kc == 0.0 || self.n_communities == 0 {
            return;
        }
        let mut sum = vec![[0f64; 2]; self.n_communities];
        let mut count = vec![0u32; self.n_communities];
        for (p, &c) in self.pos.iter().zip(&self.community) {
            sum[c as usize][0] += p[0] as f64;
            sum[c as usize][1] += p[1] as f64;
            count[c as usize] += 1;
        }
        let centroid: Vec<[f32; 2]> = sum
            .iter()
            .zip(&count)
            .map(|(s, &c)| {
                if c > 0 {
                    [(s[0] / c as f64) as f32, (s[1] / c as f64) as f32]
                } else {
                    [0.0; 2]
                }
            })
            .collect();
        let (pos, mass, community) = (&self.pos, &self.mass, &self.community);
        self.disp.par_iter_mut().enumerate().for_each(|(i, disp)| {
            let c = centroid[community[i] as usize];
            disp[0] += (c[0] - pos[i][0]) * kc * mass[i];
            disp[1] += (c[1] - pos[i][1]) * kc * mass[i];
        });
    }

    // the adaptive speed controller from the forceatlas2 paper; its constants are tuned as a
    // set and are not meaningful on their own
    fn apply_forces(&mut self) -> (f32, f32) {
        let n = self.pos.len() as f32;
        let (swinging, traction) = self
            .disp
            .par_iter()
            .zip(&self.old_disp)
            .zip(&self.mass)
            .map(|((d, o), &m)| {
                let sw = m * ((d[0] - o[0]).powi(2) + (d[1] - o[1]).powi(2)).sqrt();
                let tr = m * ((d[0] + o[0]).powi(2) + (d[1] + o[1]).powi(2)).sqrt() * 0.5;
                (sw, tr)
            })
            .reduce(|| (0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
        let estimated = 0.05 * n.sqrt();
        let min_jt = estimated.sqrt();
        let max_jt: f32 = 10.0;
        let mut jt =
            self.params.jitter_tolerance * min_jt.max(max_jt.min(estimated * traction / (n * n)));
        let min_efficiency = 0.05;
        if traction > 0.0 && swinging / traction > 2.0 {
            if self.speed_efficiency > min_efficiency {
                self.speed_efficiency *= 0.5;
            }
            jt = jt.max(self.params.jitter_tolerance);
        }
        let target = if swinging > 0.0 {
            jt * self.speed_efficiency * traction / swinging
        } else {
            self.speed * 1.5
        };
        if swinging > jt * traction {
            if self.speed_efficiency > min_efficiency {
                self.speed_efficiency *= 0.7;
            }
        } else if self.speed < 1000.0 {
            self.speed_efficiency *= 1.3;
        }
        let max_rise = 0.5;
        self.speed += (target - self.speed).min(max_rise * self.speed);
        let speed = self.speed;
        self.pos
            .par_iter_mut()
            .zip(&mut self.disp)
            .zip(&mut self.old_disp)
            .zip(&self.mass)
            .for_each(|(((p, d), o), &m)| {
                let sw = m * ((d[0] - o[0]).powi(2) + (d[1] - o[1]).powi(2)).sqrt();
                let factor = speed / (1.0 + (speed * sw).sqrt());
                p[0] += d[0] * factor;
                p[1] += d[1] * factor;
                *o = *d;
                *d = [0.0; 2];
            });
        (swinging, traction)
    }

    pub fn step(&mut self) -> (f32, f32) {
        let t = Instant::now();
        self.apply_attraction();
        let t_attr = t.elapsed();
        self.apply_repulsion_and_gravity();
        let t_rep = t.elapsed() - t_attr;
        self.apply_community_gravity();
        let t_comm = t.elapsed() - t_attr - t_rep;
        let out = self.apply_forces();
        debug!(
            "step: attraction {t_attr:.1?}, repulsion {t_rep:.1?}, community {t_comm:.1?}, integrate {:.1?}",
            t.elapsed() - t_attr - t_rep - t_comm
        );
        out
    }

    pub fn run(&mut self) {
        let t0 = Instant::now();
        for it in 1..=self.params.iterations {
            let (sw, tr) = self.step();
            if it % 25 == 0 || it == self.params.iterations {
                info!(
                    "iteration {it}: speed {:.3}, swinging/traction {:.3}, {:.0?}",
                    self.speed,
                    sw / tr.max(1e-9),
                    t0.elapsed()
                );
            }
        }
        recenter(&mut self.pos);
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(iterations: usize) -> LayoutParams {
        LayoutParams {
            iterations,
            scaling: 2.0,
            gravity: 1.0,
            strong_gravity: false,
            lin_log: false,
            theta: 1.2,
            jitter_tolerance: 1.0,
            community_gravity: 0.0,
            weight_exponent: 1.0,
            seed: 7,
        }
    }

    fn edges(list: &[(u32, u32)]) -> Edges {
        Edges {
            a: list.iter().map(|e| e.0).collect(),
            b: list.iter().map(|e| e.1).collect(),
            weight: vec![1.0; list.len()],
            count: vec![1; list.len()],
        }
    }

    #[test]
    fn tree_matches_brute_force() {
        let mut rng = StdRng::seed_from_u64(3);
        let n = 500;
        let pos: Vec<[f32; 2]> = (0..n)
            .map(|_| [rng.random_range(-50.0..50.0), rng.random_range(-50.0..50.0)])
            .collect();
        let mass: Vec<f32> = (0..n).map(|_| rng.random_range(1.0..5.0)).collect();
        let tree = QuadTree::build(&pos, &mass);
        let mut stack = Vec::new();
        for i in 0..n {
            let mut exact = [0f32; 2];
            for j in 0..n {
                if i == j {
                    continue;
                }
                let d = [pos[i][0] - pos[j][0], pos[i][1] - pos[j][1]];
                let dist2 = d[0] * d[0] + d[1] * d[1];
                exact[0] += d[0] * mass[i] * mass[j] / dist2;
                exact[1] += d[1] * mass[i] * mass[j] / dist2;
            }
            let approx = repulsion(&tree, i, pos[i], mass[i], 0.3, 1.0, &mut stack);
            let err = ((approx[0] - exact[0]).powi(2) + (approx[1] - exact[1]).powi(2)).sqrt();
            let norm = (exact[0].powi(2) + exact[1].powi(2)).sqrt().max(1e-3);
            assert!(err / norm < 0.05, "node {i}: relative error {}", err / norm);
        }
    }

    #[test]
    fn disconnected_cliques_separate() {
        let mut list = Vec::new();
        for c in 0..2u32 {
            for i in 0..6 {
                for j in i + 1..6 {
                    list.push((c * 6 + i, c * 6 + j));
                }
            }
        }
        let community = [0u32, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1];
        let mut layout = Layout::new(12, &edges(&list), &community, None, params(300));
        layout.run();
        let centroid = |c: usize| {
            let pts = &layout.pos[c * 6..c * 6 + 6];
            let (x, y) = pts
                .iter()
                .fold((0.0, 0.0), |(x, y), p| (x + p[0], y + p[1]));
            [x / 6.0, y / 6.0]
        };
        let (c0, c1) = (centroid(0), centroid(1));
        let gap = ((c0[0] - c1[0]).powi(2) + (c0[1] - c1[1]).powi(2)).sqrt();
        let spread = layout.pos[..6]
            .iter()
            .map(|p| ((p[0] - c0[0]).powi(2) + (p[1] - c0[1]).powi(2)).sqrt())
            .fold(0.0, f32::max);
        assert!(gap > 2.0 * spread, "gap {gap} spread {spread}");
    }
}
