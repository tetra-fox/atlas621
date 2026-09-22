mod emit;
mod exports;
mod names;
mod posts;
mod previous;
mod search;
mod store;
mod tags;
mod territories;
mod text;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use atlas_core::{communities, edges, embed, hex, layout, tsne};
use clap::{Args, Parser, Subcommand};

use store::Store;
use tracing::{debug, error, info};

#[derive(Parser)]
#[command(about = "turns the e621 db exports into the atlas621 map dataset")]
struct Cli {
    #[arg(long, default_value_os_t = default_cache_dir())]
    cache_dir: PathBuf,
    /// where each stage leaves its output for the next one
    #[arg(long, default_value = "pipeline/work")]
    work_dir: PathBuf,
    #[arg(long)]
    offline: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// download the e621 db exports into the cache
    Fetch,
    /// scan the posts export for tag pairs and per-year totals
    Count(CountArgs),
    /// keep each tag's strongest pairings as the graph edges
    Edges(EdgesArgs),
    /// factor the pair counts into vectors, then the knn and affinity graphs
    Embed(EmbedArgs),
    /// group tags into regions with leiden
    Communities(CommunityArgs),
    /// place every tag on the plane
    Layout(LayoutArgs),
    /// cut the layout into hex regions and continents, then name them
    Territories(TerritoryArgs),
    /// build the wiki, alias and history shards and the search index
    Text(TextArgs),
    /// write the dataset the site loads
    Emit(EmitArgs),
    /// run every stage in order, fetch through emit
    All(Box<AllArgs>),
}

#[derive(Args, Clone)]
struct CountArgs {
    #[arg(long, default_value_t = 1)]
    node_floor: u32,
    #[arg(long)]
    shards: Option<usize>,
    #[arg(long, default_value_t = 330_000_000)]
    pairs_hint: usize,
    #[arg(long, default_value_t = 1_000_000)]
    tail_pairs_hint: usize,
}

#[derive(Args, Clone)]
struct EdgesArgs {
    #[arg(long, value_enum, default_value = "npmi")]
    weight: edges::Weight,
    #[arg(long, default_value_t = 20)]
    top_k: usize,
    #[arg(long, default_value_t = 20)]
    min_count: u32,
    #[arg(long, default_value_t = 3)]
    tail_neighbors: usize,
    #[arg(long, default_value_t = 0.01)]
    min_weight: f32,
    #[arg(long, default_value_t = 3)]
    min_degree: usize,
    #[arg(long, default_value_t = 10)]
    core_floor: u32,
    #[arg(long, default_value_t = 5)]
    small_k: usize,
}

#[derive(Args, Clone)]
struct EmbedArgs {
    #[arg(long, default_value_t = 10)]
    embed_core_floor: u32,
    #[arg(long, default_value_t = 3)]
    embed_min_count: u32,
    #[arg(long, default_value_t = 128)]
    dim: usize,
    #[arg(long, default_value_t = 10)]
    oversample: usize,
    #[arg(long, default_value_t = 4)]
    power_iterations: usize,
    #[arg(long, default_value_t = 1.0)]
    shift: f64,
    #[arg(long, default_value_t = 90)]
    neighbors: usize,
    #[arg(long, default_value_t = 5)]
    embed_tail_neighbors: usize,
    #[arg(long, default_value_t = 15)]
    graph_neighbors: usize,
    #[arg(long, default_value_t = 3)]
    graph_tail_neighbors: usize,
    #[arg(long, default_value_t = 30.0)]
    perplexity: f64,
    #[arg(long, default_value_t = 621)]
    embed_seed: u64,
}

#[derive(Args, Clone)]
struct CommunityArgs {
    #[arg(long, value_enum, default_value = "affinities")]
    graph: Graph,
    #[arg(long, default_value_t = 6.0)]
    region_resolution: f64,
    #[arg(long, default_value_t = 30)]
    min_region: usize,
    #[arg(long, default_value_t = 621)]
    community_seed: u64,
    #[arg(long, default_value_t = 2)]
    refine_sweeps: usize,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Graph {
    Knn,
    Edges,
    Affinities,
}

impl Graph {
    fn name(self) -> &'static str {
        match self {
            Graph::Knn => "knn",
            Graph::Edges => "edges",
            Graph::Affinities => "affinities",
        }
    }
}

#[derive(Args, Clone)]
struct LayoutArgs {
    #[arg(long, default_value_t = 600)]
    iterations: usize,
    #[arg(long, default_value_t = 2.0)]
    scaling: f32,
    #[arg(long, default_value_t = 0.05)]
    gravity: f32,
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    strong_gravity: bool,
    #[arg(long)]
    lin_log: bool,
    #[arg(long, default_value_t = 1.2)]
    theta: f32,
    #[arg(long, default_value_t = 1.0)]
    jitter_tolerance: f32,
    #[arg(long, default_value_t = 0.02)]
    community_gravity: f32,
    #[arg(long, default_value_t = 1.0)]
    weight_exponent: f32,
    #[arg(long, default_value_t = 621)]
    layout_seed: u64,
    #[arg(long)]
    previous: Option<String>,
    #[arg(long, value_enum, default_value = "tsne")]
    mode: Mode,
    #[arg(long, default_value_t = 4.0)]
    exaggeration: f64,
    #[arg(long, default_value_t = 500)]
    tsne_iterations: usize,
    #[arg(long, default_value_t = 0.5)]
    tsne_theta: f32,
    #[arg(long, default_value_t = 30.0)]
    region_perplexity: f64,
    #[arg(long, default_value_t = 3000.0)]
    extent: f32,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Mode {
    Tsne,
    Forceatlas2,
}

#[derive(Args, Clone)]
struct TerritoryArgs {
    #[arg(long, default_value_t = 220)]
    region_cols: usize,
    #[arg(long, default_value_t = 110)]
    continent_cols: usize,
    #[arg(long, default_value_t = 2)]
    rings: usize,
    #[arg(long, default_value_t = 1500)]
    region_min_nodes: usize,
    #[arg(long, default_value_t = 15000)]
    continent_min_nodes: usize,
    #[arg(long, default_value_t = 5)]
    name_tags: usize,
    #[arg(long, default_value_t = 0.1)]
    min_fit: f64,
    #[arg(long, default_value_t = 1000)]
    name_candidate_posts: u32,
    #[arg(long, default_value_t = 100)]
    name_member_posts: u32,
}

#[derive(Args, Clone)]
struct TextArgs {
    #[arg(long, default_value_t = 300)]
    lump_threshold: usize,
    #[arg(long, default_value_t = 300)]
    excerpt_chars: usize,
    #[arg(long, default_value_t = 1024)]
    shard_size: usize,
}

#[derive(Args, Clone)]
struct EmitArgs {
    #[arg(long, default_value = "static/data")]
    out_dir: PathBuf,
    #[arg(long, default_value = "pipeline/overrides.json")]
    overrides: PathBuf,
    #[arg(long, value_enum, default_value = "knn")]
    links: Graph,
    #[arg(long, default_value_t = 150_000)]
    base_links: usize,
    #[arg(long, default_value_t = 20)]
    shard_neighbors: usize,
    #[arg(long, default_value_t = 256 * 1024)]
    text_shard_bytes: usize,
    #[arg(long, value_delimiter = ',', default_values_t = [700_000, 1_400_000, 2_800_000])]
    tile_cutoffs: Vec<usize>,
    #[arg(long, default_value_t = 1024)]
    adj_shard_size: usize,
    #[arg(long, default_value_t = 100)]
    playground_floor: u32,
}

#[derive(Args, Clone)]
struct AllArgs {
    #[command(flatten)]
    count: CountArgs,
    #[command(flatten)]
    edges: EdgesArgs,
    #[command(flatten)]
    embed: EmbedArgs,
    #[command(flatten)]
    communities: CommunityArgs,
    #[command(flatten)]
    layout: LayoutArgs,
    #[command(flatten)]
    territories: TerritoryArgs,
    #[command(flatten)]
    text: TextArgs,
    #[command(flatten)]
    emit: EmitArgs,
}

fn default_cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"));
    base.join("atlas621").join("exports")
}

struct Ctx {
    cli_cache: PathBuf,
    offline: bool,
    store: Store,
}

impl Ctx {
    fn manifest(&self) -> Result<Vec<exports::Export>> {
        exports::load_manifest(&self.cli_cache, self.offline)
    }

    fn open(
        &self,
        manifest: &[exports::Export],
        name: &str,
    ) -> Result<Box<dyn std::io::Read + Send>> {
        exports::open(&exports::ensure(
            exports::find(manifest, name)?,
            &self.cli_cache,
        )?)
    }
}

fn fetch(ctx: &Ctx) -> Result<()> {
    let manifest = ctx.manifest()?;
    for name in exports::NEEDED {
        exports::ensure(exports::find(&manifest, name)?, &ctx.cli_cache)?;
    }
    Ok(())
}

fn count(ctx: &Ctx, args: &CountArgs) -> Result<()> {
    let manifest = ctx.manifest()?;
    let tags = tags::Tags::load(ctx.open(&manifest, "tags")?, args.node_floor)?;
    ctx.store.save_tags(&tags)?;
    let shards = args.shards.unwrap_or_else(|| {
        (std::thread::available_parallelism().map_or(4, |n| n.get()) * 4).clamp(4, 32)
    });
    let mut node_sink = ctx.store.pair_sink("node_pairs")?;
    let mut tail_sink = ctx.store.pair_sink("tail_pairs")?;
    let stats = posts::count(
        ctx.open(&manifest, "posts")?,
        &tags,
        &posts::CountParams {
            shards,
            pairs_hint: args.pairs_hint,
            tail_pairs_hint: args.tail_pairs_hint,
        },
        &mut node_sink,
        &mut tail_sink,
    )?;
    ctx.store.save_post_stats(&stats)
}

fn edges(ctx: &Ctx, args: &EdgesArgs) -> Result<()> {
    let tags = ctx.store.load_tags()?;
    let stats = ctx.store.load_post_stats()?;
    let node_pairs = ctx.store.load_pairs("node_pairs")?;
    let selected = edges::select(
        &node_pairs,
        &stats.node_posts,
        &edges::EdgeParams {
            weight: args.weight,
            posts: stats.posts_kept,
            top_k: args.top_k,
            min_count: args.min_count,
            min_weight: args.min_weight,
            min_degree: args.min_degree,
            core_floor: args.core_floor,
            small_k: args.small_k,
        },
    );
    drop(node_pairs);
    ctx.store.save_edges("edges", &selected)?;
    ctx.store.save_json(
        "edges.meta",
        &edges::EdgesMeta {
            weight: args.weight,
            core_floor: args.core_floor,
            tail_neighbors: args.tail_neighbors,
        },
    )?;
    let tail_pairs = ctx.store.load_pairs("tail_pairs")?;
    let tail = edges::tail_neighbors(
        &tail_pairs,
        &stats.node_posts,
        &stats.tail_posts,
        tags.n_nodes,
        args.tail_neighbors,
        args.weight,
        stats.posts_kept,
    );
    ctx.store.save("tail_neighbors", &tail)
}

fn embed(ctx: &Ctx, args: &EmbedArgs) -> Result<()> {
    let stats = ctx.store.load_post_stats()?;
    let node_pairs = ctx.store.load_pairs("node_pairs")?;
    let embedding = embed::embed(
        &node_pairs,
        &stats.node_posts,
        stats.posts_kept,
        &embed::EmbedParams {
            core_floor: args.embed_core_floor,
            min_count: args.embed_min_count,
            dim: args.dim,
            oversample: args.oversample,
            power_iterations: args.power_iterations,
            shift: args.shift,
            neighbors: args.neighbors,
            tail_neighbors: args.embed_tail_neighbors,
            seed: args.embed_seed,
        },
    );
    ctx.store.save_embedding(&embedding)?;
    let graph = embed::knn_edges(&embedding, args.graph_neighbors, args.graph_tail_neighbors, &node_pairs);
    drop(node_pairs);
    ctx.store.save_edges("knn", &graph)?;
    let affinities = tsne::affinity_edges(&embedding, args.perplexity, args.graph_tail_neighbors);
    ctx.store.save_edges("affinities", &affinities)
}

fn communities(ctx: &Ctx, args: &CommunityArgs) -> Result<()> {
    let tags = ctx.store.load_tags()?;
    let graph = ctx.store.load_edges(args.graph.name())?;
    let edges_meta: edges::EdgesMeta = ctx.store.load_json("edges.meta")?;
    let region = communities::detect(
        &graph,
        tags.n_nodes,
        &tags.post_counts[..tags.n_nodes],
        &communities::CommunityParams {
            region_resolution: args.region_resolution,
            seed: args.community_seed,
            min_region: args.min_region,
            core_floor: edges_meta.core_floor,
            refine_sweeps: args.refine_sweeps,
        },
    )?;
    ctx.store.save("communities.region", &region)
}

fn layout(ctx: &Ctx, args: &LayoutArgs) -> Result<()> {
    let tags = ctx.store.load_tags()?;
    let region: Vec<u32> = ctx.store.load("communities.region")?;
    let initial = match &args.previous {
        Some(source) => Some(previous::seed_positions(&tags, source)?),
        None => None,
    };
    if let Mode::Tsne = args.mode {
        let embedding = ctx.store.load_embedding()?;
        let graph = ctx.store.load_edges("affinities")?;
        let pos = tsne::layout(
            tags.n_nodes,
            &embedding,
            &graph,
            &region,
            initial.as_deref(),
            &tsne::TsneParams {
                exaggeration: args.exaggeration,
                iterations: args.tsne_iterations,
                theta: args.tsne_theta,
                region_perplexity: args.region_perplexity,
                extent: args.extent,
                seed: args.layout_seed,
            },
        );
        return ctx.store.save("layout.positions", &pos);
    }
    let edges = ctx.store.load_edges("edges")?;
    let params = layout::LayoutParams {
        iterations: args.iterations,
        scaling: args.scaling,
        gravity: args.gravity,
        strong_gravity: args.strong_gravity,
        lin_log: args.lin_log,
        theta: args.theta,
        jitter_tolerance: args.jitter_tolerance,
        community_gravity: args.community_gravity,
        weight_exponent: args.weight_exponent,
        seed: args.layout_seed,
    };
    let mut lay = layout::Layout::new(tags.n_nodes, &edges, &region, initial.as_deref(), params);
    lay.run();
    ctx.store.save("layout.positions", &lay.pos)
}

fn territories(ctx: &Ctx, args: &TerritoryArgs) -> Result<()> {
    let pos: Vec<[f32; 2]> = ctx.store.load("layout.positions")?;
    let tags = ctx.store.load_tags()?;
    let ties = ctx.store.load_edges("edges")?;
    let regions = hex::cut(
        &pos,
        &hex::CutParams {
            hex_cols: args.region_cols,
            rings: args.rings,
            min_nodes: args.region_min_nodes,
        },
    )?;
    let continents = hex::cut(
        &pos,
        &hex::CutParams {
            hex_cols: args.continent_cols,
            rings: args.rings,
            min_nodes: args.continent_min_nodes,
        },
    )?;
    let names = names::NameParams {
        min_fit: args.min_fit,
        candidate_posts: args.name_candidate_posts,
        member_posts: args.name_member_posts,
        take: args.name_tags,
    };
    let (post_counts, categories) = (
        &tags.post_counts[..tags.n_nodes],
        &tags.categories[..tags.n_nodes],
    );
    let meta = territories::Meta {
        regions: territories::name(&regions, &ties, post_counts, categories, &names),
        continents: territories::name(&continents, &ties, post_counts, categories, &names),
        continent_of_region: hex::continent_of_region(&regions, &continents),
    };
    ctx.store.save("territories.region", &regions.membership)?;
    ctx.store.save("territories.continent", &continents.membership)?;
    ctx.store.save_json("territories", &meta)?;
    ctx.store.save_json("territories.regions", &regions.level)?;
    ctx.store.save_json("territories.continents", &continents.level)
}

fn text(ctx: &Ctx, args: &TextArgs) -> Result<()> {
    let manifest = ctx.manifest()?;
    let tags = ctx.store.load_tags()?;
    let out = text::build(
        &tags,
        ctx.open(&manifest, "tag_aliases")?,
        ctx.open(&manifest, "tag_implications")?,
        ctx.open(&manifest, "wiki_pages")?,
        ctx.open(&manifest, "bulk_update_requests")?,
        &text::TextParams {
            lump_threshold: args.lump_threshold,
            excerpt_chars: args.excerpt_chars,
            shard_size: args.shard_size,
        },
    )?;
    ctx.store
        .save_json("text.implications", &out.implications)?;
    ctx.store.save_json("text.changelog", &out.changelog)?;
    ctx.store.save_json("text.aliases", &out.aliases)?;
    ctx.store.save_json(
        "text.meta",
        &text::TextMeta {
            shard_size: args.shard_size,
        },
    )?;
    ctx.store.save_shards(
        "text",
        out.shards.iter().map(|(k, v)| (format!("{k:03}"), v)),
    )?;
    let tail: Vec<u32> = ctx.store.load("tail_neighbors")?;
    let edges_meta: edges::EdgesMeta = ctx.store.load_json("edges.meta")?;
    let shards = search::build(&tags, &out.aliases, &tail, edges_meta.tail_neighbors);
    let entries: usize = shards.values().map(Vec::len).sum();
    tracing::info!("{} search shards, {entries} entries", shards.len());
    let dir = ctx.store.shard_dir("search")?;
    for (key, rows) in &shards {
        let file = std::fs::File::create(dir.join(format!("{key}.bin.gz")))?;
        let mut gz = flate2::write::GzEncoder::new(
            std::io::BufWriter::new(file),
            flate2::Compression::best(),
        );
        search::write_shard(&mut gz, rows)?;
        gz.finish()?;
    }
    Ok(())
}

fn emit(ctx: &Ctx, args: &EmitArgs) -> Result<()> {
    let manifest = ctx.manifest()?;
    let export_date = exports::find(&manifest, "posts")?
        .updated_at
        .get(..10)
        .unwrap_or("unknown")
        .to_string();
    let tags = ctx.store.load_tags()?;
    let stats = ctx.store.load_post_stats()?;
    let edges = ctx.store.load_edges(args.links.name())?;
    let ties = ctx.store.load_edges("edges")?;
    let embedding = ctx.store.load_embedding()?;
    let positions: Vec<[f32; 2]> = ctx.store.load("layout.positions")?;
    let region: Vec<u32> = ctx.store.load("territories.region")?;
    let continent: Vec<u32> = ctx.store.load("territories.continent")?;
    let communities: territories::Meta = ctx.store.load_json("territories")?;
    let text_meta: text::TextMeta = ctx.store.load_json("text.meta")?;
    let edges_meta: edges::EdgesMeta = ctx.store.load_json("edges.meta")?;
    let overrides: emit::Overrides = match std::fs::read_to_string(&args.overrides) {
        Ok(s) => serde_json::from_str(&s)
            .with_context(|| format!("parse {}", args.overrides.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => emit::Overrides::default(),
        Err(e) => return Err(e).with_context(|| format!("read {}", args.overrides.display())),
    };
    emit::write(
        &args.out_dir,
        &ctx.store,
        emit::EmitInputs {
            tags: &tags,
            stats: &stats,
            edges: &edges,
            ties: &ties,
            embedding: &embedding,
            positions: &positions,
            region: &region,
            continent: &continent,
            communities: &communities,
            territories_regions: ctx.store.load_json("territories.regions")?,
            territories_continents: ctx.store.load_json("territories.continents")?,
            overrides: &overrides,
            export_date: &export_date,
            store_shard_size: text_meta.shard_size,
            text_shard_bytes: args.text_shard_bytes,
            tile_cutoffs: args.tile_cutoffs.clone(),
            adj_shard_size: args.adj_shard_size,
            tail_neighbors: edges_meta.tail_neighbors,
            base_links: args.base_links,
            shard_neighbors: args.shard_neighbors,
            playground_floor: args.playground_floor,
        },
    )
}

// humantime keeps every unit it is given, so round first or a ci line carries microseconds
fn human(d: Duration) -> String {
    let rounded = if d.as_secs() > 0 {
        Duration::from_secs(d.as_secs())
    } else {
        Duration::from_millis(d.as_millis() as u64)
    };
    humantime::format_duration(rounded).to_string()
}

#[derive(Default)]
struct Timings(Vec<(&'static str, Duration)>);

impl Timings {
    // every stage runs inside a span named after it, so a ci log line says which stage wrote it
    fn stage<T>(&mut self, name: &'static str, run: impl FnOnce() -> Result<T>) -> Result<T> {
        let span = tracing::info_span!("stage", name);
        let _enter = span.enter();
        let started = Instant::now();
        debug!("started");
        let out = run();
        let took = started.elapsed();
        match &out {
            Ok(_) => info!("{name} completed in {}", human(took)),
            Err(e) => error!("{name} failed after {}: {e:#}", human(took)),
        }
        self.0.push((name, took));
        out
    }

    fn digest(&self) {
        let total: Duration = self.0.iter().map(|(_, d)| *d).sum();
        let width = self.0.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
        let mut lines = String::from("stage summary");
        for (name, took) in &self.0 {
            let share = took.as_secs_f64() / total.as_secs_f64().max(1e-9) * 100.0;
            lines.push_str(&format!(
                "\n  {name:<width$}  {:>8}  {share:4.0}%",
                human(*took)
            ));
        }
        lines.push_str(&format!("\n  {:<width$}  {:>8}", "total", human(total)));
        info!("{lines}");
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(true)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .init();
    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.cache_dir).context("create cache dir")?;
    std::fs::create_dir_all(&cli.work_dir).context("create work dir")?;
    let ctx = Ctx {
        cli_cache: cli.cache_dir.clone(),
        offline: cli.offline,
        store: Store::new(&cli.work_dir),
    };
    let mut t = Timings::default();
    let out = match &cli.cmd {
        Cmd::Fetch => t.stage("fetch", || fetch(&ctx)),
        Cmd::Count(a) => t.stage("count", || count(&ctx, a)),
        Cmd::Edges(a) => t.stage("edges", || edges(&ctx, a)),
        Cmd::Embed(a) => t.stage("embed", || embed(&ctx, a)),
        Cmd::Communities(a) => t.stage("communities", || communities(&ctx, a)),
        Cmd::Layout(a) => t.stage("layout", || layout(&ctx, a)),
        Cmd::Territories(a) => t.stage("territories", || territories(&ctx, a)),
        Cmd::Text(a) => t.stage("text", || text(&ctx, a)),
        Cmd::Emit(a) => t.stage("emit", || emit(&ctx, a)),
        Cmd::All(a) => (|| {
            t.stage("fetch", || fetch(&ctx))?;
            t.stage("count", || count(&ctx, &a.count))?;
            t.stage("edges", || edges(&ctx, &a.edges))?;
            t.stage("embed", || embed(&ctx, &a.embed))?;
            t.stage("communities", || communities(&ctx, &a.communities))?;
            t.stage("layout", || layout(&ctx, &a.layout))?;
            t.stage("territories", || territories(&ctx, &a.territories))?;
            t.stage("text", || text(&ctx, &a.text))?;
            t.stage("emit", || emit(&ctx, &a.emit))
        })(),
    };
    t.digest();
    out
}

