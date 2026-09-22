import { loadSearchShard, shardKey, type SearchEntry } from "./dataset";

export type Token = { text: string; sign: 1 | -1; or: boolean };

export type Term = { sign: 1 | -1; tags: SearchEntry[]; source: string };

export type Resolved = { terms: Term[]; unknown: string[]; ignored: string[] };

const METATAGS = new Set([
  "rating",
  "score",
  "favcount",
  "order",
  "limit",
  "user",
  "fav",
  "id",
  "date",
  "width",
  "height",
  "ratio",
  "filesize",
  "type",
  "status",
  "source",
  "pool",
  "set",
  "md5",
  "tagcount",
  "gentags",
  "arttags",
  "chartags",
  "copytags",
  "spectags",
  "loretags",
  "metatags",
  "inpool",
  "approver",
  "commenter",
  "noter",
  "voted",
  "votedup",
  "voteddown",
  "duration",
  "parent",
  "child",
  "hassource",
  "hasdescription",
  "ischild",
  "isparent",
  "description",
  "note",
  "delreason",
  "deletedby",
  "comment",
  "randseed",
  "random"
]);

export const parseQuery = (text: string): Token[] =>
  text
    .toLowerCase()
    .split(/\s+/)
    .filter((t) => t.length > 0)
    .map((raw) => {
      let text = raw;
      let sign: 1 | -1 = 1;
      let or = false;
      for (;;) {
        if (text.startsWith("-")) sign = -1;
        else if (text.startsWith("~")) or = true;
        else break;
        text = text.slice(1);
      }
      return { text, sign, or };
    })
    .filter((t) => t.text.length > 0);

const isMetatag = (text: string) => {
  const colon = text.indexOf(":");
  return colon > 0 && METATAGS.has(text.slice(0, colon));
};

const globToRegex = (glob: string) =>
  new RegExp(
    `^${glob
      .split("*")
      .map((s) => s.replace(/[.+?^${}()|[\]\\]/g, "\\$&"))
      .join(".*")}$`
  );

const WILDCARD_LIMIT = 40;

const lookup = async (text: string): Promise<SearchEntry[]> => {
  const star = text.indexOf("*");
  const prefix = star < 0 ? text : text.slice(0, star);
  if (prefix.length < 2) return [];
  const entries = await loadSearchShard(shardKey(prefix));
  if (star < 0) {
    const hit = entries.find((e) => e.name === text);
    return hit && hit.node >= 0 ? [hit] : [];
  }
  const pattern = globToRegex(text);
  return entries
    .filter((e) => e.node >= 0 && e.alias === undefined && pattern.test(e.name))
    .sort((a, b) => b.postCount - a.postCount)
    .slice(0, WILDCARD_LIMIT);
};

export const resolveQuery = async (text: string): Promise<Resolved> => {
  const tokens = parseQuery(text);
  const terms: Term[] = [];
  const unknown: string[] = [];
  const ignored: string[] = [];
  const any: Term = { sign: 1, tags: [], source: "" };
  for (const token of tokens) {
    if (isMetatag(token.text)) {
      ignored.push(token.text);
      continue;
    }
    const tags = await lookup(token.text);
    if (tags.length === 0) {
      unknown.push(token.text);
      continue;
    }
    if (token.or) {
      any.tags.push(...tags);
      any.source += (any.source ? " ~" : "~") + token.text;
    } else {
      terms.push({ sign: token.sign, tags, source: token.text });
    }
  }
  if (any.tags.length > 0) terms.push(any);
  return { terms, unknown, ignored };
};
