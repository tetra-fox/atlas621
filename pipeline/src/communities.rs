use std::time::Instant;

use anyhow::{Context, Result};
use leiden_rs::{GraphDataBuilder, Leiden, LeidenConfig};
use log::info;
use rand::prelude::*;
use rustc_hash::FxHashMap;

use crate::csr::Csr;
use crate::edges::Edges;

pub struct CommunityParams {
    pub region_resolution: f64,
    pub seed: u64,
    pub min_region: usize,
    pub core_floor: u32,
    pub refine_sweeps: usize,
}

fn run_leiden(
    n: usize,
    edges: impl Iterator<Item = (usize, usize, f64)>,
    resolution: f64,
    seed: u64,
) -> Result<Vec<usize>> {
    let mut builder = GraphDataBuilder::new(n);
    for (a, b, w) in edges {
        builder.add_edge(a, b, w).context("leiden graph edge")?;
    }
    let data = builder.build().context("leiden graph build")?;
    let config: LeidenConfig = LeidenConfig::builder()
        .resolution(resolution)
        .seed(seed)
        .build();
    let out = Leiden::new(config).run(&data).context("leiden")?;
    Ok(out.partition.as_slice().to_vec())
}

fn absorb_small(membership: &mut [usize], edges: &Edges, min_size: usize) -> usize {
    let mut moved = 0;
    for _ in 0..4 {
        let k = membership.iter().copied().max().map_or(0, |m| m + 1);
        let mut sizes = vec![0usize; k];
        for &c in membership.iter() {
            sizes[c] += 1;
        }
        let mut outside: Vec<FxHashMap<usize, f64>> = vec![Default::default(); k];
        for e in edges.iter() {
            let (ca, cb) = (membership[e.a as usize], membership[e.b as usize]);
            if ca == cb {
                continue;
            }
            let w = e.weight as f64;
            if sizes[ca] < min_size {
                *outside[ca].entry(cb).or_insert(0.0) += w;
            }
            if sizes[cb] < min_size {
                *outside[cb].entry(ca).or_insert(0.0) += w;
            }
        }
        let target: Vec<Option<usize>> = (0..k)
            .map(|c| {
                if sizes[c] >= min_size {
                    return None;
                }
                outside[c]
                    .iter()
                    .max_by(|x, y| x.1.total_cmp(y.1).then(y.0.cmp(x.0)))
                    .map(|(&t, _)| t)
            })
            .collect();
        let mut changed = 0;
        for c in membership.iter_mut() {
            if let Some(t) = target[*c] {
                *c = t;
                changed += 1;
            }
        }
        moved += changed;
        if changed == 0 {
            break;
        }
    }
    moved
}

fn local_moving_sweeps(
    membership: &mut [usize],
    edges: &Edges,
    resolution: f64,
    sweeps: usize,
    seed: u64,
) -> usize {
    let n = membership.len();
    let k = membership.iter().copied().max().map_or(0, |m| m + 1);
    let adj = Csr::symmetric(n, edges.iter().map(|e| (e.a, e.b, e.weight as f64)));
    let degree: Vec<f64> = (0..n).map(|i| adj.row(i).map(|(_, w)| w).sum()).collect();
    let two_m: f64 = degree.iter().sum();
    let mut comm_tot = vec![0f64; k];
    for (&c, &d) in membership.iter().zip(&degree) {
        comm_tot[c] += d;
    }
    let mut rng = StdRng::seed_from_u64(seed);
    let mut order: Vec<usize> = (0..n).collect();
    let mut weight_to = vec![0f64; k];
    let mut touched: Vec<usize> = Vec::new();
    let mut moves = 0;
    for _ in 0..sweeps {
        order.shuffle(&mut rng);
        let mut moved_this_sweep = 0;
        for &v in &order {
            if adj.degree(v) == 0 {
                continue;
            }
            let current = membership[v];
            for (u, w) in adj.row(v) {
                let c = membership[u as usize];
                if weight_to[c] == 0.0 {
                    touched.push(c);
                }
                weight_to[c] += w;
            }
            let kv = degree[v];
            let gain = |c: usize, tot_without_v: f64| {
                weight_to[c] - resolution * kv * tot_without_v / two_m
            };
            let stay = gain(current, comm_tot[current] - kv);
            let mut best = current;
            let mut best_gain = stay;
            for &c in &touched {
                if c == current {
                    continue;
                }
                let g = gain(c, comm_tot[c]);
                if g > best_gain + 1e-12 {
                    best_gain = g;
                    best = c;
                }
            }
            if best != current {
                comm_tot[current] -= kv;
                comm_tot[best] += kv;
                membership[v] = best;
                moved_this_sweep += 1;
            }
            for &c in &touched {
                weight_to[c] = 0.0;
            }
            touched.clear();
        }
        moves += moved_this_sweep;
        if moved_this_sweep == 0 {
            break;
        }
    }
    moves
}

fn by_size(membership: &[usize]) -> (Vec<u32>, Vec<usize>) {
    let k = membership.iter().copied().max().map_or(0, |m| m + 1);
    let mut sizes = vec![0usize; k];
    for &c in membership {
        sizes[c] += 1;
    }
    let mut order: Vec<usize> = (0..k).filter(|&c| sizes[c] > 0).collect();
    order.sort_unstable_by(|&a, &b| sizes[b].cmp(&sizes[a]).then(a.cmp(&b)));
    let mut rank = vec![u32::MAX; k];
    for (r, &c) in order.iter().enumerate() {
        rank[c] = r as u32;
    }
    let renumbered: Vec<u32> = membership.iter().map(|&c| rank[c]).collect();
    let sizes: Vec<usize> = order.iter().map(|&c| sizes[c]).collect();
    (renumbered, sizes)
}

fn attach_tail(
    membership: &mut [Option<usize>],
    edges: &Edges,
    next_region: usize,
) -> (usize, usize) {
    let n = membership.len();
    let mut attached = 0;
    for _ in 0..6 {
        let mut best: Vec<(f32, usize)> = vec![(-1.0, usize::MAX); n];
        for e in edges.iter() {
            let (a, b, w) = (e.a as usize, e.b as usize, e.weight);
            if membership[a].is_none()
                && let Some(rb) = membership[b]
                && w > best[a].0
            {
                best[a] = (w, rb);
            }
            if membership[b].is_none()
                && let Some(ra) = membership[a]
                && w > best[b].0
            {
                best[b] = (w, ra);
            }
        }
        let mut changed = 0;
        for i in 0..n {
            if membership[i].is_none() && best[i].1 != usize::MAX {
                membership[i] = Some(best[i].1);
                changed += 1;
            }
        }
        attached += changed;
        if changed == 0 {
            break;
        }
    }
    let mut stranded = 0;
    for m in membership.iter_mut() {
        if m.is_none() {
            *m = Some(next_region);
            stranded += 1;
        }
    }
    (attached, stranded)
}

pub fn detect(edges: &Edges, n: usize, post_counts: &[u32], params: &CommunityParams) -> Result<Vec<u32>> {
    let t0 = Instant::now();
    let core: Vec<bool> = post_counts
        .iter()
        .map(|&p| p >= params.core_floor)
        .collect();
    let mut core_index = vec![usize::MAX; n];
    let mut n_core = 0;
    for i in 0..n {
        if core[i] {
            core_index[i] = n_core;
            n_core += 1;
        }
    }
    let mut core_edges = Edges::default();
    for e in edges.iter() {
        let (a, b) = (e.a as usize, e.b as usize);
        if core[a] && core[b] {
            core_edges.push(
                core_index[a] as u32,
                core_index[b] as u32,
                e.weight,
                e.count,
            );
        }
    }
    let mut core_raw = run_leiden(
        n_core,
        core_edges
            .iter()
            .map(|e| (e.a as usize, e.b as usize, e.weight as f64)),
        params.region_resolution,
        params.seed,
    )?;
    let raw_count = core_raw
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    let moved = absorb_small(&mut core_raw, &core_edges, params.min_region);
    info!(
        "leiden on {n_core} core nodes (>= {} posts, {} edges): {raw_count} raw regions, {moved} nodes folded out of regions smaller than {}, {:.0?}",
        params.core_floor,
        core_edges.len(),
        params.min_region,
        t0.elapsed()
    );
    let next_region = core_raw.iter().copied().max().map_or(0, |m| m + 1);
    let mut membership: Vec<Option<usize>> = (0..n)
        .map(|i| {
            if core[i] {
                Some(core_raw[core_index[i]])
            } else {
                None
            }
        })
        .collect();
    let (attached, stranded) = attach_tail(&mut membership, edges, next_region);
    info!(
        "{attached} tail nodes attached to a neighbor's region, {stranded} with no path to the core form their own"
    );
    let mut region_raw: Vec<usize> = membership.into_iter().map(|m| m.unwrap()).collect();
    if params.refine_sweeps > 0 {
        let t1 = Instant::now();
        let before = region_raw
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
        let moved = local_moving_sweeps(
            &mut region_raw,
            edges,
            params.region_resolution,
            params.refine_sweeps,
            params.seed,
        );
        let after = region_raw
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
        let folded = absorb_small(&mut region_raw, edges, params.min_region);
        info!(
            "{} local-moving sweeps over all {n} nodes: {before} -> {after} communities, {moved} moves, {folded} folded, {:.0?}",
            params.refine_sweeps,
            t1.elapsed()
        );
    }
    let (region, region_sizes) = by_size(&region_raw);
    let k = region_sizes.len();
    info!(
        "{k} regions (resolution {}), largest {}, median {}, singletons {}, {:.0?}",
        params.region_resolution,
        region_sizes[0],
        region_sizes[k / 2],
        region_sizes.iter().filter(|&&s| s == 1).count(),
        t0.elapsed()
    );
    Ok(region)
}
