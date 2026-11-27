// Dependency-free RSS 2.0, Atom 1.0, and RSS 1.0/RDF XML parser plus HTML feed autodiscovery.
// Keeps feed polling well under Cloudflare's 10 ms CPU limit.

import { canonicalUrl } from "./fetch";

export interface ParsedFeedItem {
  guid: string;
  link: string | null;
  title: string;
  author: string | null;
  published: string | null;
  contentHtml: string | null;
  summaryHtml: string | null;
}

export interface ParsedFeed {
  title: string;
  siteUrl: string | null;
  items: ParsedFeedItem[];
}

const NAMED_ENTITIES = new Map([
  ["amp", "&"],
  ["lt", "<"],
  ["gt", ">"],
  ["quot", '"'],
  ["apos", "'"],
  ["nbsp", " "],
]);

function decodeEntities(raw: string): string {
  return raw.replaceAll(/&(#x[0-9a-f]+|#[0-9]+|[a-z]+);/giu, (full, entity: string) => {
    const lower = entity.toLowerCase();

    if (lower.startsWith("#x")) {
      const code = Number.parseInt(lower.slice(2), 16);

      return Number.isFinite(code) && code > 0 && code <= 0x10ffff ? String.fromCodePoint(code) : full;
    }

    if (lower.startsWith("#")) {
      const code = Math.trunc(Number(lower.slice(1)));

      return Number.isFinite(code) && code > 0 && code <= 0x10ffff ? String.fromCodePoint(code) : full;
    }

    return NAMED_ENTITIES.get(lower) ?? full;
  });
}

function unwrapValue(raw: string): string {
  const trimmed = raw.trim();

  if (trimmed.includes("<![CDATA[")) {
    return trimmed.replaceAll(/<!\[CDATA\[([\s\S]*?)\]\]>/gu, "$1").trim();
  }

  return decodeEntities(trimmed).trim();
}

export function stripTags(html: string): string {
  return decodeEntities(html.replaceAll(/<[^>]+>/gu, " "))
    .replaceAll(/\s+/gu, " ")
    .trim();
}

function firstTag(xml: string, names: string[]): string | null {
  for (const name of names) {
    const pattern = new RegExp(`<${name}(?:\\s[^>]*)?>([\\s\\S]*?)</${name}>`, "iu");
    const match = xml.match(pattern);

    if (match?.[1] !== undefined) {
      const value = unwrapValue(match[1]);

      if (value) return value;
    }
  }

  return null;
}

function attr(tag: string, name: string): string | null {
  const match = tag.match(new RegExp(`\\b${name}\\s*=\\s*(?:"([^"]*)"|'([^']*)'|([^\\s"'=<>\\x60]+))`, "iu"));
  const value = match?.[1] ?? match?.[2] ?? match?.[3];

  return value ? decodeEntities(value).trim() : null;
}

function resolveOptionalUrl(raw: string | null, base: string): string | null {
  if (!raw) return null;

  try {
    const resolved = new URL(raw.trim(), base);

    if (resolved.protocol !== "http:" && resolved.protocol !== "https:") return null;

    return canonicalUrl(resolved.toString());
  } catch {
    return null;
  }
}

function normalizeDate(raw: string | null): string | null {
  if (!raw) return null;
  const parsed = Date.parse(raw);

  if (Number.isNaN(parsed)) return raw;

  return new Date(parsed).toISOString().replace(/\.\d{3}Z$/u, "Z");
}

function atomLink(fragment: string, base: string): string | null {
  const tags = fragment.match(/<link\b[^>]*>/giu) ?? [];
  let fallback: string | null = null;

  for (const tag of tags) {
    const href = attr(tag, "href");

    if (!href) continue;
    const rel = (attr(tag, "rel") ?? "alternate").toLowerCase();

    if (rel === "alternate") return resolveOptionalUrl(href, base);

    if (!fallback && rel !== "self" && rel !== "hub") fallback = resolveOptionalUrl(href, base);
  }

  return fallback;
}

/** Parses an RSS 2.0, Atom 1.0, or RSS 1.0/RDF XML document, or returns null if it isn't a feed. */
export function parseFeed(xml: string, feedUrl: string): ParsedFeed | null {
  const trimmed = xml.trim();
  const isAtom = /<feed(?:\s[^>]*)?>/iu.test(trimmed);
  const isRss = /<rss(?:\s[^>]*)?>/iu.test(trimmed) || /<rdf:RDF(?:\s[^>]*)?>/iu.test(trimmed) || /<channel(?:\s[^>]*)?>/iu.test(trimmed);

  if (!isAtom && !isRss) return null;

  if (isAtom) {
    const firstEntry = trimmed.search(/<entry(?:\s[^>]*)?>/iu);
    const header = firstEntry === -1 ? trimmed : trimmed.slice(0, firstEntry);
    const title = stripTags(firstTag(header, ["title"]) ?? "") || new URL(feedUrl).hostname;
    const siteUrl = atomLink(header, feedUrl);
    const base = siteUrl ?? feedUrl;
    const items: ParsedFeedItem[] = [];

    for (const match of trimmed.matchAll(/<entry(?:\s[^>]*)?>([\s\S]*?)<\/entry>/giu)) {
      const body = match[1] ?? "";
      const link = atomLink(body, base);
      const id = firstTag(body, ["id"]);
      const itemTitle = stripTags(firstTag(body, ["title"]) ?? "") || link || "Untitled";
      const authorBlock = firstTag(body, ["author"]);
      const author = authorBlock ? stripTags(firstTag(authorBlock, ["name"]) ?? authorBlock) || null : null;
      const published = normalizeDate(firstTag(body, ["published", "updated"]));
      const contentHtml = firstTag(body, ["content"]);
      const summaryHtml = firstTag(body, ["summary"]);
      const guid = id ?? link ?? `${itemTitle}|${published ?? ""}`;
      items.push({ guid, link, title: itemTitle, author, published, contentHtml, summaryHtml });
    }

    return { title, siteUrl, items };
  }

  const firstItem = trimmed.search(/<item(?:\s[^>]*)?>/iu);
  const header = firstItem === -1 ? trimmed : trimmed.slice(0, firstItem);
  const title = stripTags(firstTag(header, ["title"]) ?? "") || new URL(feedUrl).hostname;
  const siteUrl = resolveOptionalUrl(firstTag(header, ["link"]), feedUrl);
  const base = siteUrl ?? feedUrl;
  const items: ParsedFeedItem[] = [];

  for (const match of trimmed.matchAll(/<item(?:\s[^>]*)?>([\s\S]*?)<\/item>/giu)) {
    const body = match[1] ?? "";
    const link = resolveOptionalUrl(firstTag(body, ["link"]), base);
    const guid = firstTag(body, ["guid"]) ?? link;
    const itemTitle = stripTags(firstTag(body, ["title"]) ?? "") || link || "Untitled";
    const author = stripTags(firstTag(body, ["dc:creator", "author"]) ?? "") || null;
    const published = normalizeDate(firstTag(body, ["pubDate", "dc:date", "published", "updated"]));
    const contentHtml = firstTag(body, ["content:encoded", "content"]);
    const summaryHtml = firstTag(body, ["description", "summary"]);
    items.push({ guid: guid ?? `${itemTitle}|${published ?? ""}`, link, title: itemTitle, author, published, contentHtml, summaryHtml });
  }

  return { title, siteUrl, items };
}

/** Finds `<link rel="alternate" type="application/rss+xml|application/atom+xml" href="...">` in HTML. */
export function discoverFeedUrl(html: string, pageUrl: string): string | null {
  for (const match of html.matchAll(/<link\b[^>]*>/giu)) {
    const tag = match[0];
    const rel = (attr(tag, "rel") ?? "").toLowerCase().split(/\s+/u);

    if (!rel.includes("alternate")) continue;
    const type = (attr(tag, "type") ?? "").toLowerCase();

    if (type !== "application/rss+xml" && type !== "application/atom+xml") continue;
    const href = resolveOptionalUrl(attr(tag, "href"), pageUrl);

    if (href) return href;
  }

  return null;
}
