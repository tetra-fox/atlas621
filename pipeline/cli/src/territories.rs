use serde::{Deserialize, Serialize};

use atlas_core::edges::Edges;
use atlas_core::hex::Cut;

use crate::names::{self, Fit, NameParams};

#[derive(Serialize, Deserialize)]
pub struct RegionInfo {
    pub size: usize,
    pub name_tags: Vec<u32>,
}

#[derive(Serialize, Deserialize)]
pub struct Meta {
    pub regions: Vec<RegionInfo>,
    pub continents: Vec<RegionInfo>,
    pub continent_of_region: Vec<u32>,
}

pub fn name(
    cut: &Cut,
    ties: &Edges,
    post_counts: &[u32],
    categories: &[u8],
    params: &NameParams,
) -> Vec<RegionInfo> {
    let k = cut.sizes.len();
    let fit = Fit::compute(ties, &cut.membership, k, post_counts, categories, params);
    let names = names::names(&fit, &cut.membership, post_counts, categories, params);
    cut.sizes
        .iter()
        .zip(names)
        .map(|(&size, name_tags)| RegionInfo { size, name_tags })
        .collect()
}
