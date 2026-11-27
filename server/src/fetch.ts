// Safe public URL fetching and lightweight HTML wrapping for pages, feeds, and newsletters
// before syncing devices parse them with `inkling-books`.

import { MAX_ARTICLE_BYTES, Refused, escapeHtml } from "./http";

const BROWSER_AGENT =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";

const PRIVATE_IPV4 =
  /^(127\.|10\.|0\.|169\.254\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.|100\.(6[4-9]|[7-9]\d|1[01]\d|12[0-7])\.)/u;

/** Removes any `#fragment`, matching `books/src/article.rs`'s canonical URL rule. */
export function canonicalUrl(raw: string): string {
  const trimmed = raw.trim();
  const hash = trimmed.indexOf("#");

  return hash === -1 ? trimmed : trimmed.slice(0, hash);
}

/** Refuses non-http(s) URLs and loopback, link-local, or private network hosts. */
export function requirePublicUrl(raw: string): URL {
  let parsed: URL;

  try {
    parsed = new URL(canonicalUrl(raw));
  } catch {
    throw new Refused(400, "That isn’t a full web address.");
  }

  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    throw new Refused(400, "Only http and https addresses can be saved.");
  }

  const host = parsed.hostname.toLowerCase().replaceAll(/^\[|\]$/gu, "");

  if (
    !host ||
    host === "localhost" ||
    host.endsWith(".localhost") ||
    host.endsWith(".local") ||
    host.endsWith(".internal") ||
    PRIVATE_IPV4.test(host) ||
    host === "::1" ||
    host === "::" ||
    host.startsWith("fc") ||
    host.startsWith("fd") ||
    host.startsWith("fe80:")
  ) {
    throw new Refused(400, "Private or local addresses can’t be fetched by the server.");
  }

  return parsed;
}

export interface FetchedText {
  url: string;
  status: number;
  contentType: string;
  etag: string | null;
  lastModified: string | null;
  text: string;
  bytes: number;
}

/**
 * Fetches a public http(s) URL, following up to five redirects and checking each hop against
 * `requirePublicUrl`. Caps the body at `maxBytes`.
 */
export async function fetchPublicText(
  rawUrl: string,
  options: { accept: string; etag?: string | null; lastModified?: string | null; maxBytes?: number },
): Promise<FetchedText> {
  const maxBytes = options.maxBytes ?? MAX_ARTICLE_BYTES;
  let current = requirePublicUrl(rawUrl);

  for (let hop = 0; hop < 6; hop++) {
    const headers = new Headers({ "user-agent": BROWSER_AGENT, accept: options.accept });

    if (hop === 0 && options.etag) headers.set("if-none-match", options.etag);

    if (hop === 0 && options.lastModified) headers.set("if-modified-since", options.lastModified);

    const response = await fetch(current.toString(), { headers, redirect: "manual" });

    if (response.status >= 300 && response.status < 400) {
      const location = response.headers.get("location");

      if (!location) throw new Refused(502, `The site redirected without a target (${response.status}).`);
      current = requirePublicUrl(new URL(location, current).toString());
      await response.body?.cancel();
      continue;
    }

    const etag = response.headers.get("etag");
    const lastModified = response.headers.get("last-modified");
    const contentType = (response.headers.get("content-type") ?? "").toLowerCase();

    if (response.status === 304) {
      await response.body?.cancel();

      return { url: canonicalUrl(current.toString()), status: 304, contentType, etag, lastModified, text: "", bytes: 0 };
    }

    const declared = Number(response.headers.get("content-length") ?? NaN);

    if (declared > maxBytes) {
      await response.body?.cancel();
      throw new Refused(413, "That page is larger than 5 MB, which is as much as the server fetches.");
    }

    const buffer = await response.arrayBuffer();

    if (buffer.byteLength > maxBytes) {
      throw new Refused(413, "That page is larger than 5 MB, which is as much as the server fetches.");
    }

    return {
      url: canonicalUrl(current.toString()),
      status: response.status,
      contentType,
      etag,
      lastModified,
      text: new TextDecoder().decode(buffer),
      bytes: buffer.byteLength,
    };
  }

  throw new Refused(502, "That address redirected too many times.");
}

export interface ArticleMeta {
  title?: string | null;
  author?: string | null;
  site?: string | null;
  published?: string | null;
  excerpt?: string | null;
}

/**
 * Ensures an HTML fragment or email body carries `<head>` metadata (`<title>`, `og:title`,
 * `author`, `og:site_name`, `article:published_time`, `description`) so `inkling-books` keeps
 * the feed or newsletter's own metadata when it parses the article.
 */
export function prepareArticleHtml(rawHtml: string, meta: ArticleMeta, options: { overrideHead?: boolean } = {}): string {
  const trimmed = rawHtml.trim();
  const hasHtml = /<html[\s>]/iu.test(trimmed);
  const override = options.overrideHead === true;
  const needs = (pattern: RegExp) => !hasHtml || override || !pattern.test(trimmed);
  const tags: string[] = [];

  if (meta.title) {
    const escaped = escapeHtml(meta.title);

    if (needs(/<title[\s>]/iu)) tags.push(`<title>${escaped}</title>`);

    if (needs(/property\s*=\s*["']og:title["']/iu)) tags.push(`<meta property="og:title" content="${escaped}">`);
  }

  if (meta.author && needs(/name\s*=\s*["']author["']/iu)) {
    tags.push(`<meta name="author" content="${escapeHtml(meta.author)}">`);
  }

  if (meta.site && needs(/property\s*=\s*["']og:site_name["']/iu)) {
    tags.push(`<meta property="og:site_name" content="${escapeHtml(meta.site)}">`);
  }

  if (meta.published && needs(/property\s*=\s*["']article:published_time["']/iu)) {
    tags.push(`<meta property="article:published_time" content="${escapeHtml(meta.published)}">`);
  }

  if (meta.excerpt && needs(/name\s*=\s*["']description["']/iu)) {
    tags.push(`<meta name="description" content="${escapeHtml(meta.excerpt)}">`);
  }

  if (hasHtml) {
    if (tags.length === 0) return trimmed;
    const head = trimmed.match(/<head[\s>][^>]*>/iu);

    if (head && head.index !== undefined) {
      const at = head.index + head[0].length;

      return `${trimmed.slice(0, at)}\n${tags.join("\n")}\n${trimmed.slice(at)}`;
    }

    return trimmed.replace(/<html[\s>][^>]*>/iu, (open) => `${open}\n<head>\n<meta charset="utf-8">\n${tags.join("\n")}\n</head>`);
  }

  const heading = meta.title ? `<h1>${escapeHtml(meta.title)}</h1>\n` : "";

  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
${tags.join("\n")}
</head>
<body>
<article>
${heading}${trimmed}
</article>
</body>
</html>`;
}

/** Turns a plain-text email or feed body into paragraphs wrapped for `inkling-books`. */
export function plainTextToHtml(text: string, meta: ArticleMeta): string {
  const paragraphs = text
    .replaceAll("\r\n", "\n")
    .split(/\n{2,}/u)
    .flatMap((block) => {
      const cleaned = block.trim();

      if (!cleaned) return [];

      return [`<p>${escapeHtml(cleaned).replaceAll("\n", "<br>\n")}</p>`];
    })
    .join("\n");

  return prepareArticleHtml(paragraphs, meta);
}
