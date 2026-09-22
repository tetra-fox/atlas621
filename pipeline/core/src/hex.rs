use std::time::Instant;
use ts_rs::TS;

use anyhow::Result;
use geo::{Area, Coord, LineString, Polygon, unary_union};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, trace};

pub struct CutParams {
    pub hex_cols: usize,
    pub rings: usize,
    pub min_nodes: usize,
}

#[derive(Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/lib/data/generated.ts")]
pub struct Feature {
    pub community: u32,
    pub label: bool,
    pub anchor: [f32; 2],
    pub cells: usize,
    pub hexes: Vec<i32>,
}

#[derive(Serialize, Deserialize)]
pub struct Level {
    pub hex_size: f64,
    pub features: Vec<Feature>,
}

pub struct Cut {
    pub level: Level,
    pub membership: Vec<u32>,
    pub sizes: Vec<usize>,
}

const SQRT3: f64 = 1.732_050_807_568_877_2;
const DIRS: [(i32, i32); 6] = [(1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];

struct Grid {
    size: f64,
}

impl Grid {
    fn center(&self, q: i32, r: i32) -> (f64, f64) {
        (
            self.size * 1.5 * q as f64,
            self.size * SQRT3 * (r as f64 + q as f64 / 2.0),
        )
    }

    fn cell(&self, x: f64, y: f64) -> (i32, i32) {
        let qf = (2.0 / 3.0 * x) / self.size;
        let rf = (SQRT3 / 3.0 * y - x / 3.0) / self.size;
        // round in cube coordinates, where the three axes must sum to zero, then discard
        // whichever one moved furthest and rebuild it from the other two
        let (xf, zf) = (qf, rf);
        let yf = -xf - zf;
        let (mut rx, mut rz) = (xf.round(), zf.round());
        let ry = yf.round();
        let (dx, dy, dz) = ((rx - xf).abs(), (ry - yf).abs(), (rz - zf).abs());
        if dx > dy && dx > dz {
            rx = -ry - rz;
        } else if dy <= dz {
            rz = -rx - ry;
        }
        (rx as i32, rz as i32)
    }

    fn hexagon(&self, q: i32, r: i32) -> Polygon<f64> {
        let (cx, cy) = self.center(q, r);
        let ring: Vec<Coord<f64>> = (0..=6)
            .map(|k| {
                let angle = (60.0 * (k % 6) as f64).to_radians();
                Coord {
                    x: cx + self.size * angle.cos(),
                    y: cy + self.size * angle.sin(),
                }
            })
            .collect();
        Polygon::new(LineString::from(ring), vec![])
    }
}

fn disc(q: i32, r: i32, rings: usize) -> Vec<(i32, i32)> {
    let n = rings as i32;
    let mut out = Vec::with_capacity(1 + 3 * rings * (rings + 1));
    for dq in -n..=n {
        for dr in (-n).max(-dq - n)..=n.min(-dq + n) {
            out.push((q + dq, r + dr));
        }
    }
    out
}

fn hex_distance(a: (i32, i32), b: (i32, i32)) -> i32 {
    let (dq, dr) = (a.0 - b.0, a.1 - b.1);
    dq.abs().max(dr.abs()).max((dq + dr).abs())
}

fn basins(occupied: &[(i32, i32)], counts: &[u32], rings: usize, min_nodes: usize) -> Vec<u32> {
    let mut hexes: Vec<(i32, i32)> = occupied
        .iter()
        .flat_map(|&(q, r)| disc(q, r, rings))
        .collect();
    hexes.sort_unstable();
    hexes.dedup();
    let index: FxHashMap<(i32, i32), usize> =
        hexes.iter().enumerate().map(|(i, &h)| (h, i)).collect();
    let mut count = vec![0u32; hexes.len()];
    for (h, &c) in occupied.iter().zip(counts) {
        count[index[h]] = c;
    }
    let density: Vec<f64> = hexes
        .iter()
        .map(|&(q, r)| {
            let around = disc(q, r, rings);
            let total: u32 = around
                .iter()
                .filter_map(|h| index.get(h))
                .map(|&i| count[i])
                .sum();
            total as f64 / around.len() as f64
        })
        .collect();
    let mut order: Vec<usize> = (0..hexes.len()).collect();
    order.sort_unstable_by(|&a, &b| {
        density[b]
            .total_cmp(&density[a])
            .then(hexes[a].cmp(&hexes[b]))
    });
    let neighbors = |i: usize| {
        let (q, r) = hexes[i];
        let index = &index;
        DIRS.iter()
            .filter_map(move |&(dq, dr)| index.get(&(q + dq, r + dr)).copied())
    };
    let mut basin = vec![u32::MAX; hexes.len()];
    let mut next = 0u32;
    for &i in &order {
        let uphill = neighbors(i)
            .filter(|&j| basin[j] != u32::MAX)
            .max_by(|&a, &b| {
                density[a]
                    .total_cmp(&density[b])
                    .then(basin[b].cmp(&basin[a]))
            });
        basin[i] = match uphill {
            Some(j) => basin[j],
            None => {
                next += 1;
                next - 1
            }
        };
    }
    loop {
        let mut nodes = vec![0usize; next as usize];
        let mut cells = vec![0usize; next as usize];
        for (i, &b) in basin.iter().enumerate() {
            nodes[b as usize] += count[i] as usize;
            cells[b as usize] += 1;
        }
        let mut small: Vec<u32> = (0..next)
            .filter(|&b| cells[b as usize] > 0 && nodes[b as usize] < min_nodes.max(1))
            .collect();
        small.sort_unstable_by_key(|&b| (nodes[b as usize], b));
        let mut merged = 0;
        for b in small {
            let mine: Vec<usize> = (0..hexes.len()).filter(|&i| basin[i] == b).collect();
            let mut border: FxHashMap<u32, usize> = FxHashMap::default();
            for &i in &mine {
                for j in neighbors(i) {
                    if basin[j] != b {
                        *border.entry(basin[j]).or_insert(0) += 1;
                    }
                }
            }
            let target = border
                .iter()
                .max_by(|x, y| x.1.cmp(y.1).then(y.0.cmp(x.0)))
                .map(|(&t, _)| t)
                .or_else(|| {
                    (0..hexes.len())
                        .filter(|&j| basin[j] != b)
                        .map(|j| {
                            let gap = mine
                                .iter()
                                .map(|&i| hex_distance(hexes[i], hexes[j]))
                                .min()
                                .unwrap();
                            (gap, basin[j])
                        })
                        .min()
                        .map(|(_, t)| t)
                });
            if let Some(target) = target {
                for &i in &mine {
                    basin[i] = target;
                }
                merged += 1;
            }
        }
        trace!(merged, "basin merge pass");
        if merged == 0 {
            break;
        }
    }
    let mut nodes = vec![0usize; next as usize];
    for (i, &b) in basin.iter().enumerate() {
        nodes[b as usize] += count[i] as usize;
    }
    let mut ids: Vec<u32> = (0..next).filter(|&b| nodes[b as usize] > 0).collect();
    ids.sort_unstable_by(|&a, &b| nodes[b as usize].cmp(&nodes[a as usize]).then(a.cmp(&b)));
    let mut rank = vec![u32::MAX; next as usize];
    for (k, &b) in ids.iter().enumerate() {
        rank[b as usize] = k as u32;
    }
    occupied
        .iter()
        .map(|h| rank[basin[index[h]] as usize])
        .collect()
}

pub fn cut(pos: &[[f32; 2]], params: &CutParams) -> Result<Cut> {
    let t0 = Instant::now();
    debug!(
        hex_cols = params.hex_cols,
        rings = params.rings,
        min_nodes = params.min_nodes,
        points = pos.len(),
        "cutting territories"
    );
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for p in pos {
        lo = lo.min(p[0] as f64);
        hi = hi.max(p[0] as f64);
    }
    let grid = Grid {
        size: (hi - lo) / (SQRT3 * params.hex_cols as f64),
    };
    let cells: Vec<(i32, i32)> = pos
        .iter()
        .map(|p| grid.cell(p[0] as f64, p[1] as f64))
        .collect();
    let mut hexes: Vec<(i32, i32)> = cells.clone();
    hexes.sort_unstable();
    hexes.dedup();
    let index: FxHashMap<(i32, i32), usize> =
        hexes.iter().enumerate().map(|(i, &h)| (h, i)).collect();
    let mut counts = vec![0u32; hexes.len()];
    for c in &cells {
        counts[index[c]] += 1;
    }
    let basin = basins(&hexes, &counts, params.rings, params.min_nodes);
    let k = basin.iter().copied().max().map_or(0, |b| b as usize + 1);
    let membership: Vec<u32> = cells.iter().map(|c| basin[index[c]]).collect();
    let mut sizes = vec![0usize; k];
    for &m in &membership {
        sizes[m as usize] += 1;
    }
    let mut by_basin: Vec<Vec<(i32, i32)>> = vec![Vec::new(); k];
    for (i, &b) in basin.iter().enumerate() {
        by_basin[b as usize].push(hexes[i]);
    }
    let mut features = Vec::with_capacity(k);
    for (b, cells) in by_basin.iter().enumerate() {
        let polys: Vec<Polygon<f64>> = cells.iter().map(|&(q, r)| grid.hexagon(q, r)).collect();
        let merged = unary_union(&polys);
        let anchor = merged
            .0
            .iter()
            .max_by(|a, b| a.unsigned_area().total_cmp(&b.unsigned_area()))
            .and_then(|poly| polylabel::polylabel(poly, &(grid.size * 0.25)).ok())
            .map_or_else(
                || {
                    let (x, y) = grid.center(cells[0].0, cells[0].1);
                    [x as f32, y as f32]
                },
                |p| [p.x() as f32, p.y() as f32],
            );
        features.push(Feature {
            community: b as u32,
            label: true,
            anchor,
            cells: cells.len(),
            hexes: cells.iter().flat_map(|&(q, r)| [q, r]).collect(),
        });
    }
    info!(
        "{} hexes of size {:.2} cut into {k} territories (smoothed over {} rings, basins under {} nodes merged), largest {} nodes, median {}, {:.0?}",
        hexes.len(),
        grid.size,
        params.rings,
        params.min_nodes,
        sizes[0],
        sizes[k / 2],
        t0.elapsed()
    );
    Ok(Cut {
        level: Level {
            hex_size: grid.size,
            features,
        },
        membership,
        sizes,
    })
}

pub fn continent_of_region(regions: &Cut, continents: &Cut) -> Vec<u32> {
    let k = regions.sizes.len();
    let mut votes: Vec<FxHashMap<u32, usize>> = vec![FxHashMap::default(); k];
    for (&r, &c) in regions.membership.iter().zip(&continents.membership) {
        *votes[r as usize].entry(c).or_insert(0) += 1;
    }
    votes
        .iter()
        .map(|v| {
            v.iter()
                .max_by(|x, y| x.1.cmp(y.1).then(y.0.cmp(x.0)))
                .map_or(0, |(&c, _)| c)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_lookup_matches_nearest_center() {
        let grid = Grid { size: 3.0 };
        for i in 0..400 {
            let (x, y) = ((i % 20) as f64 * 1.7 - 17.0, (i / 20) as f64 * 1.3 - 13.0);
            let (q, r) = grid.cell(x, y);
            let (cx, cy) = grid.center(q, r);
            let d = ((cx - x).powi(2) + (cy - y).powi(2)).sqrt();
            for (dq, dr) in DIRS {
                let (nx, ny) = grid.center(q + dq, r + dr);
                let nd = ((nx - x).powi(2) + (ny - y).powi(2)).sqrt();
                assert!(
                    d <= nd + 1e-9,
                    "({x}, {y}) mapped to ({q}, {r}) but a neighbor is closer"
                );
            }
        }
    }

    #[test]
    fn hexes_merge_with_the_hole_kept() {
        let grid = Grid { size: 1.0 };
        let cells: Vec<(i32, i32)> = (-3i32..=3)
            .flat_map(|q| (-3i32..=3).map(move |r| (q, r)))
            .filter(|&(q, r)| (q, r) != (0, 0) && (q + r).abs() <= 3)
            .collect();
        let hexes: Vec<Polygon<f64>> = cells.iter().map(|&(q, r)| grid.hexagon(q, r)).collect();
        let merged = unary_union(&hexes);
        assert_eq!(merged.0.len(), 1, "polygons: {}", merged.0.len());
        let poly = &merged.0[0];
        assert_eq!(
            poly.interiors().len(),
            1,
            "holes: {}",
            poly.interiors().len()
        );
        let hex_area = 1.5 * SQRT3 * grid.size * grid.size;
        assert_eq!(
            (poly.unsigned_area() / hex_area).round() as usize,
            hexes.len()
        );
    }

    #[test]
    fn disc_has_the_hex_count_of_its_rings() {
        assert_eq!(disc(0, 0, 0), vec![(0, 0)]);
        assert_eq!(disc(2, -1, 1).len(), 7);
        assert_eq!(disc(0, 0, 2).len(), 19);
        for h in disc(2, -1, 1) {
            assert!(DIRS.contains(&(h.0 - 2, h.1 + 1)) || h == (2, -1));
        }
    }

    #[test]
    fn two_peaks_split_at_the_valley_and_a_small_one_merges() {
        let hexes: Vec<(i32, i32)> = (0..12).map(|q| (q, 0)).collect();
        let counts = [1u32, 5, 30, 5, 1, 1, 5, 30, 5, 1, 1, 9];
        let b = basins(&hexes, &counts, 0, 0);
        assert!(b[..5].iter().all(|&x| x == b[0]));
        assert!(b[5..10].iter().all(|&x| x == b[5]));
        assert!(b[10..].iter().all(|&x| x == b[10]));
        assert!(b[0] != b[5] && b[5] != b[10] && b[0] != b[10]);
        let b = basins(&hexes, &counts, 0, 20);
        assert!(b[5..].iter().all(|&x| x == b[5]));
        assert!(b[0] != b[5]);
    }

    #[test]
    fn an_island_joins_the_coast_it_can_reach_or_else_the_nearest_one() {
        let hexes: Vec<(i32, i32)> = (0..5)
            .map(|q| (q, 0))
            .chain((30..36).map(|q| (q, 0)))
            .chain([(7, 0), (20, 0)])
            .collect();
        let counts = [10u32, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 1, 1];
        let b = basins(&hexes, &counts, 1, 5);
        assert!(b[..5].iter().all(|&x| x == b[0]));
        assert!(b[5..11].iter().all(|&x| x == b[5]));
        assert!(b[0] != b[5]);
        assert_eq!(
            b[11], b[0],
            "two hexes off the first coast, one smoothing ring each side"
        );
        assert_eq!(
            b[12], b[5],
            "out of reach of both, eight hexes from the second coast, eleven from the first"
        );
    }
}
