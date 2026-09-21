use std::io::{self, Read};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use crossbeam_channel::{Receiver, bounded};
use log::{info, warn};
use rustc_hash::FxHashMap;

use crate::exports;
use crate::store::PairSink;
use crate::tags::Tags;

pub const YEAR0: u16 = 2007;
const MAX_YEARS: usize = 32;
const BATCH_POSTS: usize = 8192;
const INFLATE_CHUNK: usize = 4 << 20;
const DENSE: usize = 8192;

pub struct CountParams {
    pub shards: usize,
    pub pairs_hint: usize,
    pub tail_pairs_hint: usize,
}

pub struct PostStats {
    pub posts_total: u64,
    pub posts_deleted: u64,
    pub posts_kept: u64,
    pub years: usize,
    pub year_totals: Vec<u64>,
    pub node_year: Vec<u32>,
    pub node_rating: Vec<u32>,
    pub node_first_year: Vec<u16>,
    pub node_posts: Vec<u32>,
    pub tail_posts: Vec<u32>,
    pub distinct_node_pairs: u64,
    pub distinct_tail_pairs: u64,
    pub pair_increments: u64,
}

pub struct Pairs {
    pub keys: Vec<u64>,
    pub counts: Vec<u32>,
}

impl Pairs {
    pub fn key(hi: u32, lo: u32) -> u64 {
        ((hi as u64) << 32) | lo as u64
    }
    fn hi(key: u64) -> u32 {
        (key >> 32) as u32
    }
    fn lo(key: u64) -> u32 {
        key as u32
    }
    pub fn iter(&self) -> impl Iterator<Item = (u32, u32, u32)> + Clone + '_ {
        self.keys
            .iter()
            .zip(&self.counts)
            .map(|(&k, &c)| (Pairs::hi(k), Pairs::lo(k), c))
    }
}

#[derive(Default)]
struct Batch {
    node_off: Vec<u32>,
    node_idx: Vec<u32>,
    tail_off: Vec<u32>,
    tail_idx: Vec<u32>,
}

impl Batch {
    fn new() -> Self {
        Batch {
            node_off: vec![0],
            node_idx: Vec::new(),
            tail_off: vec![0],
            tail_idx: Vec::new(),
        }
    }
    fn len(&self) -> usize {
        self.node_off.len() - 1
    }
}

struct ChunkReader {
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for ChunkReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.pos >= self.buf.len() {
            match self.rx.recv() {
                Ok(chunk) => {
                    self.buf = chunk;
                    self.pos = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

struct RawBatch {
    text: Vec<u8>,
    off: Vec<u32>,
    year: Vec<u16>,
    rating: Vec<u8>,
}

impl RawBatch {
    fn new() -> Self {
        RawBatch {
            text: Vec::with_capacity(BATCH_POSTS * 600),
            off: vec![0],
            year: Vec::with_capacity(BATCH_POSTS),
            rating: Vec::with_capacity(BATCH_POSTS),
        }
    }
    fn len(&self) -> usize {
        self.year.len()
    }
}

struct Partial {
    kept: u64,
    year_totals: Vec<u64>,
    node_year: Vec<u32>,
    node_rating: Vec<u32>,
    node_first_year: Vec<u16>,
    node_posts: Vec<u32>,
    tail_posts: Vec<u32>,
    bad_utf8: u64,
}

impl Partial {
    fn new(n_nodes: usize, n_tail: usize) -> Self {
        Partial {
            kept: 0,
            year_totals: vec![0; MAX_YEARS],
            node_year: vec![0; n_nodes * MAX_YEARS],
            node_rating: vec![0; n_nodes * 3],
            node_first_year: vec![u16::MAX; n_nodes],
            node_posts: vec![0; n_nodes],
            tail_posts: vec![0; n_tail],
            bad_utf8: 0,
        }
    }

    fn record(&mut self, nodes: &[u32], tails: &[u32], year: u16, rating: u8) {
        let y = (year - YEAR0) as usize;
        self.kept += 1;
        self.year_totals[y] += 1;
        for &i in nodes {
            let i = i as usize;
            self.node_posts[i] += 1;
            self.node_year[i * MAX_YEARS + y] += 1;
            self.node_rating[i * 3 + rating as usize] += 1;
            if year < self.node_first_year[i] {
                self.node_first_year[i] = year;
            }
        }
        for &x in tails {
            self.tail_posts[x as usize] += 1;
        }
    }

    fn merge(&mut self, other: Partial) {
        self.kept += other.kept;
        self.bad_utf8 += other.bad_utf8;
        for (a, b) in self.year_totals.iter_mut().zip(other.year_totals) {
            *a += b;
        }
        for (a, b) in self.node_year.iter_mut().zip(other.node_year) {
            *a += b;
        }
        for (a, b) in self.node_rating.iter_mut().zip(other.node_rating) {
            *a += b;
        }
        for (a, b) in self.node_first_year.iter_mut().zip(other.node_first_year) {
            *a = (*a).min(b);
        }
        for (a, b) in self.node_posts.iter_mut().zip(other.node_posts) {
            *a += b;
        }
        for (a, b) in self.tail_posts.iter_mut().zip(other.tail_posts) {
            *a += b;
        }
    }
}

pub fn count(
    mut reader: Box<dyn Read + Send>,
    tags: &Tags,
    params: &CountParams,
    node_sink: &mut PairSink,
    tail_sink: &mut PairSink,
) -> Result<PostStats> {
    let n_nodes = tags.n_nodes;
    let n_tail = tags.n_tail;
    let shards = params.shards;
    let lookups = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(2, 8);
    let t0 = Instant::now();

    let (chunk_tx, chunk_rx) = bounded::<Vec<u8>>(8);
    let (raw_tx, raw_rx) = bounded::<RawBatch>(16);
    let mut shard_txs = Vec::with_capacity(shards);
    let mut shard_rxs = Vec::with_capacity(shards);
    for _ in 0..shards {
        let (tx, rx) = bounded::<Arc<Batch>>(8);
        shard_txs.push(tx);
        shard_rxs.push(rx);
    }

    let (totals, mut stats_part, pairs, maps) = std::thread::scope(|s| -> Result<_> {
        let inflate = s.spawn(move || -> io::Result<()> {
            loop {
                let mut chunk = vec![0u8; INFLATE_CHUNK];
                let mut filled = 0;
                while filled < chunk.len() {
                    let n = reader.read(&mut chunk[filled..])?;
                    if n == 0 {
                        break;
                    }
                    filled += n;
                }
                if filled == 0 {
                    return Ok(());
                }
                chunk.truncate(filled);
                if chunk_tx.send(chunk).is_err() {
                    return Ok(());
                }
            }
        });

        let parse = s.spawn(move || -> Result<(u64, u64)> {
            let mut rdr = csv::ReaderBuilder::new().flexible(true).from_reader(ChunkReader { rx: chunk_rx, buf: Vec::new(), pos: 0 });
            let headers = rdr.headers()?.clone();
            let col = |name| exports::column(&headers, "posts.csv", name);
            let (c_tags, c_deleted, c_created, c_rating) = (col("tag_string")?, col("is_deleted")?, col("created_at")?, col("rating")?);
            let (mut total, mut deleted) = (0u64, 0u64);
            let mut batch = RawBatch::new();
            let mut record = csv::ByteRecord::new();
            while rdr.read_byte_record(&mut record)? {
                total += 1;
                if &record[c_deleted] == b"t" {
                    deleted += 1;
                    continue;
                }
                let created = &record[c_created];
                if created.len() < 4 {
                    bail!("post {total} has an unparsable created_at");
                }
                let year: u16 = std::str::from_utf8(&created[..4])?.parse()?;
                if year < YEAR0 || (year - YEAR0) as usize >= MAX_YEARS {
                    bail!("post {total} created in {year}, outside the {MAX_YEARS} year slots from {YEAR0}");
                }
                let rating = match record[c_rating].first() {
                    Some(b's') => 0,
                    Some(b'q') => 1,
                    Some(b'e') => 2,
                    _ => bail!("post {total} has an unknown rating"),
                };
                batch.text.extend_from_slice(&record[c_tags]);
                batch.off.push(batch.text.len() as u32);
                batch.year.push(year);
                batch.rating.push(rating);
                if batch.len() == BATCH_POSTS {
                    raw_tx.send(std::mem::replace(&mut batch, RawBatch::new())).map_err(|_| anyhow!("lookup pool went away"))?;
                }
                if total.is_multiple_of(1_000_000) {
                    info!("{}M posts read, {:.0?}", total / 1_000_000, t0.elapsed());
                }
            }
            if batch.len() > 0 {
                raw_tx.send(batch).map_err(|_| anyhow!("lookup pool went away"))?;
            }
            Ok((total, deleted))
        });

        let mut lookup_handles = Vec::with_capacity(lookups);
        for _ in 0..lookups {
            let raw_rx = raw_rx.clone();
            let shard_txs = shard_txs.clone();
            lookup_handles.push(s.spawn(move || -> Partial {
                let mut part = Partial::new(n_nodes, n_tail);
                let mut nodes: Vec<u32> = Vec::with_capacity(128);
                let mut tails: Vec<u32> = Vec::with_capacity(16);
                while let Ok(raw) = raw_rx.recv() {
                    let mut batch = Batch::new();
                    for p in 0..raw.len() {
                        nodes.clear();
                        tails.clear();
                        for tag in raw.text[raw.off[p] as usize..raw.off[p + 1] as usize]
                            .split(|&b| b == b' ')
                        {
                            if tag.is_empty() {
                                continue;
                            }
                            let Ok(name) = std::str::from_utf8(tag) else {
                                part.bad_utf8 += 1;
                                continue;
                            };
                            let Some(idx) = tags.index_of(name) else {
                                continue;
                            };
                            let idx = idx as usize;
                            if idx < n_nodes {
                                nodes.push(idx as u32);
                            } else if idx < n_nodes + n_tail {
                                tails.push((idx - n_nodes) as u32);
                            }
                        }
                        nodes.sort_unstable();
                        nodes.dedup();
                        part.record(&nodes, &tails, raw.year[p], raw.rating[p]);
                        batch.node_idx.extend_from_slice(&nodes);
                        batch.node_off.push(batch.node_idx.len() as u32);
                        batch.tail_idx.extend_from_slice(&tails);
                        batch.tail_off.push(batch.tail_idx.len() as u32);
                    }
                    let batch = Arc::new(batch);
                    for tx in &shard_txs {
                        tx.send(batch.clone()).expect("counting worker died");
                    }
                }
                part
            }));
        }
        drop(shard_txs);

        let mut shard_handles = Vec::with_capacity(shards);
        for (shard, rx) in shard_rxs.into_iter().enumerate() {
            let node_cap = params.pairs_hint / shards;
            let tail_cap = params.tail_pairs_hint / shards;
            shard_handles.push(s.spawn(move || {
                let mut node_map: FxHashMap<[u32; 2], u32> =
                    FxHashMap::with_capacity_and_hasher(node_cap, Default::default());
                let mut tail_map: FxHashMap<[u32; 2], u32> =
                    FxHashMap::with_capacity_and_hasher(tail_cap, Default::default());
                let dense_rows = DENSE.div_ceil(shards);
                let mut dense = vec![0u32; dense_rows * DENSE];
                let mut increments = 0u64;
                while let Ok(batch) = rx.recv() {
                    for p in 0..batch.len() {
                        let nodes = &batch.node_idx
                            [batch.node_off[p] as usize..batch.node_off[p + 1] as usize];
                        let dense_end = nodes.partition_point(|&j| (j as usize) < DENSE);
                        for (a, &i) in nodes.iter().enumerate() {
                            if i as usize % shards != shard {
                                continue;
                            }
                            if (i as usize) < DENSE {
                                let row = (i as usize / shards) * DENSE;
                                for &j in &nodes[a + 1..dense_end] {
                                    dense[row + j as usize] += 1;
                                }
                                for &j in &nodes[dense_end.max(a + 1)..] {
                                    *node_map.entry([i, j]).or_insert(0) += 1;
                                }
                            } else {
                                for &j in &nodes[a + 1..] {
                                    *node_map.entry([i, j]).or_insert(0) += 1;
                                }
                            }
                            increments += (nodes.len() - a - 1) as u64;
                        }
                        let tails = &batch.tail_idx
                            [batch.tail_off[p] as usize..batch.tail_off[p + 1] as usize];
                        for &x in tails {
                            if x as usize % shards != shard {
                                continue;
                            }
                            for &j in nodes {
                                *tail_map.entry([x, j]).or_insert(0) += 1;
                            }
                        }
                    }
                }
                for r in 0..dense_rows {
                    let i = r * shards + shard;
                    if i >= DENSE {
                        break;
                    }
                    for j in 0..DENSE {
                        let c = dense[r * DENSE + j];
                        if c > 0 {
                            node_map.insert([i as u32, j as u32], c);
                        }
                    }
                }
                (node_map, tail_map, increments)
            }));
        }

        inflate
            .join()
            .expect("inflate thread panicked")
            .context("inflate posts export")?;
        let totals = parse.join().expect("parse thread panicked")?;
        let mut stats_part = Partial::new(n_nodes, n_tail);
        for h in lookup_handles {
            stats_part.merge(h.join().expect("lookup thread panicked"));
        }
        let mut pairs = 0u64;
        let mut maps = Vec::with_capacity(shards);
        for h in shard_handles {
            let (node_map, tail_map, increments) = h.join().expect("counting worker panicked");
            pairs += increments;
            maps.push((node_map, tail_map));
        }
        Ok((totals, stats_part, pairs, maps))
    })?;
    info!("counting done, {:.0?}", t0.elapsed());
    if stats_part.bad_utf8 > 0 {
        warn!(
            "{} tag tokens were not valid utf-8 and were skipped",
            stats_part.bad_utf8
        );
    }

    let (posts_total, posts_deleted) = totals;
    let years = stats_part
        .year_totals
        .iter()
        .rposition(|&c| c > 0)
        .map_or(0, |y| y + 1);
    let mut compact = Vec::with_capacity(n_nodes * years);
    for i in 0..n_nodes {
        compact.extend_from_slice(&stats_part.node_year[i * MAX_YEARS..i * MAX_YEARS + years]);
    }
    stats_part.year_totals.truncate(years);
    let mut stats = PostStats {
        posts_total,
        posts_deleted,
        posts_kept: stats_part.kept,
        years,
        year_totals: std::mem::take(&mut stats_part.year_totals),
        node_year: compact,
        node_rating: std::mem::take(&mut stats_part.node_rating),
        node_first_year: std::mem::take(&mut stats_part.node_first_year),
        node_posts: std::mem::take(&mut stats_part.node_posts),
        tail_posts: std::mem::take(&mut stats_part.tail_posts),
        distinct_node_pairs: 0,
        distinct_tail_pairs: 0,
        pair_increments: pairs,
    };
    for (node_map, tail_map) in maps {
        stats.distinct_node_pairs += node_map.len() as u64;
        stats.distinct_tail_pairs += tail_map.len() as u64;
        for ([hi, lo], c) in node_map {
            node_sink.push(Pairs::key(hi, lo), c)?;
        }
        for ([hi, lo], c) in tail_map {
            tail_sink.push(Pairs::key(hi, lo), c)?;
        }
    }
    node_sink.finish()?;
    tail_sink.finish()?;
    info!(
        "{} posts ({} deleted), {} pair increments, {} distinct node pairs, {} distinct tail pairs, {:.0?}",
        stats.posts_kept,
        stats.posts_deleted,
        stats.pair_increments,
        stats.distinct_node_pairs,
        stats.distinct_tail_pairs,
        t0.elapsed()
    );
    Ok(stats)
}
