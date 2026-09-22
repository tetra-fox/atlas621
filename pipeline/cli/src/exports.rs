use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use flate2::bufread::MultiGzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{debug, info};

const MANIFEST_URL: &str = "https://e621.net/db_exports.json";
const USER_AGENT: &str = "atlas621/0.1 (https://github.com/tetra-fox/atlas621)";

pub const NEEDED: &[&str] = &[
    "tags",
    "tag_aliases",
    "tag_implications",
    "wiki_pages",
    "bulk_update_requests",
    "posts",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Export {
    pub name: String,
    pub file_name: String,
    pub file_size: u64,
    pub checksum: String,
    pub updated_at: String,
    pub url: String,
}

pub fn load_manifest(cache: &Path, offline: bool) -> Result<Vec<Export>> {
    let cached = cache.join("db_exports.json");
    let text = if offline {
        fs::read_to_string(&cached)
            .with_context(|| format!("offline, but {} is missing", cached.display()))?
    } else {
        let mut resp = ureq::get(MANIFEST_URL)
            .header("user-agent", USER_AGENT)
            .call()
            .context("fetch export manifest")?;
        let text = resp
            .body_mut()
            .read_to_string()
            .context("read export manifest")?;
        fs::write(&cached, &text).context("cache export manifest")?;
        text
    };
    serde_json::from_str(&text).context("parse export manifest")
}

pub fn find<'a>(manifest: &'a [Export], name: &str) -> Result<&'a Export> {
    manifest
        .iter()
        .find(|e| e.name == name)
        .with_context(|| format!("export {name} not in manifest"))
}

// the bool says the file came over the network rather than out of the cache
pub fn ensure(export: &Export, cache: &Path) -> Result<(PathBuf, bool)> {
    let path = cache.join(&export.file_name);
    if let Ok(meta) = fs::metadata(&path)
        && meta.len() == export.file_size
    {
        debug!(
            file = %export.file_name,
            mb = export.file_size / 1_000_000,
            "cached"
        );
        return Ok((path, false));
    }
    info!(
        "downloading {} ({} MB)",
        export.file_name,
        export.file_size / 1_000_000
    );
    let part = path.with_extension("csv.gz.part");
    {
        let mut resp = ureq::get(&export.url)
            .header("user-agent", USER_AGENT)
            .call()
            .with_context(|| format!("fetch {}", export.url))?;
        let mut reader = resp.body_mut().as_reader();
        let mut out = io::BufWriter::new(File::create(&part)?);
        io::copy(&mut reader, &mut out)?;
        out.flush()?;
    }
    let digest = sha256_hex(&part)?;
    if digest != export.checksum {
        bail!(
            "checksum mismatch for {}: got {digest}, manifest says {}",
            export.file_name,
            export.checksum
        );
    }
    fs::rename(&part, &path)?;
    Ok((path, true))
}

fn sha256_hex(path: &Path) -> Result<String> {
    let mut file = BufReader::with_capacity(1 << 20, File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn column(headers: &csv::StringRecord, file: &str, name: &str) -> Result<usize> {
    headers
        .iter()
        .position(|h| h == name)
        .with_context(|| format!("{file} has no {name} column"))
}

pub fn open(path: &Path) -> Result<Box<dyn Read + Send>> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    Ok(Box::new(MultiGzDecoder::new(BufReader::with_capacity(
        1 << 20,
        file,
    ))))
}
