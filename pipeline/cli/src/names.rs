use std::collections::HashSet;

use log::info;

use atlas_core::edges::Edges;

pub struct NameParams {
    pub min_fit: f64,
    pub candidate_posts: u32,
    pub member_posts: u32,
    pub take: usize,
}

// e621's own tag category ids
const GENERAL: u8 = 0;
const COPYRIGHT: u8 = 3;
const SPECIES: u8 = 5;

pub struct Fit {
    candidates: Vec<u32>,
    k: usize,
    fit: Vec<f64>,
    pub best: Vec<f64>,
}

impl Fit {
    pub fn compute(
        edges: &Edges,
        membership: &[u32],
        k: usize,
        post_counts: &[u32],
        categories: &[u8],
        params: &NameParams,
    ) -> Fit {
        let n = membership.len();
        let mut slot = vec![u32::MAX; n];
        let mut candidates = Vec::new();
        for i in 0..n {
            if post_counts[i] >= params.candidate_posts
                && matches!(categories[i], GENERAL | COPYRIGHT | SPECIES)
            {
                slot[i] = candidates.len() as u32;
                candidates.push(i as u32);
            }
        }
        let mut members = vec![0u32; k];
        for i in 0..n {
            if post_counts[i] >= params.member_posts {
                members[membership[i] as usize] += 1;
            }
        }
        let nc = candidates.len();
        let mut total = vec![0f64; nc];
        let mut into = vec![0f64; nc * k];
        let mut touch = vec![0u32; nc * k];
        let mut tally = |c: u32, other: usize, w: f64| {
            if c == u32::MAX {
                return;
            }
            let (c, r) = (c as usize, membership[other] as usize);
            total[c] += w;
            into[c * k + r] += w;
            if post_counts[other] >= params.member_posts {
                touch[c * k + r] += 1;
            }
        };
        for e in edges.iter() {
            let (a, b, w) = (e.a as usize, e.b as usize, e.weight as f64);
            tally(slot[a], b, w);
            tally(slot[b], a, w);
        }
        let mut fit = vec![0f64; nc * k];
        let mut best = vec![0f64; k];
        for c in 0..nc {
            for r in 0..k {
                let precision = into[c * k + r] / total[c].max(1e-9);
                let recall = touch[c * k + r] as f64 / members[r].max(1) as f64;
                let f1 = 2.0 * precision * recall / (precision + recall).max(1e-9);
                fit[c * k + r] = f1;
                best[r] = best[r].max(f1);
            }
        }
        Fit {
            candidates,
            k,
            fit,
            best,
        }
    }

    fn ranked(&self, r: usize, post_counts: &[u32]) -> Vec<u32> {
        let mut scored: Vec<(f64, u32)> = self
            .candidates
            .iter()
            .enumerate()
            .filter_map(|(c, &i)| {
                let f = self.fit[c * self.k + r];
                (f > 0.0).then(|| (f * (post_counts[i as usize].max(10) as f64).log10(), i))
            })
            .collect();
        scored.sort_unstable_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)));
        scored.into_iter().map(|(_, i)| i).collect()
    }
}

pub fn names(
    fit: &Fit,
    membership: &[u32],
    post_counts: &[u32],
    categories: &[u8],
    params: &NameParams,
) -> Vec<Vec<u32>> {
    let k = fit.k;
    let mut general: Vec<Vec<u32>> = vec![Vec::new(); k];
    let mut any: Vec<Vec<u32>> = vec![Vec::new(); k];
    for (i, &r) in membership.iter().enumerate() {
        if categories[i] == GENERAL {
            general[r as usize].push(i as u32);
        }
        any[r as usize].push(i as u32);
    }
    let mut order: Vec<usize> = (0..k).collect();
    order.sort_unstable_by(|&a, &b| fit.best[b].total_cmp(&fit.best[a]).then(a.cmp(&b)));
    let mut out = vec![Vec::new(); k];
    let mut used: HashSet<u32> = HashSet::new();
    let mut fallback = 0;
    for r in order {
        if general[r].is_empty() {
            general[r] = std::mem::take(&mut any[r]);
        }
        general[r].sort_unstable_by(|&a, &b| {
            post_counts[b as usize]
                .cmp(&post_counts[a as usize])
                .then(a.cmp(&b))
        });
        let mut ranked = if fit.best[r] >= params.min_fit {
            fit.ranked(r, post_counts)
        } else {
            fallback += 1;
            general[r].clone()
        };
        let fresh = |list: &[u32]| -> Vec<u32> {
            list.iter().copied().filter(|i| !used.contains(i)).collect()
        };
        let candidates = fresh(&ranked);
        let members = fresh(&general[r]);
        if !candidates.is_empty() {
            ranked = candidates;
        } else if !members.is_empty() {
            ranked = members;
        }
        ranked.truncate(params.take);
        if let Some(&first) = ranked.first() {
            used.insert(first);
        }
        out[r] = ranked;
    }
    if fallback > 0 {
        info!("{fallback} communities named by their most posted member, nothing fits them");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> NameParams {
        NameParams {
            min_fit: 0.1,
            candidate_posts: 100,
            member_posts: 1,
            take: 2,
        }
    }

    #[test]
    fn hubs_name_their_family_and_the_rest_fall_back() {
        let membership = [0u32, 0, 0, 0, 1, 1, 1];
        let post_counts = [50u32, 40, 30, 20, 500, 200, 150];
        let categories = [0u8; 7];
        let mut edges = Edges::default();
        for m in 0..4 {
            edges.push(m, 4, 0.9, 1);
        }
        edges.push(0, 1, 0.5, 1);
        let p = params();
        let fit = Fit::compute(&edges, &membership, 2, &post_counts, &categories, &p);
        assert!(fit.best[0] > p.min_fit);
        assert!(fit.best[1] < p.min_fit);
        let out = names(&fit, &membership, &post_counts, &categories, &p);
        assert_eq!(out[0][0], 4);
        assert_eq!(out[1], vec![5, 6]);
    }

    #[test]
    fn a_taken_name_falls_through() {
        let membership = [0u32, 0, 1, 1, 2];
        let post_counts = [50u32, 50, 50, 50, 1000];
        let categories = [0u8; 5];
        let mut edges = Edges::default();
        edges.push(0, 4, 1.0, 1);
        edges.push(1, 4, 1.0, 1);
        edges.push(2, 4, 0.5, 1);
        edges.push(0, 1, 1.0, 1);
        edges.push(2, 3, 1.0, 1);
        let p = params();
        let fit = Fit::compute(&edges, &membership, 3, &post_counts, &categories, &p);
        let out = names(&fit, &membership, &post_counts, &categories, &p);
        assert_eq!(out[0][0], 4);
        assert_ne!(out[1][0], 4);
    }
}
