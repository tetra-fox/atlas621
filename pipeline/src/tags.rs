use std::io::Read;

use anyhow::Result;
use log::{info, warn};
use rustc_hash::FxHashMap;

use crate::exports;

pub fn name_offsets(names: &[String]) -> Vec<u32> {
    std::iter::once(0)
        .chain(names.iter().scan(0u32, |total, name| {
            *total += name.len() as u32;
            Some(*total)
        }))
        .collect()
}

pub struct Tags {
    pub ids: Vec<u32>,
    pub names: Vec<String>,
    pub categories: Vec<u8>,
    pub post_counts: Vec<u32>,
    pub n_nodes: usize,
    pub n_tail: usize,
    pub node_floor: u32,
    index: FxHashMap<String, u32>,
}

impl Tags {
    pub fn load(reader: Box<dyn Read + Send>, node_floor: u32) -> Result<Tags> {
        let mut rdr = csv::ReaderBuilder::new().flexible(true).from_reader(reader);
        let headers = rdr.headers()?.clone();
        let col = |name| exports::column(&headers, "tags.csv", name);
        let (c_id, c_name, c_cat, c_count) = (
            col("id")?,
            col("name")?,
            col("category")?,
            col("post_count")?,
        );
        let mut rows: Vec<(u32, String, u8, u32)> = Vec::with_capacity(1 << 20);
        let mut bad_category = 0usize;
        for rec in rdr.records() {
            let rec = rec?;
            let category = match rec[c_cat].parse::<u8>() {
                Ok(c) => c,
                Err(_) => {
                    bad_category += 1;
                    0
                }
            };
            rows.push((
                rec[c_id].parse()?,
                rec[c_name].to_owned(),
                category,
                rec[c_count].parse()?,
            ));
        }
        if bad_category > 0 {
            warn!("{bad_category} tags had an unparsable category, treated as general");
        }
        rows.sort_unstable_by(|a, b| b.3.cmp(&a.3).then_with(|| a.1.cmp(&b.1)));
        let n_nodes = rows.iter().take_while(|r| r.3 >= node_floor).count();
        let n_tail = rows[n_nodes..].iter().take_while(|r| r.3 >= 1).count();
        info!(
            "{} tags: {n_nodes} nodes (>= {node_floor} posts), {n_tail} tail, {} without posts",
            rows.len(),
            rows.len() - n_nodes - n_tail
        );
        let mut ids = Vec::with_capacity(rows.len());
        let mut names = Vec::with_capacity(rows.len());
        let mut categories = Vec::with_capacity(rows.len());
        let mut post_counts = Vec::with_capacity(rows.len());
        for (id, name, category, count) in rows {
            ids.push(id);
            names.push(name);
            categories.push(category);
            post_counts.push(count);
        }
        Ok(Tags::from_parts(
            ids,
            names,
            categories,
            post_counts,
            n_nodes,
            n_tail,
            node_floor,
        ))
    }

    pub fn from_parts(
        ids: Vec<u32>,
        names: Vec<String>,
        categories: Vec<u8>,
        post_counts: Vec<u32>,
        n_nodes: usize,
        n_tail: usize,
        node_floor: u32,
    ) -> Tags {
        let index = names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), i as u32))
            .collect();
        Tags {
            ids,
            names,
            categories,
            post_counts,
            n_nodes,
            n_tail,
            node_floor,
            index,
        }
    }

    pub fn index_of(&self, name: &str) -> Option<u32> {
        self.index.get(name).copied()
    }
}
