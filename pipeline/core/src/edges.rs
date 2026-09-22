use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::csr::Csr;
use crate::pairs::Pairs;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum Weight {
    Cosine,
    Npmi,
}

#[derive(Serialize, Deserialize)]
pub struct EdgesMeta {
    pub weight: Weight,
    pub core_floor: u32,
    pub tail_neighbors: usize,
}

pub struct EdgeParams {
    pub weight: Weight,
    pub posts: u64,
    pub top_k: usize,
    pub min_count: u32,
    pub min_weight: f32,
    pub min_degree: usize,
    pub core_floor: u32,
    pub small_k: usize,
}

#[derive(Default)]
pub struct Edges {
    pub a: Vec<u32>,
    pub b: Vec<u32>,
    pub weight: Vec<f32>,
    pub count: Vec<u32>,
}

#[derive(Clone, Copy)]
pub struct Edge {
    pub a: u32,
    pub b: u32,
    pub weight: f32,
    pub count: u32,
}

impl Edges {
    pub fn len(&self) -> usize {
        self.a.len()
    }

    pub fn is_empty(&self) -> bool {
        self.a.is_empty()
    }

    pub fn push(&mut self, a: u32, b: u32, weight: f32, count: u32) {
        self.a.push(a);
        self.b.push(b);
        self.weight.push(weight);
        self.count.push(count);
    }

    pub fn iter(&self) -> impl Iterator<Item = Edge> + Clone + '_ {
        self.a
            .iter()
            .zip(&self.b)
            .zip(&self.weight)
            .zip(&self.count)
            .map(|(((&a, &b), &weight), &count)| Edge {
                a,
                b,
                weight,
                count,
            })
    }
}

fn cosine(count: u32, pa: u32, pb: u32) -> f32 {
    count as f32 / ((pa as f64) * (pb as f64)).sqrt() as f32
}

fn npmi(posts: u64, count: u32, pa: u32, pb: u32) -> f32 {
    let (n, c, pa, pb) = (posts as f64, count as f64, pa as f64, pb as f64);
    if c >= n {
        return 1.0;
    }
    let pmi = (c * n / (pa * pb)).ln();
    (pmi / (n / c).ln()).clamp(0.0, 1.0) as f32
}

fn weight(kind: Weight, posts: u64, count: u32, pa: u32, pb: u32) -> f32 {
    match kind {
        Weight::Cosine => cosine(count, pa, pb),
        Weight::Npmi => npmi(posts, count, pa, pb),
    }
}

// a rare tag can never co-occur min_count times, so its pairs are judged against half its
// own post count instead of being dropped outright
fn threshold(min_count: u32, pa: u32, pb: u32) -> u32 {
    min_count.min(pa.min(pb).div_ceil(2)).max(1)
}

pub fn select(pairs: &Pairs, node_posts: &[u32], params: &EdgeParams) -> Edges {
    let t0 = Instant::now();
    let n = node_posts.len();
    let adj = Csr::symmetric(n, pairs.iter());
    info!(
        "adjacency built for {n} nodes, {} entries, {:.0?}",
        adj.len(),
        t0.elapsed()
    );

    let chosen: Vec<Vec<(u32, u32, f32, u32)>> = (0..n)
        .into_par_iter()
        .map(|i| {
            let pi = node_posts[i];
            let candidates = |core_only: bool| -> Vec<(f32, u32, u32)> {
                adj.row(i)
                    .filter_map(|(j, c)| {
                        let pj = node_posts[j as usize];
                        (c >= threshold(params.min_count, pi, pj)
                            && (!core_only || pj >= params.core_floor))
                            .then(|| (weight(params.weight, params.posts, c, pi, pj), j, c))
                    })
                    .collect()
            };
            let k = if pi >= params.core_floor {
                params.top_k
            } else {
                params.small_k
            };
            let pick = |mut row: Vec<(f32, u32, u32)>| -> Vec<(f32, u32, u32)> {
                if row.len() > k {
                    row.select_nth_unstable_by(k, |x, y| y.0.total_cmp(&x.0));
                    row.truncate(k);
                }
                row.sort_unstable_by(|x, y| y.0.total_cmp(&x.0));
                let strong = row.iter().take_while(|r| r.0 >= params.min_weight).count();
                row.truncate(strong.max(params.min_degree.min(row.len())));
                row
            };
            let mut row = pick(candidates(false));
            if pi >= params.core_floor {
                row.extend(pick(candidates(true)));
            }
            row.into_iter()
                .map(|(w, j, c)| {
                    let (a, b) = if (i as u32) < j {
                        (i as u32, j)
                    } else {
                        (j, i as u32)
                    };
                    (a, b, w, c)
                })
                .collect()
        })
        .collect();
    drop(adj);
    let mut flat: Vec<(u32, u32, f32, u32)> = chosen.into_iter().flatten().collect();
    flat.par_sort_unstable_by(|x, y| (x.0, x.1).cmp(&(y.0, y.1)));
    flat.dedup_by(|x, y| x.0 == y.0 && x.1 == y.1);

    let mut edges = Edges::default();
    let mut degree = vec![0u32; n];
    for (a, b, w, c) in flat {
        edges.push(a, b, w, c);
        degree[a as usize] += 1;
        degree[b as usize] += 1;
    }
    let isolated = degree.iter().filter(|&&d| d == 0).count();
    degree.sort_unstable();
    info!(
        "{} edges from top-{} {:?} (min count {}, min weight {}, min degree {}, core floor {}, small k {}), degree min {} median {} p90 {} max {}, {isolated} isolated nodes, {:.0?}",
        edges.len(),
        params.top_k,
        params.weight,
        params.min_count,
        params.min_weight,
        params.min_degree,
        params.core_floor,
        params.small_k,
        degree[0],
        degree[n / 2],
        degree[n * 9 / 10],
        degree[n - 1],
        t0.elapsed()
    );
    edges
}

pub fn tail_neighbors(
    pairs: &Pairs,
    node_posts: &[u32],
    tail_posts: &[u32],
    n_nodes: usize,
    k: usize,
    kind: Weight,
    posts: u64,
) -> Vec<u32> {
    let t0 = Instant::now();
    let n_tail = tail_posts.len();
    let adj = Csr::directed(n_tail, pairs.iter());
    debug_assert!((0..n_tail).all(|x| adj.row(x).all(|(j, _)| (j as usize) < n_nodes)));
    let out: Vec<Vec<u32>> = (0..n_tail)
        .into_par_iter()
        .map(|x| {
            let mut row: Vec<(f32, u32, u32)> = adj
                .row(x)
                .map(|(j, c)| {
                    (
                        weight(kind, posts, c, tail_posts[x], node_posts[j as usize]),
                        c,
                        j,
                    )
                })
                .collect();
            if row.len() > k {
                row.select_nth_unstable_by(k, |p, q| q.0.total_cmp(&p.0).then(q.1.cmp(&p.1)));
                row.truncate(k);
            }
            row.sort_unstable_by(|p, q| q.0.total_cmp(&p.0).then(q.1.cmp(&p.1)));
            let mut ids: Vec<u32> = row.into_iter().map(|(_, _, j)| j).collect();
            ids.resize(k, u32::MAX);
            ids
        })
        .collect();
    let flat: Vec<u32> = out.into_iter().flatten().collect();
    let without = flat.chunks(k).filter(|c| c[0] == u32::MAX).count();
    info!(
        "tail neighbors for {n_tail} tags, {without} have none, {:.0?}",
        t0.elapsed()
    );
    flat
}
