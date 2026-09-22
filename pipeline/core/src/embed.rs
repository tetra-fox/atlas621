use std::time::Instant;

use nalgebra::DMatrix;
use rand::prelude::*;
use rayon::prelude::*;
use tracing::{debug, info, trace};

use crate::csr::Csr;
use crate::edges::Edges;
use crate::pairs::Pairs;
use crate::rng::normal;

pub struct EmbedParams {
    pub core_floor: u32,
    pub min_count: u32,
    pub dim: usize,
    pub oversample: usize,
    pub power_iterations: usize,
    pub shift: f64,
    pub neighbors: usize,
    pub tail_neighbors: usize,
    pub seed: u64,
}

pub struct Embedding {
    pub core: Vec<u32>,
    pub dim: usize,
    pub vectors: Vec<f32>,
    pub core_knn: Vec<u32>,
    pub core_knn_sim: Vec<f32>,
    pub tail: Vec<u32>,
    pub tail_knn: Vec<u32>,
    pub tail_knn_sim: Vec<f32>,
}

fn pmi(count: u32, posts: u64, pa: u32, pb: u32, shift: f64) -> f64 {
    (count as f64 * posts as f64 / (pa as f64 * pb as f64)).ln() - shift.ln()
}

fn sparse_times(a: &Csr<f32>, x: &[f32], w: usize) -> Vec<f32> {
    let n = x.len() / w;
    let mut y = vec![0f32; n * w];
    y.par_chunks_mut(w).enumerate().for_each(|(i, row)| {
        for (j, v) in a.row(i) {
            let xj = &x[j as usize * w..(j as usize + 1) * w];
            for (r, &xv) in row.iter_mut().zip(xj) {
                *r += v * xv;
            }
        }
    });
    y
}

fn gram(x: &[f32], w: usize) -> DMatrix<f64> {
    const ROWS: usize = 4096;
    let parts: Vec<Vec<f64>> = x
        .par_chunks(w * ROWS)
        .map(|block| {
            let mut g = vec![0f64; w * w];
            for row in block.chunks(w) {
                for i in 0..w {
                    let ri = row[i] as f64;
                    for j in i..w {
                        g[i * w + j] += ri * row[j] as f64;
                    }
                }
            }
            g
        })
        .collect();
    let mut sum = vec![0f64; w * w];
    for g in &parts {
        for (s, v) in sum.iter_mut().zip(g) {
            *s += v;
        }
    }
    let mut g = DMatrix::from_row_slice(w, w, &sum);
    for i in 0..w {
        for j in 0..i {
            g[(i, j)] = g[(j, i)];
        }
    }
    g
}

// the gram matrix and its cholesky are f64 but the result rounds back to f32, which leaves the
// columns slightly off orthogonal; one repeat is enough to fix it
fn orthonormalize(x: &mut [f32], w: usize) {
    for _ in 0..2 {
        let g = gram(x, w);
        let l = g
            .cholesky()
            .expect("gram matrix of the range finder is not positive definite")
            .l();
        x.par_chunks_mut(w).for_each(|row| {
            for i in 0..w {
                let mut s = row[i] as f64;
                for j in 0..i {
                    s -= l[(i, j)] * row[j] as f64;
                }
                row[i] = (s / l[(i, i)]) as f32;
            }
        });
    }
}

fn randomized_svd(a: &Csr<f32>, n: usize, params: &EmbedParams) -> (Vec<f32>, Vec<f32>, Vec<f64>) {
    let t0 = Instant::now();
    let w = params.dim + params.oversample;
    debug!(
        dim = params.dim,
        oversample = params.oversample,
        power_iterations = params.power_iterations,
        seed = params.seed,
        rows = n,
        "randomized svd starting"
    );
    let mut rng = StdRng::seed_from_u64(params.seed);
    let omega: Vec<f32> = (0..n * w).map(|_| normal(&mut rng, 1.0) as f32).collect();
    let mut q = sparse_times(a, &omega, w);
    drop(omega);
    orthonormalize(&mut q, w);
    // a is symmetric, so one power iteration of a*a' is two multiplies by a
    for step in 0..params.power_iterations {
        for _ in 0..2 {
            q = sparse_times(a, &q, w);
            orthonormalize(&mut q, w);
        }
        debug!(step = step + 1, of = params.power_iterations, elapsed = ?t0.elapsed(), "power iteration");
    }
    let aq = sparse_times(a, &q, w);
    let c = gram(&aq, w);
    let eig = c.symmetric_eigen();
    let mut order: Vec<usize> = (0..w).collect();
    order.sort_unstable_by(|&i, &j| eig.eigenvalues[j].total_cmp(&eig.eigenvalues[i]));
    let order = &order[..params.dim];
    let sigma: Vec<f64> = order
        .iter()
        .map(|&i| eig.eigenvalues[i].max(0.0).sqrt())
        .collect();
    let rotate = |x: &[f32], scale: &[f64]| -> Vec<f32> {
        let mut out = vec![0f32; n * params.dim];
        out.par_chunks_mut(params.dim)
            .zip(x.par_chunks(w))
            .for_each(|(dst, src)| {
                for (d, &e) in order.iter().enumerate() {
                    let mut s = 0f64;
                    for (i, &v) in src.iter().enumerate() {
                        s += v as f64 * eig.eigenvectors[(i, e)];
                    }
                    dst[d] = (s * scale[d]) as f32;
                }
            });
        out
    };
    let ones = vec![1.0; params.dim];
    let inv_sigma: Vec<f64> = sigma.iter().map(|s| 1.0 / s.max(1e-12)).collect();
    let u = rotate(&q, &ones);
    let v = rotate(&aq, &inv_sigma);
    info!(
        "randomized svd: {} dims (+{} oversampled), {} power iterations, singular values {:.1} .. {:.1}, {:.0?}",
        params.dim,
        params.oversample,
        params.power_iterations,
        sigma[0],
        sigma[params.dim - 1],
        t0.elapsed()
    );
    (u, v, sigma)
}

fn normalize_rows(x: &mut [f32], w: usize) -> usize {
    x.par_chunks_mut(w)
        .map(|row| {
            let norm = row.iter().map(|v| v * v).sum::<f32>().sqrt();
            if norm > 0.0 {
                for v in row.iter_mut() {
                    *v /= norm;
                }
                0
            } else {
                1
            }
        })
        .sum()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0f32; 8];
    for (ca, cb) in a.as_chunks::<8>().0.iter().zip(b.as_chunks::<8>().0) {
        for l in 0..8 {
            acc[l] += ca[l] * cb[l];
        }
    }
    acc.iter().sum()
}

fn nearest(
    query: &[f32],
    base: &[f32],
    w: usize,
    k: usize,
    skip: impl Fn(usize) -> usize + Sync,
) -> (Vec<u32>, Vec<f32>) {
    let nq = query.len() / w;
    let nb = base.len() / w;
    const QBLOCK: usize = 64;
    const BBLOCK: usize = 512;
    let mut idx = vec![u32::MAX; nq * k];
    let mut sim = vec![f32::NEG_INFINITY; nq * k];
    idx.par_chunks_mut(QBLOCK * k)
        .zip(sim.par_chunks_mut(QBLOCK * k))
        .enumerate()
        .for_each_init(
            || (vec![0f32; QBLOCK * nb], vec![0u32; nb]),
            |(scores, order), (qb, (idx, sim))| {
                let q0 = qb * QBLOCK;
                let rows = (nq - q0).min(QBLOCK);
                for b0 in (0..nb).step_by(BBLOCK) {
                    let b1 = (b0 + BBLOCK).min(nb);
                    for r in 0..rows {
                        let qrow = &query[(q0 + r) * w..(q0 + r + 1) * w];
                        for j in b0..b1 {
                            scores[r * nb + j] = dot(qrow, &base[j * w..(j + 1) * w]);
                        }
                    }
                }
                for r in 0..rows {
                    let own = skip(q0 + r);
                    let s = &mut scores[r * nb..(r + 1) * nb];
                    if own < nb {
                        s[own] = f32::NEG_INFINITY;
                    }
                    for (o, j) in order.iter_mut().zip(0..nb as u32) {
                        *o = j;
                    }
                    order.select_nth_unstable_by(k, |&x, &y| {
                        s[y as usize].total_cmp(&s[x as usize])
                    });
                    let top = &mut order[..k];
                    top.sort_unstable_by(|&x, &y| s[y as usize].total_cmp(&s[x as usize]));
                    for (t, &j) in top.iter().enumerate() {
                        idx[r * k + t] = j;
                        sim[r * k + t] = s[j as usize];
                    }
                }
            },
        );
    (idx, sim)
}

pub fn embed(pairs: &Pairs, post_counts: &[u32], posts: u64, params: &EmbedParams) -> Embedding {
    let t0 = Instant::now();
    let n = post_counts.len();
    assert!(
        params.dim.is_multiple_of(8),
        "the dot product runs eight lanes wide"
    );
    let mut core = Vec::new();
    let mut tail = Vec::new();
    let mut row = vec![u32::MAX; n];
    for i in 0..n {
        if post_counts[i] >= params.core_floor {
            row[i] = core.len() as u32;
            core.push(i as u32);
        } else {
            row[i] = tail.len() as u32;
            tail.push(i as u32);
        }
    }
    let is_core = |i: u32| post_counts[i as usize] >= params.core_floor;

    let entries: Vec<(u32, u32, f32)> = pairs
        .keys
        .par_iter()
        .zip(&pairs.counts)
        .filter_map(|(&key, &c)| {
            let (a, b) = ((key >> 32) as u32, key as u32);
            if c < params.min_count || !is_core(a) || !is_core(b) {
                return None;
            }
            let v = pmi(
                c,
                posts,
                post_counts[a as usize],
                post_counts[b as usize],
                params.shift,
            );
            (v > 0.0).then_some((row[a as usize], row[b as usize], v as f32))
        })
        .collect();
    trace!(
        core = core.len(),
        tail = tail.len(),
        "split tags by the core floor"
    );
    let matrix = Csr::symmetric(core.len(), entries.iter().copied());
    info!(
        "ppmi over {} core tags (>= {} posts): {} positive pairs of {} with count >= {}, {:.0?}",
        core.len(),
        params.core_floor,
        entries.len(),
        pairs.keys.len(),
        params.min_count,
        t0.elapsed()
    );
    drop(entries);
    let (mut vectors, v, sigma) = randomized_svd(&matrix, core.len(), params);
    drop(matrix);
    vectors.par_chunks_mut(params.dim).for_each(|r| {
        for (x, s) in r.iter_mut().zip(&sigma) {
            *x *= s.sqrt() as f32;
        }
    });
    let zero_core = normalize_rows(&mut vectors, params.dim);

    let tail_entries: Vec<(u32, u32, f32)> = pairs
        .keys
        .par_iter()
        .zip(&pairs.counts)
        .filter_map(|(&key, &c)| {
            let (a, b) = ((key >> 32) as u32, key as u32);
            let (t, k) = match (is_core(a), is_core(b)) {
                (false, true) => (a, b),
                (true, false) => (b, a),
                _ => return None,
            };
            let v = pmi(
                c,
                posts,
                post_counts[t as usize],
                post_counts[k as usize],
                params.shift,
            );
            (v > 0.0).then_some((row[t as usize], row[k as usize], v as f32))
        })
        .collect();
    let rows = Csr::directed(tail.len(), tail_entries.iter().copied());
    drop(tail_entries);
    let scale: Vec<f32> = sigma
        .iter()
        .map(|s| (1.0 / s.max(1e-12).sqrt()) as f32)
        .collect();
    let mut tail_vectors = vec![0f32; tail.len() * params.dim];
    tail_vectors
        .par_chunks_mut(params.dim)
        .enumerate()
        .for_each(|(x, out)| {
            for (j, r) in rows.row(x) {
                let vj = &v[j as usize * params.dim..(j as usize + 1) * params.dim];
                for d in 0..params.dim {
                    out[d] += r * vj[d] * scale[d];
                }
            }
        });
    let no_row: Vec<bool> = (0..tail.len()).map(|x| rows.degree(x) == 0).collect();
    drop(rows);
    drop(v);
    let zero_tail = normalize_rows(&mut tail_vectors, params.dim);
    info!(
        "{} tail tags folded in, {} of them with no core pair at all ({zero_core} core rows and {zero_tail} tail rows are zero), {:.0?}",
        tail.len(),
        no_row.iter().filter(|&&z| z).count(),
        t0.elapsed()
    );

    let (core_knn, core_knn_sim) = nearest(&vectors, &vectors, params.dim, params.neighbors, |i| i);
    info!(
        "{} nearest core tags per core tag, {:.0?}",
        params.neighbors,
        t0.elapsed()
    );
    let (mut tail_knn, mut tail_knn_sim) = nearest(
        &tail_vectors,
        &vectors,
        params.dim,
        params.tail_neighbors,
        |_| usize::MAX,
    );
    for (x, &empty) in no_row.iter().enumerate() {
        if empty {
            let range = x * params.tail_neighbors..(x + 1) * params.tail_neighbors;
            tail_knn[range.clone()].fill(u32::MAX);
            tail_knn_sim[range].fill(0.0);
        }
    }
    info!(
        "{} nearest core tags per tail tag, {:.0?}",
        params.tail_neighbors,
        t0.elapsed()
    );
    Embedding {
        core,
        dim: params.dim,
        vectors,
        core_knn,
        core_knn_sim,
        tail,
        tail_knn,
        tail_knn_sim,
    }
}

pub fn knn_edges(emb: &Embedding, k: usize, kt: usize, pairs: &Pairs) -> Edges {
    let t0 = Instant::now();
    let kn = emb.core_knn.len() / emb.core.len();
    let kept = emb.tail_knn.len() / emb.tail.len().max(1);
    assert!(
        k <= kn && kt <= kept,
        "the graph asks for more neighbors than were kept"
    );
    let mut list: Vec<(u32, u32, f32)> =
        Vec::with_capacity(emb.core.len() * k + emb.tail.len() * kt);
    let mut push = |a: u32, b: u32, w: f32| {
        if a != b {
            list.push((a.min(b), a.max(b), w.clamp(0.0, 1.0)));
        }
    };
    for (i, &node) in emb.core.iter().enumerate() {
        for t in 0..k {
            let j = emb.core_knn[i * kn + t];
            push(node, emb.core[j as usize], emb.core_knn_sim[i * kn + t]);
        }
    }
    for (x, &node) in emb.tail.iter().enumerate() {
        for t in 0..kt {
            let j = emb.tail_knn[x * kept + t];
            if j != u32::MAX {
                push(node, emb.core[j as usize], emb.tail_knn_sim[x * kept + t]);
            }
        }
    }
    list.par_sort_unstable_by(|x, y| (x.0, x.1).cmp(&(y.0, y.1)));
    list.dedup_by(|x, y| x.0 == y.0 && x.1 == y.1);
    let mut order: Vec<u32> = (0..pairs.keys.len() as u32).collect();
    order.par_sort_unstable_by_key(|&p| pairs.keys[p as usize]);
    let mut edges = Edges::default();
    for (a, b, w) in list {
        let key = Pairs::key(a, b);
        let count = match order.binary_search_by(|&p| pairs.keys[p as usize].cmp(&key)) {
            Ok(p) => pairs.counts[order[p] as usize],
            Err(_) => 0,
        };
        edges.push(a, b, w, count);
    }
    info!("knn graph: {} edges, {:.0?}", edges.len(), t0.elapsed());
    edges
}
