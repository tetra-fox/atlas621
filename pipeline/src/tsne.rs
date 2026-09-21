use std::time::Instant;

use log::info;
use nalgebra::DMatrix;
use rand::prelude::*;
use rayon::prelude::*;

use crate::csr::Csr;
use crate::edges::Edges;
use crate::embed::Embedding;
use crate::quadtree::QuadTree;

pub struct TsneParams {
    pub exaggeration: f64,
    pub iterations: usize,
    pub theta: f32,
    pub region_perplexity: f64,
    pub extent: f32,
    pub seed: u64,
}

struct Optimizer {
    update: Vec<[f64; 2]>,
    gains: Vec<[f64; 2]>,
    learning_rate: f64,
}

const MOMENTUM: f64 = 0.8;
const MAX_STEP: f64 = 5.0;
const MIN_GAIN: f64 = 0.01;

impl Optimizer {
    fn new(n: usize, learning_rate: f64) -> Optimizer {
        Optimizer {
            update: vec![[0.0; 2]; n],
            gains: vec![[1.0; 2]; n],
            learning_rate,
        }
    }

    fn step(&mut self, pos: &mut [[f64; 2]], grad: &[[f64; 2]]) {
        let lr = self.learning_rate;
        pos.par_iter_mut()
            .zip(&mut self.update)
            .zip(&mut self.gains)
            .zip(grad)
            .for_each(|(((p, u), g), d)| {
                for c in 0..2 {
                    g[c] = if (u[c] < 0.0) != (d[c] < 0.0) {
                        g[c] + 0.2
                    } else {
                        g[c] * 0.8 + MIN_GAIN
                    };
                    u[c] = MOMENTUM * u[c] - lr * g[c] * d[c];
                }
                let norm = (u[0] * u[0] + u[1] * u[1]).sqrt();
                if norm > MAX_STEP {
                    u[0] *= MAX_STEP / norm;
                    u[1] *= MAX_STEP / norm;
                }
                p[0] += u[0];
                p[1] += u[1];
            });
        recenter(pos);
    }
}

fn recenter(pos: &mut [[f64; 2]]) {
    let n = pos.len() as f64;
    let (mx, my) = pos
        .iter()
        .fold((0.0, 0.0), |(x, y), p| (x + p[0], y + p[1]));
    for p in pos.iter_mut() {
        p[0] -= mx / n;
        p[1] -= my / n;
    }
}

fn row_affinities(d2: &[f64], perplexity: f64) -> Vec<f64> {
    let target = perplexity.ln();
    let (mut lo, mut hi) = (0.0f64, f64::INFINITY);
    let mut beta = 1.0;
    let mut p = vec![0.0; d2.len()];
    let min = d2.iter().copied().fold(f64::INFINITY, f64::min);
    for _ in 0..200 {
        let mut sum = 0.0;
        for (q, &d) in p.iter_mut().zip(d2) {
            *q = (-beta * (d - min)).exp();
            sum += *q;
        }
        let mut entropy = 0.0;
        for q in p.iter_mut() {
            *q /= sum;
            if *q > 1e-300 {
                entropy -= *q * q.ln();
            }
        }
        let diff = entropy - target;
        if diff.abs() < 1e-5 {
            break;
        }
        if diff > 0.0 {
            lo = beta;
            beta = if hi.is_finite() { (beta + hi) / 2.0 } else { beta * 2.0 };
        } else {
            hi = beta;
            beta = (beta + lo) / 2.0;
        }
    }
    p
}

fn exact_tsne(dist: &[f64], k: usize, perplexity: f64) -> Vec<[f64; 2]> {
    let perplexity = perplexity.min((k - 1) as f64 / 3.0);
    let mut p = vec![0.0; k * k];
    for i in 0..k {
        let others: Vec<usize> = (0..k).filter(|&j| j != i).collect();
        let d2: Vec<f64> = others.iter().map(|&j| dist[i * k + j].powi(2)).collect();
        for (&j, q) in others.iter().zip(row_affinities(&d2, perplexity)) {
            p[i * k + j] = q;
        }
    }
    for i in 0..k {
        for j in i + 1..k {
            let s = (p[i * k + j] + p[j * k + i]) / (2.0 * k as f64);
            p[i * k + j] = s;
            p[j * k + i] = s;
        }
    }
    let deg: Vec<f64> = (0..k).map(|i| (0..k).map(|j| p[i * k + j]).sum()).collect();
    let m = DMatrix::from_fn(k, k, |i, j| p[i * k + j] / (deg[i] * deg[j]).sqrt().max(1e-12));
    let eig = m.symmetric_eigen();
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_unstable_by(|&a, &b| eig.eigenvalues[b].total_cmp(&eig.eigenvalues[a]));
    let mut pos: Vec<[f64; 2]> = (0..k)
        .map(|i| [eig.eigenvectors[(i, order[1])], eig.eigenvectors[(i, order[2])]])
        .collect();
    rescale(&mut pos, 1e-4);
    let mut grad = vec![[0.0; 2]; k];
    for (iterations, exaggeration) in [(250, 12.0), (500, 1.0)] {
        let mut opt = Optimizer::new(k, k as f64 / exaggeration);
        for _ in 0..iterations {
            let mut z = 0.0;
            for i in 0..k {
                for j in 0..k {
                    if i != j {
                        let d = [pos[i][0] - pos[j][0], pos[i][1] - pos[j][1]];
                        z += 1.0 / (1.0 + d[0] * d[0] + d[1] * d[1]);
                    }
                }
            }
            for i in 0..k {
                let mut g = [0.0; 2];
                for j in 0..k {
                    if i == j {
                        continue;
                    }
                    let d = [pos[i][0] - pos[j][0], pos[i][1] - pos[j][1]];
                    let q = 1.0 / (1.0 + d[0] * d[0] + d[1] * d[1]);
                    let f = exaggeration * p[i * k + j] * q - q * q / z;
                    g[0] += f * d[0];
                    g[1] += f * d[1];
                }
                grad[i] = g;
            }
            opt.step(&mut pos, &grad);
        }
    }
    pos
}

fn rescale(pos: &mut [[f64; 2]], target: f64) {
    recenter(pos);
    let var = pos.iter().map(|p| p[0] * p[0] + p[1] * p[1]).sum::<f64>() / (2.0 * pos.len() as f64);
    let s = target / var.sqrt().max(1e-300);
    for p in pos.iter_mut() {
        p[0] *= s;
        p[1] *= s;
    }
}

fn place_regions(graph: &Edges, region: &[u32], k: usize, perplexity: f64) -> Vec<[f64; 2]> {
    let mut tie = vec![0.0f64; k * k];
    for e in graph.iter() {
        let (a, b) = (region[e.a as usize] as usize, region[e.b as usize] as usize);
        if a != b {
            tie[a * k + b] += e.weight as f64;
            tie[b * k + a] += e.weight as f64;
        }
    }
    let deg: Vec<f64> = (0..k).map(|i| (0..k).map(|j| tie[i * k + j]).sum::<f64>().max(1.0)).collect();
    let mut norm = vec![0.0; k * k];
    let mut max = 0.0f64;
    for i in 0..k {
        for j in 0..k {
            norm[i * k + j] = tie[i * k + j] / (deg[i] * deg[j]).sqrt();
            max = max.max(norm[i * k + j]);
        }
    }
    let dist: Vec<f64> = (0..k * k)
        .map(|x| if x / k == x % k { 0.0 } else { 1.0 - norm[x] / max.max(1e-12) })
        .collect();
    exact_tsne(&dist, k, perplexity)
}

pub fn affinity_edges(emb: &Embedding, perplexity: f64, tail_neighbors: usize) -> Edges {
    let t0 = Instant::now();
    let n = emb.core.len();
    let kn = emb.core_knn.len() / n;
    let k = ((3.0 * perplexity) as usize).min(kn);
    let rows: Vec<Vec<f64>> = (0..n)
        .into_par_iter()
        .map(|i| {
            let d2: Vec<f64> = emb.core_knn_sim[i * kn..i * kn + k]
                .iter()
                .map(|&s| (2.0 - 2.0 * s as f64).max(0.0))
                .collect();
            row_affinities(&d2, perplexity)
        })
        .collect();
    let mut entries: Vec<(u32, u32, f32)> = Vec::with_capacity(n * k);
    for (i, row) in rows.iter().enumerate() {
        for (t, &p) in row.iter().enumerate() {
            let j = emb.core_knn[i * kn + t] as usize;
            entries.push((i.min(j) as u32, i.max(j) as u32, p as f32));
        }
    }
    entries.par_sort_unstable_by(|x, y| (x.0, x.1).cmp(&(y.0, y.1)));
    let mut edges = Edges::default();
    let mut last = (u32::MAX, u32::MAX);
    for (i, j, p) in entries {
        if (i, j) == last {
            *edges.weight.last_mut().unwrap() += p;
        } else {
            edges.push(emb.core[i as usize], emb.core[j as usize], p, 0);
            last = (i, j);
        }
    }
    let core_pairs = edges.len();
    let kept = emb.tail_knn.len() / emb.tail.len().max(1);
    let kt = tail_neighbors.min(kept);
    for (x, &node) in emb.tail.iter().enumerate() {
        let row = &emb.tail_knn[x * kept..x * kept + kt];
        let sims = &emb.tail_knn_sim[x * kept..x * kept + kt];
        let mut total = 0.0f32;
        for (&j, &s) in row.iter().zip(sims) {
            if j != u32::MAX {
                total += s.max(0.0);
            }
        }
        if total <= 0.0 {
            continue;
        }
        for (&j, &s) in row.iter().zip(sims) {
            if j != u32::MAX {
                edges.push(node, emb.core[j as usize], s.max(0.0) / total, 0);
            }
        }
    }
    info!(
        "affinity graph: {core_pairs} core pairs at perplexity {perplexity} plus {} tail links, {:.0?}",
        edges.len() - core_pairs,
        t0.elapsed()
    );
    edges
}

fn affinity_csr(n: usize, emb: &Embedding, graph: &Edges) -> Csr<f32> {
    let nc = emb.core.len();
    let mut slot = vec![u32::MAX; n];
    for (i, &node) in emb.core.iter().enumerate() {
        slot[node as usize] = i as u32;
    }
    let scale = 1.0 / (2.0 * nc as f32);
    let links: Vec<(u32, u32, f32)> = graph
        .iter()
        .filter_map(|e| {
            let (a, b) = (slot[e.a as usize], slot[e.b as usize]);
            (a != u32::MAX && b != u32::MAX).then_some((a, b, e.weight * scale))
        })
        .collect();
    Csr::symmetric(nc, links.iter().copied())
}

fn gradient(pos: &[[f64; 2]], p: &Csr<f32>, exaggeration: f64, theta: f32, grad: &mut [[f64; 2]]) {
    let n = pos.len();
    let pos32: Vec<[f32; 2]> = pos.iter().map(|p| [p[0] as f32, p[1] as f32]).collect();
    let tree = QuadTree::build(&pos32, &vec![1.0; n]);
    let theta2 = theta * theta;
    let parts: Vec<([f64; 2], [f64; 2], f64)> = (0..n)
        .into_par_iter()
        .map_init(
            || Vec::with_capacity(256),
            |stack, i| {
                let pi = pos[i];
                let mut attr = [0.0; 2];
                for (j, w) in p.row(i) {
                    let pj = pos[j as usize];
                    let d = [pi[0] - pj[0], pi[1] - pj[1]];
                    let q = 1.0 / (1.0 + d[0] * d[0] + d[1] * d[1]);
                    let f = exaggeration * w as f64 * q;
                    attr[0] += f * d[0];
                    attr[1] += f * d[1];
                }
                let mut rep = [0.0; 2];
                let mut z = 0.0;
                stack.clear();
                stack.push(0u32);
                while let Some(ci) = stack.pop() {
                    let cell = &tree.cells[ci as usize];
                    if cell.count == 0 || (cell.count == 1 && cell.body == i as u32) {
                        continue;
                    }
                    let com = cell.center_of_mass();
                    let d = [pi[0] - com[0] as f64, pi[1] - com[1] as f64];
                    let dist2 = d[0] * d[0] + d[1] * d[1];
                    let width = cell.half * 2.0;
                    let width2 = (width * width) as f64;
                    if cell.is_leaf() || width2 < theta2 as f64 * dist2 {
                        let q = 1.0 / (1.0 + dist2);
                        let m = cell.count as f64;
                        z += m * q;
                        rep[0] += m * q * q * d[0];
                        rep[1] += m * q * q * d[1];
                    } else {
                        stack.extend_from_slice(&cell.child);
                    }
                }
                (attr, rep, z)
            },
        )
        .collect();
    let z: f64 = parts.iter().map(|p| p.2).sum::<f64>().max(1e-300);
    grad.par_iter_mut().zip(&parts).for_each(|(g, (attr, rep, _))| {
        g[0] = attr[0] - rep[0] / z;
        g[1] = attr[1] - rep[1] / z;
    });
}

fn spacing(pos: &[[f64; 2]]) -> Vec<f64> {
    const K: usize = 8;
    let n = pos.len();
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in pos {
        for d in 0..2 {
            lo[d] = lo[d].min(p[d]);
            hi[d] = hi[d].max(p[d]);
        }
    }
    let cols = ((n as f64).sqrt() / 2.0).max(1.0) as usize;
    let cell = ((hi[0] - lo[0]).max(hi[1] - lo[1]) / cols as f64).max(1e-9);
    let coord = |p: &[f64; 2]| {
        [
            (((p[0] - lo[0]) / cell) as usize).min(cols),
            (((p[1] - lo[1]) / cell) as usize).min(cols),
        ]
    };
    let side = cols + 1;
    let mut count = vec![0usize; side * side + 1];
    for p in pos {
        let c = coord(p);
        count[c[1] * side + c[0] + 1] += 1;
    }
    for i in 1..count.len() {
        count[i] += count[i - 1];
    }
    let mut fill = count.clone();
    let mut members = vec![0u32; n];
    for (i, p) in pos.iter().enumerate() {
        let c = coord(p);
        members[fill[c[1] * side + c[0]]] = i as u32;
        fill[c[1] * side + c[0]] += 1;
    }
    (0..n)
        .into_par_iter()
        .map(|i| {
            let c = coord(&pos[i]);
            let mut best: Vec<f64> = Vec::new();
            for ring in 0..=side {
                let x0 = c[0].saturating_sub(ring);
                let x1 = (c[0] + ring).min(cols);
                let y0 = c[1].saturating_sub(ring);
                let y1 = (c[1] + ring).min(cols);
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        if ring > 0 && x != x0 && x != x1 && y != y0 && y != y1 {
                            continue;
                        }
                        for &j in &members[count[y * side + x]..count[y * side + x + 1]] {
                            if j as usize == i {
                                continue;
                            }
                            let q = pos[j as usize];
                            let d = ((q[0] - pos[i][0]).powi(2) + (q[1] - pos[i][1]).powi(2)).sqrt();
                            if best.len() < K {
                                best.push(d);
                                best.sort_by(f64::total_cmp);
                            } else if d < best[K - 1] {
                                best[K - 1] = d;
                                best.sort_by(f64::total_cmp);
                            }
                        }
                    }
                }
                if best.len() == K && best[K - 1] <= ring as f64 * cell {
                    break;
                }
            }
            best.last().copied().unwrap_or(cell)
        })
        .collect()
}

pub fn layout(
    n: usize,
    emb: &Embedding,
    graph: &Edges,
    region: &[u32],
    previous: Option<&[[f32; 2]]>,
    params: &TsneParams,
) -> Vec<[f32; 2]> {
    let t0 = Instant::now();
    let nc = emb.core.len();
    let k = region.iter().copied().max().map_or(0, |r| r as usize + 1);
    let mut centers = place_regions(graph, region, k, params.region_perplexity);
    rescale(&mut centers, 1e-4);
    info!("{k} regions placed by their ties, {:.0?}", t0.elapsed());
    let mut rng = StdRng::seed_from_u64(params.seed);
    let mut pos: Vec<[f64; 2]> = match previous {
        None => emb
            .core
            .iter()
            .map(|&node| {
                let c = centers[region[node as usize] as usize];
                [c[0] + jitter(&mut rng), c[1] + jitter(&mut rng)]
            })
            .collect(),
        Some(prev) => {
            let known = |p: &[f32; 2]| !p[0].is_nan() && !p[1].is_nan();
            let mut old: Vec<[f64; 2]> = emb
                .core
                .iter()
                .map(|&i| [prev[i as usize][0] as f64, prev[i as usize][1] as f64])
                .collect();
            let seen: Vec<bool> = emb.core.iter().map(|&i| known(&prev[i as usize])).collect();
            let mut sum = vec![[0.0f64; 2]; k];
            let mut count = vec![0usize; k];
            for (i, &node) in emb.core.iter().enumerate() {
                if seen[i] {
                    let r = region[node as usize] as usize;
                    sum[r][0] += old[i][0];
                    sum[r][1] += old[i][1];
                    count[r] += 1;
                }
            }
            let kept: Vec<[f64; 2]> = old.iter().zip(&seen).filter(|(_, s)| **s).map(|(p, _)| *p).collect();
            let mut scaled = kept.clone();
            rescale(&mut scaled, 1e-4);
            let factor = if kept.len() > 1 {
                (scaled[0][0] - scaled[1][0]).hypot(scaled[0][1] - scaled[1][1])
                    / (kept[0][0] - kept[1][0]).hypot(kept[0][1] - kept[1][1]).max(1e-300)
            } else {
                1.0
            };
            let (mx, my) = kept.iter().fold((0.0, 0.0), |(x, y), p| (x + p[0], y + p[1]));
            let (mx, my) = (mx / kept.len().max(1) as f64, my / kept.len().max(1) as f64);
            let mut from_region = 0;
            let mut from_ties = 0;
            for (i, &node) in emb.core.iter().enumerate() {
                let r = region[node as usize] as usize;
                old[i] = if seen[i] {
                    [(old[i][0] - mx) * factor, (old[i][1] - my) * factor]
                } else if count[r] > 0 {
                    from_region += 1;
                    let c = [(sum[r][0] / count[r] as f64 - mx) * factor, (sum[r][1] / count[r] as f64 - my) * factor];
                    [c[0] + jitter(&mut rng), c[1] + jitter(&mut rng)]
                } else {
                    from_ties += 1;
                    let c = centers[r];
                    [c[0] + jitter(&mut rng), c[1] + jitter(&mut rng)]
                };
            }
            info!(
                "warm start: {} core tags keep their previous position, {from_region} start at their region's, {from_ties} where the ties put their region",
                kept.len()
            );
            old
        }
    };
    let p = affinity_csr(n, emb, graph);
    info!("affinities: {} entries over {nc} core tags, {:.0?}", p.len(), t0.elapsed());
    let mut opt = Optimizer::new(nc, nc as f64 / params.exaggeration);
    let mut grad = vec![[0.0; 2]; nc];
    for it in 1..=params.iterations {
        gradient(&pos, &p, params.exaggeration, params.theta, &mut grad);
        opt.step(&mut pos, &grad);
        if it % 50 == 0 || it == params.iterations {
            let spread = (pos.iter().map(|p| p[0] * p[0] + p[1] * p[1]).sum::<f64>() / nc as f64).sqrt();
            info!(
                "iteration {it}: exaggeration {}, rms radius {spread:.1}, {:.0?}",
                params.exaggeration,
                t0.elapsed()
            );
        }
    }
    drop(p);

    let space = spacing(&pos);
    let kt = emb.tail_knn.len() / emb.tail.len().max(1);
    let mut all = vec![[0.0f64; 2]; n];
    for (i, &node) in emb.core.iter().enumerate() {
        all[node as usize] = pos[i];
    }
    let mut unplaced = 0;
    let median = {
        let mut s = space.clone();
        s.sort_by(f64::total_cmp);
        s[s.len() / 2]
    };
    for (x, &node) in emb.tail.iter().enumerate() {
        let nb = &emb.tail_knn[x * kt..(x + 1) * kt];
        let sim = &emb.tail_knn_sim[x * kt..(x + 1) * kt];
        let (anchor, r0) = if nb[0] == u32::MAX {
            unplaced += 1;
            ([0.0, 0.0], median)
        } else {
            let p0 = pos[nb[0] as usize];
            let r0 = space[nb[0] as usize].min(median);
            let (mut sx, mut sy, mut sw) = (0.0, 0.0, 0.0);
            for (&j, &s) in nb.iter().zip(sim) {
                if j == u32::MAX {
                    continue;
                }
                let q = pos[j as usize];
                if ((q[0] - p0[0]).powi(2) + (q[1] - p0[1]).powi(2)).sqrt() <= 2.0 * r0 {
                    let w = s.max(0.0) as f64 + 1e-6;
                    sx += q[0] * w;
                    sy += q[1] * w;
                    sw += w;
                }
            }
            ([sx / sw, sy / sw], r0)
        };
        let angle = rng.random::<f64>() * std::f64::consts::TAU;
        let radius = r0 * rng.random::<f64>().sqrt();
        all[node as usize] = [anchor[0] + radius * angle.cos(), anchor[1] + radius * angle.sin()];
    }
    recenter(&mut all);
    let max = all
        .iter()
        .map(|p| p[0].abs().max(p[1].abs()))
        .fold(0.0, f64::max)
        .max(1e-9);
    let s = params.extent as f64 / max;
    info!(
        "{} tail tags placed next to the core tags they resemble, {unplaced} with no core pair put at the centre, {:.0?}",
        emb.tail.len(),
        t0.elapsed()
    );
    all.iter().map(|p| [(p[0] * s) as f32, (p[1] * s) as f32]).collect()
}

fn jitter(rng: &mut StdRng) -> f64 {
    let (u, v): (f64, f64) = (rng.random_range(f64::EPSILON..1.0), rng.random());
    2e-5 * (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affinities_hit_the_perplexity() {
        let d2: Vec<f64> = (1..=60).map(|i| (i as f64).powi(2) / 100.0).collect();
        let p = row_affinities(&d2, 10.0);
        let entropy: f64 = -p.iter().filter(|&&q| q > 0.0).map(|q| q * q.ln()).sum::<f64>();
        assert!((entropy.exp() - 10.0).abs() < 0.01, "perplexity {}", entropy.exp());
        assert!((p.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn stored_affinities_rebuild_a_matrix_summing_to_one() {
        let emb = Embedding {
            core: vec![0, 2, 3],
            dim: 1,
            vectors: vec![],
            core_knn: vec![1, 2, 0, 2, 0, 1],
            core_knn_sim: vec![0.9, 0.5, 0.9, 0.7, 0.5, 0.7],
            tail: vec![1],
            tail_knn: vec![0, u32::MAX],
            tail_knn_sim: vec![0.8, f32::NEG_INFINITY],
        };
        let edges = affinity_edges(&emb, 1.0, 2);
        let tail: Vec<f32> = edges.iter().filter(|e| e.a == 1).map(|e| e.weight).collect();
        assert_eq!(tail, vec![1.0]);
        let p = affinity_csr(4, &emb, &edges);
        assert_eq!(p.len(), 6);
        let total: f32 = (0..3).flat_map(|i| p.row(i).map(|(_, v)| v)).sum();
        assert!((total - 1.0).abs() < 1e-5, "total {total}");
    }

    #[test]
    fn tree_gradient_matches_brute_force() {
        let mut rng = StdRng::seed_from_u64(5);
        let n = 400;
        let pos: Vec<[f64; 2]> = (0..n)
            .map(|_| [rng.random_range(-20.0..20.0), rng.random_range(-20.0..20.0)])
            .collect();
        let p = Csr::directed(n, std::iter::empty::<(u32, u32, f32)>());
        let mut approx = vec![[0.0; 2]; n];
        gradient(&pos, &p, 1.0, 0.2, &mut approx);
        let mut z = 0.0;
        for i in 0..n {
            for j in 0..n {
                if i != j {
                    let d = [pos[i][0] - pos[j][0], pos[i][1] - pos[j][1]];
                    z += 1.0 / (1.0 + d[0] * d[0] + d[1] * d[1]);
                }
            }
        }
        for i in 0..n {
            let mut exact = [0.0; 2];
            for j in 0..n {
                if i == j {
                    continue;
                }
                let d = [pos[i][0] - pos[j][0], pos[i][1] - pos[j][1]];
                let q = 1.0 / (1.0 + d[0] * d[0] + d[1] * d[1]);
                exact[0] -= q * q * d[0] / z;
                exact[1] -= q * q * d[1] / z;
            }
            let err = ((approx[i][0] - exact[0]).powi(2) + (approx[i][1] - exact[1]).powi(2)).sqrt();
            let norm = (exact[0].powi(2) + exact[1].powi(2)).sqrt().max(1e-9);
            assert!(err / norm < 0.05, "point {i}: relative error {}", err / norm);
        }
    }

    #[test]
    fn tied_regions_sit_together() {
        let k = 12;
        let mut dist = vec![1.0; k * k];
        for i in 0..k {
            for j in 0..k {
                if i == j {
                    dist[i * k + j] = 0.0;
                } else if (i < 6) == (j < 6) {
                    dist[i * k + j] = 0.2;
                }
            }
        }
        let pos = exact_tsne(&dist, k, 3.0);
        let within = |a: usize, b: usize| ((pos[a][0] - pos[b][0]).powi(2) + (pos[a][1] - pos[b][1]).powi(2)).sqrt();
        let same = within(0, 1).max(within(6, 7));
        let across = within(0, 6).min(within(1, 7));
        assert!(across > 2.0 * same, "same {same} across {across}");
    }
}
