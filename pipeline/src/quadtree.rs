use log::debug;

pub const NONE: u32 = u32::MAX;
const MAX_DEPTH: u32 = 48;

pub struct Cell {
    pub center: [f32; 2],
    pub half: f32,
    pub mass: f32,
    pub moment: [f32; 2],
    pub count: u32,
    pub body: u32,
    pub child: [u32; 4],
}

impl Cell {
    fn empty(center: [f32; 2], half: f32) -> Cell {
        Cell {
            center,
            half,
            mass: 0.0,
            moment: [0.0; 2],
            count: 0,
            body: NONE,
            child: [NONE; 4],
        }
    }

    pub fn is_leaf(&self) -> bool {
        self.child[0] == NONE
    }

    pub fn center_of_mass(&self) -> [f32; 2] {
        [self.moment[0] / self.mass, self.moment[1] / self.mass]
    }
}

pub struct QuadTree {
    pub cells: Vec<Cell>,
}

impl QuadTree {
    pub fn build(pos: &[[f32; 2]], mass: &[f32]) -> QuadTree {
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in pos {
            for d in 0..2 {
                lo[d] = lo[d].min(p[d]);
                hi[d] = hi[d].max(p[d]);
            }
        }
        let half = ((hi[0] - lo[0]).max(hi[1] - lo[1]) * 0.5 + 1e-3) * 1.001;
        let center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
        let mut tree = QuadTree {
            cells: Vec::with_capacity(pos.len() * 2),
        };
        tree.cells.push(Cell::empty(center, half));
        for (i, p) in pos.iter().enumerate() {
            tree.insert(i as u32, *p, mass[i]);
        }
        debug!(
            "quadtree: {} cells for {} bodies, half size {half:.1}",
            tree.cells.len(),
            pos.len()
        );
        tree
    }

    fn insert(&mut self, body: u32, p: [f32; 2], m: f32) {
        let mut idx = 0usize;
        let mut depth = 0;
        loop {
            let cell = &mut self.cells[idx];
            cell.mass += m;
            cell.moment[0] += p[0] * m;
            cell.moment[1] += p[1] * m;
            cell.count += 1;
            if cell.count == 1 {
                cell.body = body;
                return;
            }
            if depth >= MAX_DEPTH {
                return;
            }
            if cell.child[0] == NONE {
                let (prev, prev_m) = (cell.body, cell.mass - m);
                let prev_p = [
                    (cell.moment[0] - p[0] * m) / prev_m,
                    (cell.moment[1] - p[1] * m) / prev_m,
                ];
                let (center, half) = (cell.center, cell.half * 0.5);
                cell.body = NONE;
                let base = self.cells.len() as u32;
                for q in 0..4 {
                    let c = [
                        center[0] + if q & 1 == 1 { half } else { -half },
                        center[1] + if q & 2 == 2 { half } else { -half },
                    ];
                    self.cells.push(Cell::empty(c, half));
                }
                let cell = &mut self.cells[idx];
                cell.child = [base, base + 1, base + 2, base + 3];
                let ci = cell.child[quadrant(cell.center, prev_p)] as usize;
                let sub = &mut self.cells[ci];
                sub.mass = prev_m;
                sub.moment = [prev_p[0] * prev_m, prev_p[1] * prev_m];
                sub.count = 1;
                sub.body = prev;
            }
            let q = quadrant(self.cells[idx].center, p);
            idx = self.cells[idx].child[q] as usize;
            depth += 1;
        }
    }
}

fn quadrant(center: [f32; 2], p: [f32; 2]) -> usize {
    (p[0] >= center[0]) as usize | (((p[1] >= center[1]) as usize) << 1)
}
