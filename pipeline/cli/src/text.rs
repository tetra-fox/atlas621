use ts_rs::TS;
use std::collections::BTreeMap;
use std::io::Read;
use std::time::Instant;

use anyhow::Result;
use jiff::civil;
use tracing::{debug, info, warn};
use regex::Regex;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

use crate::exports;
use crate::posts::YEAR0;
use crate::tags::Tags;

pub struct TextParams {
    pub lump_threshold: usize,
    pub excerpt_chars: usize,
    pub shard_size: usize,
}

#[derive(Serialize, Deserialize)]
pub struct TextMeta {
    pub shard_size: usize,
}

#[derive(Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/lib/data/generated.ts")]
pub struct Bur {
    pub id: u32,
    pub date: String,
    pub title: String,
    pub ops: Vec<[String; 3]>,
}

#[derive(Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/lib/data/generated.ts")]
pub struct NodeImplication(pub u32, pub u32, pub Option<u16>, pub bool);

#[derive(Serialize, Deserialize, Default, TS)]
#[ts(export, export_to = "../../../src/lib/data/generated.ts")]
pub struct Event {
    pub date: String,
    pub kind: String,
    // the tag on the other side of the alias or implication
    pub other: String,
    pub source: String,
    // the date came from a migration day rather than the change itself
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub approx: bool,
}

#[derive(Serialize, Deserialize, Default, TS)]
#[ts(export, export_to = "../../../src/lib/data/generated.ts")]
pub struct NodeText {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wiki: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<Event>,
    // the emit stage fills the rest in from the dataset rather than the text exports
    #[serde(default)]
    pub ratings: [u32; 3],
    #[serde(default)]
    pub region: u32,
    // flat triples of node, quantised weight, shared post count
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<u32>,
    // flat pairs of node, quantised similarity
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub similar: Vec<u32>,
}

pub struct TextOutput {
    pub aliases: Vec<(String, u32)>,
    pub implications: Vec<NodeImplication>,
    pub changelog: Vec<Bur>,
    pub shards: BTreeMap<usize, BTreeMap<u32, NodeText>>,
}

pub fn days_since_2007(date: &str) -> Option<u16> {
    let day: civil::Date = date.get(..10)?.parse().ok()?;
    let since = day.since(civil::date(YEAR0 as i16, 1, 1)).ok()?;
    u16::try_from(since.get_days()).ok()
}

fn text_for(
    shards: &mut BTreeMap<usize, BTreeMap<u32, NodeText>>,
    shard_size: usize,
    idx: u32,
) -> &mut NodeText {
    shards
        .entry(idx as usize / shard_size)
        .or_default()
        .entry(idx)
        .or_default()
}

fn date_part(ts: &str) -> &str {
    ts.get(..10).unwrap_or("")
}

struct Dtext {
    link: Regex,
    named_link: Regex,
    tag: Regex,
    heading: Regex,
    thumb: Regex,
    search: Regex,
}

impl Dtext {
    fn new() -> Dtext {
        Dtext {
            link: Regex::new(r"\[\[([^\]|]+)(?:\|([^\]]*))?\]\]").unwrap(),
            named_link: Regex::new(r#""([^"]+)":(?:https?://\S+|/\S+)"#).unwrap(),
            tag: Regex::new(r"\[/?[a-zA-Z]+(?:[=,][^\]]*)?\]").unwrap(),
            heading: Regex::new(r"(?m)^h[1-6]\.\s*").unwrap(),
            thumb: Regex::new(r"thumb #\d+").unwrap(),
            search: Regex::new(r"\{\{([^}]+)\}\}").unwrap(),
        }
    }

    fn links(&self, body: &str) -> Vec<String> {
        self.link
            .captures_iter(body)
            .map(|c| c[1].trim().to_lowercase().replace(' ', "_"))
            .collect()
    }

    fn excerpt(&self, body: &str, max_chars: usize) -> Option<String> {
        for para in body.split("\r\n\r\n").flat_map(|p| p.split("\n\n")) {
            let text = self.link.replace_all(para, |c: &regex::Captures| {
                c.get(2)
                    .map_or_else(|| c[1].to_string(), |m| m.as_str().to_string())
            });
            let text = self.named_link.replace_all(&text, "$1");
            let text = self.search.replace_all(&text, "$1");
            let text = self.tag.replace_all(&text, "");
            let text = self.heading.replace_all(&text, "");
            let text = self.thumb.replace_all(&text, "");
            let text = text
                .lines()
                .map(|l| l.trim_start_matches(['*', '#', ' ']).trim())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if text.chars().count() < 8 {
                continue;
            }
            if text.chars().count() <= max_chars {
                return Some(text);
            }
            let mut cut: String = text.chars().take(max_chars).collect();
            if let Some(space) = cut.rfind(' ') {
                cut.truncate(space);
            }
            cut.push_str("...");
            return Some(cut);
        }
        None
    }
}

fn parse_op(line: &str) -> Option<[String; 3]> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let lower = line.to_lowercase();
    let (kind, rest) = [
        ("create alias ", "alias"),
        ("create implication ", "implicate"),
        ("remove alias ", "unalias"),
        ("remove implication ", "unimplicate"),
        ("change category ", "category"),
        ("mass update ", "update"),
        ("alias ", "alias"),
        ("implicate ", "implicate"),
        ("imply ", "implicate"),
        ("unalias ", "unalias"),
        ("unimplicate ", "unimplicate"),
        ("unimply ", "unimplicate"),
        ("category ", "category"),
        ("update ", "update"),
        ("rename ", "rename"),
        ("nuke ", "nuke"),
        ("deprecate ", "deprecate"),
        ("undeprecate ", "undeprecate"),
    ]
    .iter()
    .find_map(|(prefix, kind)| lower.strip_prefix(prefix).map(|r| (*kind, r)))?;
    let mut parts = rest.split("->").map(str::trim);
    let a = parts.next()?.split_whitespace().next()?.to_string();
    let b = parts
        .next()
        .and_then(|s| s.split_whitespace().next())
        .unwrap_or("")
        .to_string();
    Some([kind.to_string(), a, b])
}

pub fn build(
    tags: &Tags,
    aliases_csv: Box<dyn Read + Send>,
    implications_csv: Box<dyn Read + Send>,
    wiki_csv: Box<dyn Read + Send>,
    burs_csv: Box<dyn Read + Send>,
    params: &TextParams,
) -> Result<TextOutput> {
    let t0 = Instant::now();
    debug!(
        lump_threshold = params.lump_threshold,
        excerpt_chars = params.excerpt_chars,
        shard_size = params.shard_size,
        "building text shards"
    );
    let n_nodes = tags.n_nodes;
    let mut shards: BTreeMap<usize, BTreeMap<u32, NodeText>> = BTreeMap::new();
    let shard_size = params.shard_size;

    let mut changelog: Vec<Bur> = Vec::new();
    let mut op_date: FxHashMap<(String, String, String), (String, u32)> = FxHashMap::default();
    let mut unknown_lines = 0usize;
    {
        let mut rdr = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(burs_csv);
        let headers = rdr.headers()?.clone();
        let col = |name| exports::column(&headers, "bulk_update_requests.csv", name);
        let (c_id, c_script, c_status, c_title, c_created, c_updated) = (
            col("id")?,
            col("script")?,
            col("status")?,
            col("title")?,
            col("created_at")?,
            col("updated_at")?,
        );
        for rec in rdr.records() {
            let rec = rec?;
            if &rec[c_status] != "approved" {
                continue;
            }
            let id: u32 = rec[c_id].parse()?;
            let date = date_part(if rec[c_updated].is_empty() {
                &rec[c_created]
            } else {
                &rec[c_updated]
            })
            .to_string();
            let mut ops = Vec::new();
            for line in rec[c_script].lines() {
                match parse_op(line) {
                    Some(op) => {
                        let key = (op[0].clone(), op[1].clone(), op[2].clone());
                        let e = op_date.entry(key).or_insert((date.clone(), id));
                        if date < e.0 {
                            *e = (date.clone(), id);
                        }
                        ops.push(op);
                    }
                    None if line.trim().is_empty() => {}
                    None => unknown_lines += 1,
                }
            }
            changelog.push(Bur {
                id,
                date,
                title: rec[c_title].to_string(),
                ops,
            });
        }
    }
    changelog.sort_by(|a, b| b.date.cmp(&a.date).then(b.id.cmp(&a.id)));
    info!(
        "{} approved bulk update requests, {} script lines not understood, {:.0?}",
        changelog.len(),
        unknown_lines,
        t0.elapsed()
    );

    for bur in &changelog {
        for op in &bur.ops {
            for (me, other) in [(&op[1], &op[2]), (&op[2], &op[1])] {
                let Some(idx) = tags.index_of(me) else {
                    continue;
                };
                if idx as usize >= n_nodes || (me == other) {
                    continue;
                }
                text_for(&mut shards, shard_size, idx).history.push(Event {
                    date: bur.date.clone(),
                    kind: op[0].clone(),
                    other: other.clone(),
                    source: format!("bur:{}", bur.id),
                    approx: false,
                });
            }
        }
    }

    let mut aliases: Vec<(String, u32)> = Vec::new();
    {
        let mut rdr = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(aliases_csv);
        let headers = rdr.headers()?.clone();
        let col = |name| exports::column(&headers, "tag_aliases.csv", name);
        let (c_from, c_to, c_status, c_created) = (
            col("antecedent_name")?,
            col("consequent_name")?,
            col("status")?,
            col("created_at")?,
        );
        for rec in rdr.records() {
            let rec = rec?;
            if &rec[c_status] != "active" {
                continue;
            }
            let Some(target) = tags.index_of(&rec[c_to]) else {
                continue;
            };
            aliases.push((rec[c_from].to_string(), target));
            if (target as usize) < n_nodes {
                let t = text_for(&mut shards, shard_size, target);
                t.aliases.push(rec[c_from].to_string());
                if !op_date.contains_key(&(
                    "alias".to_string(),
                    rec[c_from].to_string(),
                    rec[c_to].to_string(),
                )) && !rec[c_created].is_empty()
                {
                    t.history.push(Event {
                        date: date_part(&rec[c_created]).to_string(),
                        kind: "alias".into(),
                        other: rec[c_from].to_string(),
                        source: "created_at".into(),
                        approx: false,
                    });
                }
            }
        }
    }
    info!("{} active aliases resolve to a tag", aliases.len());

    let mut implications: Vec<NodeImplication> = Vec::new();
    {
        let mut rdr = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(implications_csv);
        let headers = rdr.headers()?.clone();
        let col = |name| exports::column(&headers, "tag_implications.csv", name);
        let (c_from, c_to, c_status, c_created) = (
            col("antecedent_name")?,
            col("consequent_name")?,
            col("status")?,
            col("created_at")?,
        );
        let mut rows: Vec<(String, String, String)> = Vec::new();
        for rec in rdr.records() {
            let rec = rec?;
            if &rec[c_status] != "active" {
                continue;
            }
            rows.push((
                rec[c_from].to_string(),
                rec[c_to].to_string(),
                date_part(&rec[c_created]).to_string(),
            ));
        }
        let mut per_day: FxHashMap<&str, usize> = FxHashMap::default();
        for (_, _, d) in &rows {
            if !d.is_empty() {
                *per_day.entry(d.as_str()).or_insert(0) += 1;
            }
        }
        // a day with this many implications is a migration that stamped its own date on old
        // rows, so those dates are reported as approximate
        let lumps: FxHashSet<&str> = per_day
            .iter()
            .filter(|(_, c)| **c > params.lump_threshold)
            .map(|(d, _)| *d)
            .collect();
        let mut dated_by_bur = 0;
        let mut approx_count = 0;
        for (child, parent, created) in &rows {
            let (ci, pi) = (tags.index_of(child), tags.index_of(parent));
            if let Some(ci) = ci
                && (ci as usize) < n_nodes
            {
                text_for(&mut shards, shard_size, ci).parents.push(parent.clone());
            }
            if let Some(pi) = pi
                && (pi as usize) < n_nodes
            {
                text_for(&mut shards, shard_size, pi).children.push(child.clone());
            }
            let bur = op_date.get(&("implicate".to_string(), child.clone(), parent.clone()));
            let (date, approx, source) = match bur {
                Some((d, id)) => {
                    dated_by_bur += 1;
                    (Some(d.clone()), false, format!("bur:{id}"))
                }
                None if !created.is_empty() => {
                    let approx = lumps.contains(created.as_str());
                    approx_count += approx as usize;
                    (Some(created.clone()), approx, "created_at".to_string())
                }
                None => (None, true, String::new()),
            };
            if bur.is_none()
                && let Some(d) = &date
            {
                if let Some(ci) = ci
                    && (ci as usize) < n_nodes
                {
                    text_for(&mut shards, shard_size, ci).history.push(Event {
                        date: d.clone(),
                        kind: "implicate".into(),
                        other: parent.clone(),
                        source: source.clone(),
                        approx,
                    });
                }
                if let Some(pi) = pi
                    && (pi as usize) < n_nodes
                {
                    text_for(&mut shards, shard_size, pi).history.push(Event {
                        date: d.clone(),
                        kind: "implicate".into(),
                        other: child.clone(),
                        source: source.clone(),
                        approx,
                    });
                }
            }
            if let (Some(ci), Some(pi)) = (ci, pi)
                && (ci as usize) < n_nodes
                && (pi as usize) < n_nodes
            {
                implications.push(NodeImplication(
                    ci,
                    pi,
                    date.as_deref().and_then(days_since_2007),
                    approx,
                ));
            }
        }
        info!(
            "{} active implications, {} between nodes, {dated_by_bur} dated by a request, {approx_count} on migration-looking days ({} such days), {:.0?}",
            rows.len(),
            implications.len(),
            lumps.len(),
            t0.elapsed()
        );
    }

    {
        let dtext = Dtext::new();
        let mut rdr = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(wiki_csv);
        let headers = rdr.headers()?.clone();
        let col = |name| exports::column(&headers, "wiki_pages.csv", name);
        let (c_title, c_body) = (col("title")?, col("body")?);
        let (mut pages, mut with_excerpt, mut links) = (0usize, 0usize, 0usize);
        for rec in rdr.records() {
            let rec = rec?;
            let Some(idx) = tags.index_of(&rec[c_title]) else {
                continue;
            };
            if idx as usize >= n_nodes {
                continue;
            }
            pages += 1;
            let body = &rec[c_body];
            let t = text_for(&mut shards, shard_size, idx);
            t.wiki = dtext.excerpt(body, params.excerpt_chars);
            with_excerpt += t.wiki.is_some() as usize;
            let mut seen = FxHashSet::default();
            for link in dtext.links(body) {
                if let Some(li) = tags.index_of(&link)
                    && (li as usize) < n_nodes
                    && li != idx
                    && seen.insert(li)
                {
                    t.links.push(li);
                }
            }
            links += t.links.len();
        }
        info!(
            "{pages} wiki pages belong to nodes, {with_excerpt} yield an excerpt, {links} see-also links, {:.0?}",
            t0.elapsed()
        );
    }

    for shard in shards.values_mut() {
        for t in shard.values_mut() {
            t.history.sort_by(|x, y| x.date.cmp(&y.date).then(x.kind.cmp(&y.kind)).then(x.other.cmp(&y.other)));
            t.history.dedup_by(|x, y| x.date == y.date && x.kind == y.kind && x.other == y.other);
        }
    }
    if unknown_lines > 2000 {
        warn!(
            "{unknown_lines} bulk update script lines were not understood; check parse_op against current e621 syntax"
        );
    }
    Ok(TextOutput {
        aliases,
        implications,
        changelog,
        shards,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_numbers() {
        assert_eq!(days_since_2007("2007-01-01"), Some(0));
        assert_eq!(days_since_2007("2007-03-01"), Some(59));
        assert_eq!(days_since_2007("2008-03-01"), Some(425));
        assert_eq!(days_since_2007("2020-03-12 08:53:27.845484"), Some(4819));
        assert_eq!(days_since_2007(""), None);
    }

    #[test]
    fn bulk_update_lines() {
        assert_eq!(
            parse_op("implicate fox -> canine"),
            Some(["implicate".into(), "fox".into(), "canine".into()])
        );
        assert_eq!(
            parse_op("create alias tits -> breasts"),
            Some(["alias".into(), "tits".into(), "breasts".into()])
        );
        assert_eq!(
            parse_op("unimplicate a -> b # reason"),
            Some(["unimplicate".into(), "a".into(), "b".into()])
        );
        assert_eq!(
            parse_op("category foo -> species"),
            Some(["category".into(), "foo".into(), "species".into()])
        );
        assert_eq!(
            parse_op("nuke bad_tag"),
            Some(["nuke".into(), "bad_tag".into(), "".into()])
        );
        assert_eq!(parse_op("# comment"), None);
        assert_eq!(parse_op("frobnicate x -> y"), None);
    }

    #[test]
    fn dtext_excerpt() {
        let d = Dtext::new();
        let body = "h4. Nav\n\n[Back: [[e621:index]]]\n\nA [[canid|canine]] that is \"red\":/wiki_pages/red and [b]bold[/b]. See {{fox solo}}.\n\nSecond paragraph.";
        assert_eq!(d.excerpt(body, 300).as_deref(), Some("[Back: e621:index]"));
        let body2 =
            "A [[canid|canine]] that is \"red\":/wiki_pages/red and [b]bold[/b]. See {{fox solo}}.";
        assert_eq!(
            d.excerpt(body2, 300).as_deref(),
            Some("A canine that is red and bold. See fox solo.")
        );
        assert_eq!(
            d.links("see [[Red Fox]] and [[canid|dogs]]"),
            vec!["red_fox", "canid"]
        );
        let long = "word ".repeat(100);
        let e = d.excerpt(&long, 40).unwrap();
        assert!(e.ends_with("...") && e.len() <= 44, "{e}");
    }
}
