use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use atlas_core::bin::{self, Le};
use atlas_core::edges::Edges;
use atlas_core::embed::Embedding;
use atlas_core::pairs::Pairs;

use crate::posts::PostStats;
use crate::tags::{self, Tags};

pub struct Store {
    dir: PathBuf,
}

pub struct PairSink {
    keys: BufWriter<File>,
    counts: BufWriter<File>,
}

impl PairSink {
    pub fn push(&mut self, key: u64, count: u32) -> Result<()> {
        self.keys.write_all(&key.to_le_bytes())?;
        self.counts.write_all(&count.to_le_bytes())?;
        Ok(())
    }

    pub fn finish(&mut self) -> Result<()> {
        self.keys.flush()?;
        self.counts.flush()?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct TagsMeta {
    n_nodes: usize,
    n_tail: usize,
    node_floor: u32,
}

#[derive(Serialize, Deserialize)]
struct EmbedMeta {
    dim: usize,
}

#[derive(Serialize, Deserialize)]
struct PostStatsMeta {
    posts_total: u64,
    posts_deleted: u64,
    posts_kept: u64,
    years: usize,
    year_totals: Vec<u64>,
    distinct_node_pairs: u64,
    distinct_tail_pairs: u64,
    pair_increments: u64,
}

impl Store {
    pub fn new(dir: &Path) -> Self {
        Store {
            dir: dir.to_path_buf(),
        }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    pub fn save<T: Le>(&self, name: &str, values: &[T]) -> Result<()> {
        let path = self.path(&format!("{name}.bin"));
        let mut w = BufWriter::with_capacity(1 << 20, File::create(&path)?);
        bin::write_all(&mut w, values)?;
        w.flush()?;
        Ok(())
    }

    pub fn load<T: Le>(&self, name: &str) -> Result<Vec<T>> {
        let path = self.path(&format!("{name}.bin"));
        let mut r = BufReader::with_capacity(
            1 << 20,
            File::open(&path).with_context(|| format!("open {}", path.display()))?,
        );
        Ok(bin::read_all(&mut r)?)
    }

    pub fn save_json<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        let path = self.path(&format!("{name}.json"));
        serde_json::to_writer(BufWriter::new(File::create(&path)?), value)?;
        Ok(())
    }

    pub fn load_json<T: DeserializeOwned>(&self, name: &str) -> Result<T> {
        let path = self.path(&format!("{name}.json"));
        let file = File::open(&path).with_context(|| format!("open {}", path.display()))?;
        Ok(serde_json::from_reader(BufReader::new(file))?)
    }

    pub fn shard_dir(&self, dir: &str) -> Result<PathBuf> {
        let path = self.path(dir);
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
        }
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub fn save_shards<'a, T: Serialize + 'a>(
        &self,
        dir: &str,
        shards: impl Iterator<Item = (String, &'a T)>,
    ) -> Result<()> {
        let path = self.shard_dir(dir)?;
        for (key, value) in shards {
            serde_json::to_writer(
                BufWriter::new(File::create(path.join(format!("{key}.json")))?),
                value,
            )?;
        }
        Ok(())
    }

    pub fn save_tags(&self, tags: &Tags) -> Result<()> {
        self.save("tags.ids", &tags.ids)?;
        self.save("tags.categories", &tags.categories)?;
        self.save("tags.post_counts", &tags.post_counts)?;
        self.save("tags.name_offsets", &tags::name_offsets(&tags.names))?;
        std::fs::write(self.path("tags.names.bin"), tags.names.concat())?;
        self.save_json(
            "tags.meta",
            &TagsMeta {
                n_nodes: tags.n_nodes,
                n_tail: tags.n_tail,
                node_floor: tags.node_floor,
            },
        )
    }

    pub fn load_tags(&self) -> Result<Tags> {
        let meta: TagsMeta = self.load_json("tags.meta")?;
        let offsets: Vec<u32> = self.load("tags.name_offsets")?;
        let blob = std::fs::read_to_string(self.path("tags.names.bin"))?;
        let names = offsets
            .windows(2)
            .map(|w| blob[w[0] as usize..w[1] as usize].to_owned())
            .collect();
        Ok(Tags::from_parts(
            self.load("tags.ids")?,
            names,
            self.load("tags.categories")?,
            self.load("tags.post_counts")?,
            meta.n_nodes,
            meta.n_tail,
            meta.node_floor,
        ))
    }

    pub fn save_post_stats(&self, s: &PostStats) -> Result<()> {
        self.save("posts.node_year", &s.node_year)?;
        self.save("posts.node_rating", &s.node_rating)?;
        self.save("posts.node_first_year", &s.node_first_year)?;
        self.save("posts.node_posts", &s.node_posts)?;
        self.save("posts.tail_posts", &s.tail_posts)?;
        self.save_json(
            "posts.meta",
            &PostStatsMeta {
                posts_total: s.posts_total,
                posts_deleted: s.posts_deleted,
                posts_kept: s.posts_kept,
                years: s.years,
                year_totals: s.year_totals.clone(),
                distinct_node_pairs: s.distinct_node_pairs,
                distinct_tail_pairs: s.distinct_tail_pairs,
                pair_increments: s.pair_increments,
            },
        )
    }

    pub fn load_post_stats(&self) -> Result<PostStats> {
        let m: PostStatsMeta = self.load_json("posts.meta")?;
        Ok(PostStats {
            posts_total: m.posts_total,
            posts_deleted: m.posts_deleted,
            posts_kept: m.posts_kept,
            years: m.years,
            year_totals: m.year_totals,
            node_year: self.load("posts.node_year")?,
            node_rating: self.load("posts.node_rating")?,
            node_first_year: self.load("posts.node_first_year")?,
            node_posts: self.load("posts.node_posts")?,
            tail_posts: self.load("posts.tail_posts")?,
            distinct_node_pairs: m.distinct_node_pairs,
            distinct_tail_pairs: m.distinct_tail_pairs,
            pair_increments: m.pair_increments,
        })
    }

    pub fn pair_sink(&self, name: &str) -> Result<PairSink> {
        Ok(PairSink {
            keys: BufWriter::with_capacity(
                1 << 20,
                File::create(self.path(&format!("{name}.keys.bin")))?,
            ),
            counts: BufWriter::with_capacity(
                1 << 20,
                File::create(self.path(&format!("{name}.counts.bin")))?,
            ),
        })
    }

    pub fn load_pairs(&self, name: &str) -> Result<Pairs> {
        Ok(Pairs {
            keys: self.load(&format!("{name}.keys"))?,
            counts: self.load(&format!("{name}.counts"))?,
        })
    }

    pub fn save_edges(&self, name: &str, e: &Edges) -> Result<()> {
        self.save(&format!("{name}.a"), &e.a)?;
        self.save(&format!("{name}.b"), &e.b)?;
        self.save(&format!("{name}.weight"), &e.weight)?;
        self.save(&format!("{name}.count"), &e.count)
    }

    pub fn load_edges(&self, name: &str) -> Result<Edges> {
        Ok(Edges {
            a: self.load(&format!("{name}.a"))?,
            b: self.load(&format!("{name}.b"))?,
            weight: self.load(&format!("{name}.weight"))?,
            count: self.load(&format!("{name}.count"))?,
        })
    }

    pub fn save_embedding(&self, e: &Embedding) -> Result<()> {
        self.save("embed.core", &e.core)?;
        self.save("embed.vectors", &e.vectors)?;
        self.save("embed.core_knn", &e.core_knn)?;
        self.save("embed.core_knn_sim", &e.core_knn_sim)?;
        self.save("embed.tail", &e.tail)?;
        self.save("embed.tail_knn", &e.tail_knn)?;
        self.save("embed.tail_knn_sim", &e.tail_knn_sim)?;
        self.save_json("embed.meta", &EmbedMeta { dim: e.dim })
    }

    pub fn load_embedding(&self) -> Result<Embedding> {
        let meta: EmbedMeta = self.load_json("embed.meta")?;
        Ok(Embedding {
            core: self.load("embed.core")?,
            dim: meta.dim,
            vectors: self.load("embed.vectors")?,
            core_knn: self.load("embed.core_knn")?,
            core_knn_sim: self.load("embed.core_knn_sim")?,
            tail: self.load("embed.tail")?,
            tail_knn: self.load("embed.tail_knn")?,
            tail_knn_sim: self.load("embed.tail_knn_sim")?,
        })
    }
}
