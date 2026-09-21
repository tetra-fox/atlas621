use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::tags::Tags;

#[derive(Serialize, Deserialize)]
pub struct Entry {
    pub n: String,
    pub c: u32,
    pub k: u8,
    pub i: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub t: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<String>,
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
            n: name.clone(),
            c: tags.post_counts[idx],
            k: tags.categories[idx],
            i: if idx < tags.n_nodes { idx as i32 } else { -1 },
            t: neighbors(idx),
            a: None,
        });
    }
    for (from, target) in aliases {
        let t = *target as usize;
        shards.entry(shard_key(from)).or_default().push(Entry {
            n: from.clone(),
            c: tags.post_counts[t],
            k: tags.categories[t],
            i: if t < tags.n_nodes { t as i32 } else { -1 },
            t: neighbors(t),
            a: Some(tags.names[t].clone()),
        });
    }
    for list in shards.values_mut() {
        list.sort_by(|a, b| a.n.cmp(&b.n).then(b.c.cmp(&a.c)));
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
