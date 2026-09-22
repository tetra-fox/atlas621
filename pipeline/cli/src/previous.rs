use anyhow::{Context, Result};
use log::info;
use rustc_hash::FxHashMap;

const ARROW_MAGIC: &[u8] = b"ARROW1";

use crate::tags::Tags;

pub fn seed_positions(tags: &Tags, previous: &str) -> Result<Vec<[f32; 2]>> {
    let fetch = |file: &str| -> Result<Vec<u8>> {
        if previous.starts_with("http://") || previous.starts_with("https://") {
            let url = format!("{}/{file}", previous.trim_end_matches('/'));
            let mut resp = ureq::get(&url)
                .call()
                .map_err(|e| anyhow::anyhow!("fetch {url}: {e}"))?;
            Ok(resp.body_mut().read_to_vec()?)
        } else {
            Ok(std::fs::read(std::path::Path::new(previous).join(file))?)
        }
    };
    let gunzip = |bytes: Vec<u8>| -> Result<Vec<u8>> {
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::MultiGzDecoder::new(&bytes[..]), &mut out)?;
        Ok(out)
    };
    let manifest: serde_json::Value = serde_json::from_slice(&fetch("manifest.json")?)?;
    let n_old = manifest["nodes"]
        .as_u64()
        .context("previous manifest has no node count")? as usize;
    let position_blob = gunzip(fetch("positions.bin.gz")?)?;
    let names_blob = gunzip(fetch("names.bin.gz")?)?;
    // TODO remove the raw branch once a rebuild has published arrow, which is the only reason a
    // deploy older than that format can still be the previous one
    let (positions, names) = if position_blob.starts_with(ARROW_MAGIC) {
        (
            atlas_core::ipc::read_pairs(&position_blob, "position")?,
            atlas_core::ipc::read_strings(&names_blob, "name")?,
        )
    } else {
        let positions = atlas_core::bin::read_all(&mut &position_blob[..])?;
        let offsets: Vec<u32> = atlas_core::bin::read_all(&mut &names_blob[..(n_old + 1) * 4])?;
        let text = std::str::from_utf8(&names_blob[(n_old + 1) * 4..])?;
        let names = (0..n_old)
            .map(|i| text[offsets[i] as usize..offsets[i + 1] as usize].to_string())
            .collect();
        (positions, names)
    };
    let by_name: FxHashMap<&str, [f32; 2]> = names
        .iter()
        .map(String::as_str)
        .zip(positions.iter().copied())
        .collect();
    let mut found = 0;
    let seeded = tags.names[..tags.n_nodes]
        .iter()
        .map(|n| match by_name.get(n.as_str()) {
            Some(p) => {
                found += 1;
                *p
            }
            None => [f32::NAN; 2],
        })
        .collect();
    info!(
        "warm start from {previous}: {found} of {} nodes have a previous position",
        tags.n_nodes
    );
    Ok(seeded)
}
