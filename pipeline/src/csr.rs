pub struct Csr<T> {
    off: Vec<usize>,
    col: Vec<u32>,
    val: Vec<T>,
}

impl<T: Copy + Default> Csr<T> {
    pub fn symmetric(n: usize, links: impl Iterator<Item = (u32, u32, T)> + Clone) -> Csr<T> {
        Csr::build(n, links, true)
    }

    pub fn directed(n: usize, links: impl Iterator<Item = (u32, u32, T)> + Clone) -> Csr<T> {
        Csr::build(n, links, false)
    }

    fn build(n: usize, links: impl Iterator<Item = (u32, u32, T)> + Clone, both: bool) -> Csr<T> {
        let mut off = vec![0usize; n + 1];
        for (a, b, _) in links.clone() {
            off[a as usize + 1] += 1;
            if both {
                off[b as usize + 1] += 1;
            }
        }
        let mut total = 0;
        for o in &mut off {
            total += *o;
            *o = total;
        }
        let mut col = vec![0u32; total];
        let mut val = vec![T::default(); total];
        let mut fill = off[..n].to_vec();
        let mut place = |row: u32, neighbor: u32, v: T| {
            let slot = &mut fill[row as usize];
            col[*slot] = neighbor;
            val[*slot] = v;
            *slot += 1;
        };
        for (a, b, v) in links {
            place(a, b, v);
            if both {
                place(b, a, v);
            }
        }
        Csr { off, col, val }
    }

    pub fn len(&self) -> usize {
        self.col.len()
    }

    pub fn degree(&self, i: usize) -> usize {
        self.off[i + 1] - self.off[i]
    }

    pub fn row(&self, i: usize) -> impl Iterator<Item = (u32, T)> + '_ {
        let range = self.off[i]..self.off[i + 1];
        self.col[range.clone()]
            .iter()
            .copied()
            .zip(self.val[range].iter().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::Csr;

    #[test]
    fn rows_hold_the_right_neighbors() {
        let links = [(0u32, 1u32, 1.0f32), (1, 2, 2.0), (0, 3, 3.0)];
        let both = Csr::symmetric(4, links.iter().copied());
        assert_eq!(both.row(0).collect::<Vec<_>>(), vec![(1, 1.0), (3, 3.0)]);
        assert_eq!(both.row(1).collect::<Vec<_>>(), vec![(0, 1.0), (2, 2.0)]);
        assert_eq!(both.row(2).collect::<Vec<_>>(), vec![(1, 2.0)]);
        assert_eq!(both.row(3).collect::<Vec<_>>(), vec![(0, 3.0)]);
        assert_eq!(both.len(), 6);
        let one_way = Csr::directed(4, links.iter().copied());
        assert_eq!(one_way.row(0).collect::<Vec<_>>(), vec![(1, 1.0), (3, 3.0)]);
        assert_eq!(one_way.degree(1), 1);
        assert_eq!(one_way.degree(3), 0);
    }
}
