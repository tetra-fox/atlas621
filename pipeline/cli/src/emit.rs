use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use flate2::Compression;
use flate2::write::GzEncoder;
use log::info;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use atlas_core::bin::Le;
use atlas_core::ipc::{self, Column};
use atlas_core::edges::Edges;
use atlas_core::embed::Embedding;
use atlas_core::hex::{Feature, Level};

use crate::posts::{PostStats, YEAR0};
use crate::store::Store;
use crate::tags::Tags;
use crate::territories::{Meta, RegionInfo};

// cloudflare rejects static assets over 25 MiB, kept under with a margin
const MAX_FILE: u64 = 24 * 1024 * 1024;
// uncompressed bytes per part; gzip brings each well under MAX_FILE
const PART_RAW: usize = 32 * 1024 * 1024;

#[derive(Deserialize, Default)]
pub struct Overrides {
    #[serde(default)]
    pub regions: BTreeMap<String, String>,
    #[serde(default)]
    pub continents: BTreeMap<String, String>,
}

const SPACE_SIZE: f64 = 4096.0;
const SPACE_MARGIN: f64 = 0.02;

#[derive(Serialize)]
struct Manifest {
    export_date: String,
    generated_at: String,
    node_floor: u32,
    playground_floor: u32,
    nodes: usize,
    edges: usize,
    space_size: u32,
    max_degree: u32,
    base_links: usize,
    tail_neighbors: usize,
    text_shards: Vec<u32>,
    tile_cutoffs: Vec<usize>,
    adj_shard_size: usize,
    years: Vec<u16>,
    years_over: usize,
    files: BTreeMap<String, u64>,
    parts: BTreeMap<String, usize>,
}

#[derive(Serialize)]
struct NamedCommunity {
    name: String,
    tags: Vec<String>,
    size: usize,
}

#[derive(Serialize)]
struct CommunitiesOut {
    regions: Vec<NamedCommunity>,
    continents: Vec<NamedCommunity>,
    continent_of_region: Vec<u32>,
}

#[derive(Serialize)]
struct LevelOut {
    hex_size: f64,
    origin: [f64; 2],
    features: Vec<Feature>,
}

#[derive(Serialize)]
struct TerritoriesOut {
    regions: LevelOut,
    continents: LevelOut,
}

pub struct Emitter {
    out: PathBuf,
    files: BTreeMap<String, u64>,
    parts: BTreeMap<String, usize>,
}

impl Emitter {
    pub fn new(out: &Path) -> Result<Emitter> {
        if out.exists() {
            fs::remove_dir_all(out).with_context(|| format!("clear {}", out.display()))?;
        }
        fs::create_dir_all(out)?;
        Ok(Emitter {
            out: out.to_path_buf(),
            files: BTreeMap::new(),
            parts: BTreeMap::new(),
        })
    }

    fn record(&mut self, name: &str) -> Result<()> {
        let size = fs::metadata(self.out.join(name))?.len();
        if size > MAX_FILE {
            bail!("{name} is {size} bytes, over the {MAX_FILE} byte static asset limit; split it");
        }
        self.files.insert(name.to_string(), size);
        Ok(())
    }

    fn gz(
        &mut self,
        name: &str,
        write: impl FnOnce(&mut GzEncoder<BufWriter<File>>) -> Result<()>,
    ) -> Result<()> {
        let path = self.out.join(name);
        let mut enc = GzEncoder::new(
            BufWriter::with_capacity(1 << 20, File::create(&path)?),
            Compression::best(),
        );
        write(&mut enc)?;
        enc.finish()?.flush()?;
        self.record(name)
    }

    fn array<T: Column + Le>(&mut self, name: &str, values: &[T]) -> Result<()> {
        let stem = name.strip_suffix(".bin.gz").unwrap_or(name);
        let column = stem.rsplit('.').next().unwrap_or(stem);
        let per_part = PART_RAW / T::SIZE;
        if values.len() <= per_part {
            return self.gz(name, |w| ipc::one(w, column, values));
        }
        let chunks: Vec<&[T]> = values.chunks(per_part).collect();
        for (k, chunk) in chunks.iter().enumerate() {
            self.gz(&format!("{stem}.{k}.bin.gz"), |w| ipc::one(w, column, chunk))?;
        }
        self.parts.insert(name.to_string(), chunks.len());
        Ok(())
    }

    fn json<T: Serialize>(&mut self, name: &str, value: &T) -> Result<()> {
        let path = self.out.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        serde_json::to_writer(BufWriter::new(File::create(&path)?), value)?;
        self.record(name)
    }

    fn text_shards(
        &mut self,
        from: &Path,
        n: usize,
        store_shard_size: usize,
        target: usize,
        fields: impl Fn(usize, &mut Map<String, Value>),
    ) -> Result<Vec<u32>> {
        let mut shards: Vec<Vec<u8>> = Vec::new();
        let mut starts: Vec<u32> = Vec::new();
        let mut current: Vec<u8> = Vec::new();
        for s in 0..n.div_ceil(store_shard_size) {
            let path = from.join(format!("{s:03}.json"));
            let mut shard: BTreeMap<u32, Map<String, Value>> = match File::open(&path) {
                Ok(f) => serde_json::from_reader(BufReader::new(f))
                    .with_context(|| format!("parse {}", path.display()))?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
                Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
            };
            for i in s * store_shard_size..((s + 1) * store_shard_size).min(n) {
                let entry = shard.entry(i as u32).or_default();
                fields(i, entry);
                let key = format!("\"{i}\":");
                let value = serde_json::to_vec(entry)?;
                if !current.is_empty() && current.len() + key.len() + value.len() + 2 > target {
                    current.push(b'}');
                    shards.push(std::mem::take(&mut current));
                }
                if current.is_empty() {
                    starts.push(i as u32);
                    current.push(b'{');
                } else {
                    current.push(b',');
                }
                current.extend_from_slice(key.as_bytes());
                current.extend_from_slice(&value);
            }
        }
        if !current.is_empty() {
            current.push(b'}');
            shards.push(current);
        }
        let dest = self.out.join("text");
        fs::create_dir_all(&dest)?;
        for (k, bytes) in shards.iter().enumerate() {
            fs::write(dest.join(format!("{k}.json")), bytes)?;
        }
        let total: usize = shards.iter().map(Vec::len).sum();
        self.files
            .insert(format!("text/ ({} files)", shards.len()), total as u64);
        Ok(starts)
    }

    fn copy_dir(&mut self, from: &Path, name: &str) -> Result<()> {
        let dest = self.out.join(name);
        fs::create_dir_all(&dest)?;
        let mut total = 0u64;
        let mut count = 0usize;
        for entry in fs::read_dir(from).with_context(|| format!("read {}", from.display()))? {
            let entry = entry?;
            let size = fs::copy(entry.path(), dest.join(entry.file_name()))?;
            if size > MAX_FILE {
                bail!(
                    "{}/{} is {size} bytes, over the static asset limit",
                    name,
                    entry.file_name().display()
                );
            }
            total += size;
            count += 1;
        }
        self.files.insert(format!("{name}/ ({count} files)"), total);
        Ok(())
    }
}

fn display_name(tag: &str) -> String {
    tag.replace('_', " ")
}

fn named(
    meta: &[RegionInfo],
    tags: &Tags,
    membership: &[u32],
    overrides: &BTreeMap<String, String>,
) -> Vec<NamedCommunity> {
    let mut out: Vec<NamedCommunity> = meta
        .iter()
        .map(|c| NamedCommunity {
            name: c.name_tags.first().map_or_else(
                || "unnamed".to_string(),
                |&t| display_name(&tags.names[t as usize]),
            ),
            tags: c
                .name_tags
                .iter()
                .map(|&t| tags.names[t as usize].clone())
                .collect(),
            size: c.size,
        })
        .collect();
    for (member, name) in overrides {
        match tags
            .index_of(member)
            .filter(|&i| (i as usize) < membership.len())
        {
            Some(i) => out[membership[i as usize] as usize].name = name.clone(),
            None => log::warn!("override member {member} is not a node, ignored"),
        }
    }
    out
}

fn base_scene(a: &[u32], b: &[u32], post_counts: &[u32], budget: usize) -> Vec<u32> {
    const NONE: u32 = u32::MAX;
    let mut best = vec![NONE; post_counts.len()];
    for (k, (&x, &y)) in a.iter().zip(b).enumerate() {
        for end in [x, y] {
            if best[end as usize] == NONE {
                best[end as usize] = k as u32;
            }
        }
    }
    let shown = |i: u32| post_counts[i as usize] > 0;
    let mut in_base = vec![false; a.len()];
    let mut chosen: Vec<u32> = Vec::with_capacity(budget.min(a.len()));
    for (i, &k) in best.iter().enumerate() {
        if chosen.len() >= budget {
            break;
        }
        if k == NONE {
            continue;
        }
        let (x, y) = (a[k as usize], b[k as usize]);
        let other = if x as usize == i { y } else { x };
        if (other as usize) < i && best[other as usize] == k {
            continue;
        }
        if !shown(x) || !shown(y) {
            continue;
        }
        chosen.push(k);
        in_base[k as usize] = true;
    }
    for (k, (&x, &y)) in a.iter().zip(b).enumerate() {
        if chosen.len() >= budget {
            break;
        }
        if in_base[k] || !shown(x) || !shown(y) {
            continue;
        }
        chosen.push(k as u32);
        in_base[k] = true;
    }
    chosen.sort_unstable();
    chosen
}

fn space_fit(positions: &[[f32; 2]]) -> Result<(f64, f64, f64)> {
    let (mut min_x, mut max_x) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in positions {
        min_x = min_x.min(p[0] as f64);
        max_x = max_x.max(p[0] as f64);
        min_y = min_y.min(p[1] as f64);
        max_y = max_y.max(p[1] as f64);
    }
    let extent = (max_x - min_x).max(max_y - min_y);
    if extent <= 0.0 {
        bail!("the layout has no extent");
    }
    let scale = (SPACE_SIZE * (1.0 - 2.0 * SPACE_MARGIN)) / extent;
    let ox = SPACE_SIZE / 2.0 - ((min_x + max_x) / 2.0) * scale;
    let oy = SPACE_SIZE / 2.0 - ((min_y + max_y) / 2.0) * scale;
    Ok((scale, ox, oy))
}

fn to_space(level: Level, scale: f64, ox: f64, oy: f64) -> LevelOut {
    LevelOut {
        hex_size: level.hex_size * scale,
        origin: [ox, oy],
        features: level
            .features
            .into_iter()
            .map(|f| Feature {
                anchor: [
                    (f.anchor[0] as f64 * scale + ox) as f32,
                    (f.anchor[1] as f64 * scale + oy) as f32,
                ],
                ..f
            })
            .collect(),
    }
}

fn tiles(
    e: &mut Emitter,
    cutoffs: &[usize],
    positions: &[[f32; 2]],
    a: &[u32],
    b: &[u32],
    weight: &[u16],
) -> Result<()> {
    let levels = cutoffs.len() + 1;
    let mut members: Vec<Vec<Vec<u32>>> = (0..levels)
        .map(|l| vec![Vec::new(); 1 << (2 * l)])
        .collect();
    for (rank, &x) in a.iter().enumerate() {
        let level = cutoffs
            .iter()
            .position(|&c| rank < c)
            .unwrap_or(cutoffs.len());
        let cells = 1usize << level;
        let p = positions[x as usize];
        let at = |v: f32| (((v as f64 / SPACE_SIZE) * cells as f64).floor() as usize).min(cells - 1);
        members[level][at(p[1]) * cells + at(p[0])].push(rank as u32);
    }
    for (level, cells) in members.iter().enumerate() {
        let per = 1usize << level;
        for (i, ranks) in cells.iter().enumerate() {
            if ranks.is_empty() {
                continue;
            }
            let name = format!("tiles/{level}/{}_{}.bin.gz", i % per, i / per);
            fs::create_dir_all(e.out.join(format!("tiles/{level}")))?;
            e.gz(&name, |w| edge_run(w, ranks, a, b, weight))?;
        }
    }
    Ok(())
}

fn adjacency_shards(
    e: &mut Emitter,
    shard_size: usize,
    n: usize,
    a: &[u32],
    b: &[u32],
    weight: &[u16],
) -> Result<()> {
    let mut off = vec![0usize; n + 1];
    for (&x, &y) in a.iter().zip(b) {
        off[x as usize + 1] += 1;
        off[y as usize + 1] += 1;
    }
    for i in 0..n {
        off[i + 1] += off[i];
    }
    let mut fill = off[..n].to_vec();
    let mut far = vec![0u32; off[n]];
    let mut w = vec![0u16; off[n]];
    for ((&x, &y), &wt) in a.iter().zip(b).zip(weight) {
        far[fill[x as usize]] = y;
        w[fill[x as usize]] = wt;
        fill[x as usize] += 1;
        far[fill[y as usize]] = x;
        w[fill[y as usize]] = wt;
        fill[y as usize] += 1;
    }
    let dest = e.out.join("adj");
    fs::create_dir_all(&dest)?;
    let shards = n.div_ceil(shard_size);
    let mut total = 0u64;
    for s in 0..shards {
        let lo = s * shard_size;
        let hi = ((s + 1) * shard_size).min(n);
        let start = off[lo];
        let path = dest.join(format!("{s}.bin.gz"));
        let mut enc = GzEncoder::new(
            BufWriter::with_capacity(1 << 20, File::create(&path)?),
            Compression::best(),
        );
        let local: Vec<u32> = off[lo..=hi].iter().map(|&o| (o - start) as u32).collect();
        ipc::write(
            &mut enc,
            &[
                ("far", ipc::runs(&local, &far[start..off[hi]])?),
                ("weight", ipc::runs(&local, &w[start..off[hi]])?),
            ],
        )?;
        enc.finish()?.flush()?;
        total += fs::metadata(&path)?.len();
    }
    e.files.insert(format!("adj/ ({shards} files)"), total);
    Ok(())
}

fn quantize(w: f32) -> u16 {
    (w.clamp(0.0, 1.0) * 65535.0).round() as u16
}

fn gather<T: Copy>(keys: &[u32], src: &[T]) -> Vec<T> {
    keys.iter().map(|&k| src[k as usize]).collect()
}

// a run of edges picked out of the global edge list by rank; read by readEdgeRun on the web side
fn edge_run<W: Write>(w: W, keys: &[u32], a: &[u32], b: &[u32], weight: &[u16]) -> Result<()> {
    ipc::write(
        w,
        &[
            ("rank", u32::column(keys)),
            ("a", u32::column(&gather(keys, a))),
            ("b", u32::column(&gather(keys, b))),
            ("weight", u16::column(&gather(keys, weight))),
        ],
    )
}

fn similar_lists(emb: &Embedding, take: usize, n: usize) -> Vec<Vec<(u32, u16)>> {
    let mut out = vec![Vec::new(); n];
    let kn = emb.core_knn.len() / emb.core.len().max(1);
    for (i, &node) in emb.core.iter().enumerate() {
        let row = i * kn..i * kn + take.min(kn);
        out[node as usize] = emb.core_knn[row.clone()]
            .iter()
            .zip(&emb.core_knn_sim[row])
            .map(|(&j, &s)| (emb.core[j as usize], quantize(s)))
            .collect();
    }
    let kept = emb.tail_knn.len() / emb.tail.len().max(1);
    for (x, &node) in emb.tail.iter().enumerate() {
        let row = x * kept..x * kept + take.min(kept);
        out[node as usize] = emb.tail_knn[row.clone()]
            .iter()
            .zip(&emb.tail_knn_sim[row])
            .filter(|(j, _)| **j != u32::MAX)
            .map(|(&j, &s)| (emb.core[j as usize], quantize(s)))
            .collect();
    }
    out
}

fn path_graph(e: &mut Emitter, ties: &Edges, post_counts: &[u32], floor: u32) -> Result<()> {
    let n = post_counts.len();
    let keep = |i: u32| post_counts[i as usize] >= floor;
    let nodes: Vec<u32> = (0..n as u32).filter(|&i| keep(i)).collect();
    let mut row = vec![u32::MAX; n];
    for (r, &i) in nodes.iter().enumerate() {
        row[i as usize] = r as u32;
    }
    let mut off = vec![0u32; nodes.len() + 1];
    for (&a, &b) in ties.a.iter().zip(&ties.b) {
        if keep(a) && keep(b) {
            off[row[a as usize] as usize + 1] += 1;
            off[row[b as usize] as usize + 1] += 1;
        }
    }
    for r in 0..nodes.len() {
        off[r + 1] += off[r];
    }
    let m = off[nodes.len()] as usize;
    let mut fill: Vec<u32> = off[..nodes.len()].to_vec();
    let mut far = vec![0u32; m];
    let mut weight = vec![0u16; m];
    for ((&a, &b), &w) in ties.a.iter().zip(&ties.b).zip(&ties.weight) {
        if !(keep(a) && keep(b)) {
            continue;
        }
        for (me, other) in [(a, b), (b, a)] {
            let slot = &mut fill[row[me as usize] as usize];
            far[*slot as usize] = other;
            weight[*slot as usize] = quantize(w);
            *slot += 1;
        }
    }
    e.gz("paths.bin.gz", |w| {
        ipc::write(
            w,
            &[
                ("node", u32::column(&nodes)),
                ("far", ipc::runs(&off, &far)?),
                ("weight", ipc::runs(&off, &weight)?),
            ],
        )
    })
}

fn vectors(e: &mut Emitter, emb: &Embedding, post_counts: &[u32], floor: u32) -> Result<()> {
    let dim = emb.dim;
    let rows: Vec<usize> = (0..emb.core.len())
        .filter(|&i| post_counts[emb.core[i] as usize] >= floor)
        .collect();
    let nodes: Vec<u32> = rows.iter().map(|&i| emb.core[i]).collect();
    let mut data = Vec::with_capacity(rows.len() * dim);
    for &i in &rows {
        for &v in &emb.vectors[i * dim..(i + 1) * dim] {
            data.push((v.clamp(-1.0, 1.0) * 127.0).round() as i8);
        }
    }
    e.gz("vectors.bin.gz", |w| {
        ipc::write(
            w,
            &[("node", u32::column(&nodes)), ("vector", ipc::rows(dim, &data)?)],
        )
    })
}

pub struct EmitInputs<'a> {
    pub tags: &'a Tags,
    pub stats: &'a PostStats,
    pub edges: &'a Edges,
    pub ties: &'a Edges,
    pub embedding: &'a Embedding,
    pub positions: &'a [[f32; 2]],
    pub region: &'a [u32],
    pub continent: &'a [u32],
    pub communities: &'a Meta,
    pub territories_regions: Level,
    pub territories_continents: Level,
    pub overrides: &'a Overrides,
    pub export_date: &'a str,
    pub store_shard_size: usize,
    pub text_shard_bytes: usize,
    pub tile_cutoffs: Vec<usize>,
    pub adj_shard_size: usize,
    pub tail_neighbors: usize,
    pub base_links: usize,
    pub shard_neighbors: usize,
    pub playground_floor: u32,
}

pub fn write(out_dir: &Path, store: &Store, inputs: EmitInputs) -> Result<()> {
    let tags = inputs.tags;
    let n = tags.n_nodes;
    let m = inputs.edges.len();
    let mut e = Emitter::new(out_dir)?;

    let (scale, ox, oy) = space_fit(inputs.positions)?;
    let positions: Vec<[f32; 2]> = inputs
        .positions
        .iter()
        .map(|p| [(p[0] as f64 * scale + ox) as f32, (p[1] as f64 * scale + oy) as f32])
        .collect();
    e.gz("positions.bin.gz", |w| {
        ipc::write(w, &[("position", ipc::rows(2, positions.as_flattened())?)])
    })?;
    e.array("post_counts.bin.gz", &tags.post_counts[..n])?;
    e.array("categories.bin.gz", &tags.categories[..n])?;
    e.array("tag_ids.bin.gz", &tags.ids[..n])?;
    e.array("first_year.bin.gz", &inputs.stats.node_first_year)?;
    if inputs.communities.regions.len() >= u16::MAX as usize
        || inputs.communities.continents.len() >= u16::MAX as usize
    {
        bail!("more than 65534 communities does not fit the u16 membership arrays");
    }
    let to_u16 = |v: &[u32]| v.iter().map(|&x| x as u16).collect::<Vec<u16>>();
    e.array("region.bin.gz", &to_u16(inputs.region))?;
    e.array("continent.bin.gz", &to_u16(inputs.continent))?;

    e.gz("names.bin.gz", |w| {
        ipc::write(w, &[("name", ipc::strings(&tags.names[..n]))])
    })?;

    let quantized: Vec<u16> = inputs.edges.weight.iter().map(|&w| quantize(w)).collect();
    let mut order: Vec<usize> = (0..m).collect();
    order.sort_unstable_by_key(|&k| (Reverse(quantized[k]), inputs.edges.a[k], inputs.edges.b[k]));
    let permuted = |values: &[u32]| order.iter().map(|&k| values[k]).collect::<Vec<u32>>();
    let a = permuted(&inputs.edges.a);
    let b = permuted(&inputs.edges.b);
    let weight: Vec<u16> = order.iter().map(|&k| quantized[k]).collect();
    let count = permuted(&inputs.edges.count);
    e.array("edges.a.bin.gz", &a)?;
    e.array("edges.b.bin.gz", &b)?;
    e.array("edges.weight.bin.gz", &weight)?;
    e.array("edges.count.bin.gz", &count)?;
    let base = base_scene(&a, &b, &tags.post_counts[..n], inputs.base_links);
    e.gz("base.bin.gz", |w| edge_run(w, &base, &a, &b, &weight))?;
    tiles(&mut e, &inputs.tile_cutoffs, &positions, &a, &b, &weight)?;
    adjacency_shards(&mut e, inputs.adj_shard_size, n, &a, &b, &weight)?;
    path_graph(&mut e, inputs.ties, &tags.post_counts[..n], inputs.playground_floor)?;
    vectors(&mut e, inputs.embedding, &tags.post_counts[..n], inputs.playground_floor)?;
    let mut degree = vec![0u32; n];
    for &x in a.iter().chain(&b) {
        degree[x as usize] += 1;
    }
    let max_degree = degree.into_iter().max().unwrap_or(0);

    let mut rows: Vec<u16> = Vec::with_capacity(inputs.stats.node_year.len());
    let mut over: Vec<u32> = Vec::new();
    for (cell, &v) in inputs.stats.node_year.iter().enumerate() {
        match u16::try_from(v) {
            Ok(v) => rows.push(v),
            Err(_) => {
                rows.push(u16::MAX);
                over.extend([cell as u32, v]);
            }
        }
    }
    e.array("years.bin.gz", &rows)?;
    e.array("years_over.bin.gz", &over)?;

    let implications: serde_json::Value = store.load_json("text.implications")?;
    e.json("implications.json", &implications)?;
    let changelog: serde_json::Value = store.load_json("text.changelog")?;
    e.json("changelog.json", &changelog)?;
    e.json(
        "communities.json",
        &CommunitiesOut {
            regions: named(
                &inputs.communities.regions,
                tags,
                inputs.region,
                &inputs.overrides.regions,
            ),
            continents: named(
                &inputs.communities.continents,
                tags,
                inputs.continent,
                &inputs.overrides.continents,
            ),
            continent_of_region: inputs.communities.continent_of_region.clone(),
        },
    )?;
    e.json(
        "territories.json",
        &TerritoriesOut {
            regions: to_space(inputs.territories_regions, scale, ox, oy),
            continents: to_space(inputs.territories_continents, scale, ox, oy),
        },
    )?;
    let mut appears: Vec<Vec<(u32, u16, u32)>> = vec![Vec::new(); n];
    {
        let ties = inputs.ties;
        let mut order: Vec<usize> = (0..ties.len()).collect();
        order.sort_unstable_by(|&x, &y| {
            ties.weight[y]
                .total_cmp(&ties.weight[x])
                .then((ties.a[x], ties.b[x]).cmp(&(ties.a[y], ties.b[y])))
        });
        for k in order {
            let w = quantize(ties.weight[k]);
            for (me, other) in [(ties.a[k], ties.b[k]), (ties.b[k], ties.a[k])] {
                let list = &mut appears[me as usize];
                if list.len() < inputs.shard_neighbors {
                    list.push((other, w, ties.count[k]));
                }
            }
        }
    }
    let similar = similar_lists(inputs.embedding, inputs.shard_neighbors, n);
    let text_shards = e.text_shards(
        &store.path("text"),
        n,
        inputs.store_shard_size,
        inputs.text_shard_bytes,
        |i, entry| {
            entry.insert("r".into(), json!(&inputs.stats.node_rating[i * 3..i * 3 + 3]));
            entry.insert("g".into(), json!(inputs.region[i]));
            if !appears[i].is_empty() {
                let flat: Vec<u32> = appears[i]
                    .iter()
                    .flat_map(|&(other, w, c)| [other, u32::from(w), c])
                    .collect();
                entry.insert("e".into(), json!(flat));
            }
            if !similar[i].is_empty() {
                let flat: Vec<u32> = similar[i]
                    .iter()
                    .flat_map(|&(other, w)| [other, u32::from(w)])
                    .collect();
                entry.insert("s".into(), json!(flat));
            }
        },
    )?;
    e.copy_dir(&store.path("search"), "search")?;

    let manifest = Manifest {
        export_date: inputs.export_date.to_string(),
        generated_at: jiff::Timestamp::now()
            .strftime("%Y-%m-%dT%H:%M:%SZ")
            .to_string(),
        node_floor: tags.node_floor,
        playground_floor: inputs.playground_floor,
        nodes: n,
        edges: m,
        space_size: SPACE_SIZE as u32,
        max_degree,
        base_links: base.len(),
        tail_neighbors: inputs.tail_neighbors,
        text_shards,
        tile_cutoffs: inputs.tile_cutoffs.clone(),
        adj_shard_size: inputs.adj_shard_size,
        years: (0..inputs.stats.years).map(|y| YEAR0 + y as u16).collect(),
        years_over: over.len() / 2,
        files: e.files.clone(),
        parts: e.parts.clone(),
    };
    e.json("manifest.json", &manifest)?;
    let total: u64 = e.files.values().sum();
    info!(
        "dataset written to {}: {} entries, {:.1} MB",
        out_dir.display(),
        e.files.len(),
        total as f64 / 1e6
    );
    for (name, size) in &e.files {
        info!("  {name}: {:.2} MB", *size as f64 / 1e6);
    }
    Ok(())
}
