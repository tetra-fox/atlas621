# atlas621

t-sne map of e621's tags

## layout

```text
pipeline/core/   atlas-core, a library: sparse graphs, ppmi + randomized svd, t-sne,
                 forceatlas2, leiden, barnes-hut quadtree, hex grids. no e621 in it
pipeline/cli/    atlas621, the binary: reads the e621 db exports, runs the stages,
                 writes the dataset
src/lib/core/    generic browser code: arrow tables, label decluttering, a glyph atlas,
                 an fnv string table, hex geometry. no atlas621 in it
src/lib/data/    loading and typing the dataset
src/lib/render/  canvas and webgl drawing
src/lib/map/     the map itself, its components, workers and state
```

## building the dataset

`atlas621` is a 9 stage pipeline. each stage reads what the last one left in `--work-dir`, so they run in order and can be run one at a time while you are changing one:

```sh
# first, build the pipeline executable
cargo build --release --manifest-path pipeline/Cargo.toml

# optionally, add it to your terminal session's path
export PATH="$PWD/pipeline/target/release:$PATH"

# then build the dataset
atlas621 fetch        # download the e621 db exports into the cache
atlas621 count        # scan the posts export for tag pairs and per-year totals
atlas621 edges        # keep each tag's strongest pairings as the graph edges
atlas621 embed        # factor the pair counts into vectors, then the knn and affinity graphs
atlas621 communities  # group tags into regions with leiden
atlas621 layout       # place every tag on the plane
atlas621 territories  # cut the layout into hex regions and continents, then name them
atlas621 text         # build the wiki, alias and history shards and the search index
atlas621 emit         # write the dataset the site loads
# or just
atlas621 all
```

the process downloads about 10 gb of e621 db exports into `~/.cache/atlas621/exports`, chews through every post, and writes the site's dataset into `static/data`. it takes a while and wants a lot of ram.

useful flags:

- `--offline` works from the cached exports without hitting the network
- `--previous <url or dir>` seeds the layout from an existing dataset, so a rebuild keeps tags roughly where they already were instead of shuffling the whole map
- `--work-dir` is where the intermediates go, `pipeline/work` by default

## running the site

```sh
pnpm install
pnpm dev
```

the site reads `static/data`, so build a dataset first or copy one from a deploy.

## checks

```sh
pnpm lint     # oxfmt + oxlint
pnpm check    # svelte-check
pnpm test     # vitest
pnpm build

cargo fmt --all --check --manifest-path pipeline/Cargo.toml
cargo clippy --manifest-path pipeline/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path pipeline/Cargo.toml
```
