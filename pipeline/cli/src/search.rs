use std::collections::BTreeMap;

use atlas_core::ipc::{self, Column};

use crate::tags::Tags;

pub struct Entry {
    pub name: String,
    pub post_count: u32,
    pub category: u8,
    // the node this row points at, or -1 for a tag that never became one
    pub node: i32,
    pub tail: Vec<u32>,
    // set only on an alias row, naming the tag it redirects to
    pub alias: Option<String>,
}

pub fn write_shard<W: std::io::Write>(w: W, rows: &[Entry]) -> anyhow::Result<()> {
    let column = |f: fn(&Entry) -> u32| rows.iter().map(f).collect::<Vec<u32>>();
    ipc::write(
        w,
        &[
            (
                "name",
                ipc::strings(&rows.iter().map(|e| e.name.clone()).collect::<Vec<_>>()),
            ),
            ("post_count", u32::column(&column(|e| e.post_count))),
            (
                "category",
                u8::column(&rows.iter().map(|e| e.category).collect::<Vec<_>>()),
            ),
            (
                "node",
                i32::column(&rows.iter().map(|e| e.node).collect::<Vec<_>>()),
            ),
            (
                "tail",
                ipc::lists(&rows.iter().map(|e| e.tail.clone()).collect::<Vec<_>>())?,
            ),
            (
                "alias",
                ipc::maybe_strings(&rows.iter().map(|e| e.alias.clone()).collect::<Vec<_>>()),
            ),
        ],
    )
}

pub fn shard_key(name: &str) -> String {
    let mut key = String::with_capacity(2);
    for ch in name.chars().chain(std::iter::repeat('_')).take(2) {
        key.push(if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            ch
        } else {
            '_'
        });
    }
    key
}

pub fn build(
    tags: &Tags,
    aliases: &[(String, u32)],
    tail_neighbors: &[u32],
    tail_k: usize,
) -> BTreeMap<String, Vec<Entry>> {
    let mut shards: BTreeMap<String, Vec<Entry>> = BTreeMap::new();
    let neighbors = |idx: usize| -> Vec<u32> {
        if idx < tags.n_nodes || idx >= tags.n_nodes + tags.n_tail {
            return Vec::new();
        }
        let x = idx - tags.n_nodes;
        tail_neighbors[x * tail_k..(x + 1) * tail_k]
            .iter()
            .copied()
            .filter(|&j| j != u32::MAX)
            .collect()
    };
    for (idx, name) in tags.names.iter().enumerate() {
        shards.entry(shard_key(name)).or_default().push(Entry {
            name: name.clone(),
            post_count: tags.post_counts[idx],
            category: tags.categories[idx],
            node: if idx < tags.n_nodes { idx as i32 } else { -1 },
            tail: neighbors(idx),
            alias: None,
        });
    }
    for (from, target) in aliases {
        let t = *target as usize;
        shards.entry(shard_key(from)).or_default().push(Entry {
            name: from.clone(),
            post_count: tags.post_counts[t],
            category: tags.categories[t],
            node: if t < tags.n_nodes { t as i32 } else { -1 },
            tail: neighbors(t),
            alias: Some(tags.names[t].clone()),
        });
    }
    for list in shards.values_mut() {
        list.sort_by(|a, b| a.name.cmp(&b.name).then(b.post_count.cmp(&a.post_count)));
    }
    shards
}

#[cfg(test)]
mod tests {
    use super::shard_key;

    #[test]
    fn keys_are_two_safe_chars() {
        assert_eq!(shard_key("dragon"), "dr");
        assert_eq!(shard_key("a"), "a_");
        assert_eq!(shard_key(":3"), "_3");
        assert_eq!(shard_key("ñandu"), "_a");
        assert_eq!(shard_key(""), "__");
    }
}

#[cfg(test)]
mod shard_tests {
    use super::*;

    #[test]
    fn a_shard_round_trips_through_arrow() {
        let rows = vec![
            Entry {
                name: "dragon".into(),
                post_count: 1234,
                category: 0,
                node: 7,
                tail: vec![],
                alias: None,
            },
            Entry {
                name: "dragons".into(),
                post_count: 1234,
                category: 5,
                node: -1,
                tail: vec![7, 9],
                alias: Some("dragon".into()),
            },
        ];
        let mut buf = Vec::new();
        write_shard(&mut buf, &rows).unwrap();
        assert!(buf.starts_with(b"ARROW1"));

        let batch = atlas_core::ipc::first_batch(&buf).unwrap();
        assert_eq!(batch.num_rows(), 2);
        let names: Vec<String> = atlas_core::ipc::read_strings(&buf, "name").unwrap();
        assert_eq!(names, vec!["dragon", "dragons"]);
        let schema: Vec<String> = batch
            .schema()
            .fields()
            .iter()
            .map(|f| {
                format!(
                    "{}:{}{}",
                    f.name(),
                    f.data_type(),
                    if f.is_nullable() { "?" } else { "" }
                )
            })
            .collect();
        assert_eq!(
            schema.join(" "),
            "name:Utf8 post_count:UInt32 category:UInt8 node:Int32 tail:List(non-null UInt32) alias:Utf8?"
        );
    }
}
