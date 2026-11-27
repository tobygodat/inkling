// RSS and Atom feeds (TOB-554): follow feeds via `/v1/feeds`, fetch them on the 30-minute Worker
// cron with conditional GETs, and stage each new item in `incoming` (`destination = 'inbox'`)
// without duplicates so syncing devices parse it with `inkling-books`.

import { z } from "zod";
import type { Env } from "./env";
import { discoverFeedUrl, parseFeed, stripTags, type ParsedFeedItem } from "./feed-xml";
import { canonicalUrl, fetchPublicText, plainTextToHtml, prepareArticleHtml, requirePublicUrl } from "./fetch";
import { MAX_ARTICLE_BYTES, Refused, json, readJson, sha256Hex } from "./http";

const FEED_ACCEPT = "application/atom+xml, application/rss+xml, application/xml, text/xml;q=0.9, text/html;q=0.5, */*;q=0.1";
const HTML_ACCEPT = "text/html, application/xhtml+xml, application/xml;q=0.9, */*;q=0.8";
const HTML_TYPE = /^(text\/html|application\/xhtml\+xml|text\/plain)\b/u;

/** Keep feed polling inside the free plan's 50 subrequests alongside Readwise. */
const MAX_FETCHES_PER_POLL = 20;
const MAX_ITEMS_PER_FEED = 10;

/** Inline feed HTML with at least this many readable characters is staged directly without a page fetch. */
const FULL_CONTENT_CHARS = 400;

export interface FeedRow {
  id: string;
  url: string;
  title: string;
  site_url: string | null;
  etag: string | null;
  last_modified: string | null;
  added_at: number;
  checked_at: number;
  last_error: string | null;
}

const FollowInput = z.object({ url: z.string().trim().min(1) });

export async function listFeeds(env: Env): Promise<Response> {
  const { results } = await env.DB.prepare(
    "SELECT id, url, title, site_url, added_at, checked_at, last_error FROM feeds ORDER BY title COLLATE NOCASE ASC, added_at DESC",
  ).all<Omit<FeedRow, "etag" | "last_modified">>();

  return json({ feeds: results });
}

export async function followFeed(request: Request, env: Env): Promise<Response> {
  const { url: rawUrl } = await readJson(request, FollowInput, "Send a JSON body with the feed’s `url`.");
  const initial = await fetchPublicText(rawUrl, { accept: FEED_ACCEPT });

  if (initial.status < 200 || initial.status >= 300) {
    throw new Refused(502, `That site answered ${initial.status}, so the feed couldn’t be followed.`);
  }

  let feedUrl = initial.url;
  let parsed = parseFeed(initial.text, feedUrl);

  if (!parsed) {
    const discovered = discoverFeedUrl(initial.text, feedUrl);

    if (!discovered) throw new Refused(400, "No RSS or Atom feed was found at that address.");
    const second = await fetchPublicText(discovered, { accept: FEED_ACCEPT });

    if (second.status < 200 || second.status >= 300) {
      throw new Refused(502, `The feed at ${discovered} answered ${second.status}.`);
    }

    feedUrl = second.url;
    parsed = parseFeed(second.text, feedUrl);

    if (!parsed) throw new Refused(400, "No RSS or Atom feed was found at that address.");
  }

  const id = (await sha256Hex(feedUrl)).slice(0, 32);

  const existing = await env.DB.prepare("SELECT id, url, title, site_url, added_at, checked_at, last_error FROM feeds WHERE id = ?")
    .bind(id)
    .first<Omit<FeedRow, "etag" | "last_modified">>();

  if (existing) {
    return json(existing, 200);
  }

  const now = Date.now();
  await env.DB.prepare("INSERT INTO feeds (id, url, title, site_url, added_at, checked_at) VALUES (?, ?, ?, ?, ?, 0)")
    .bind(id, feedUrl, parsed.title, parsed.siteUrl, now)
    .run();

  return json({ id, url: feedUrl, title: parsed.title, site_url: parsed.siteUrl, added_at: now, checked_at: 0, last_error: null }, 201);
}

export async function unfollowFeed(env: Env, id: string): Promise<Response> {
  const existing = await env.DB.prepare("SELECT 1 FROM feeds WHERE id = ?").bind(id).first();

  if (!existing) throw new Refused(404, "That feed isn’t followed.");

  await env.DB.batch([
    env.DB.prepare("DELETE FROM feed_seen WHERE feed_id = ?").bind(id),
    env.DB.prepare("DELETE FROM feeds WHERE id = ?").bind(id),
  ]);

  return json({ ok: true });
}

function buildInlineArticle(item: ParsedFeedItem, feedTitle: string): string | null {
  const raw = item.contentHtml ?? item.summaryHtml;

  if (!raw) return null;
  const plain = stripTags(raw);

  if (!plain) return null;
  const meta = { title: item.title, author: item.author, site: feedTitle, published: item.published };

  if (/<[a-z][\s\S]*>/iu.test(raw)) {
    return prepareArticleHtml(raw, meta);
  }

  return plainTextToHtml(raw, meta);
}

async function fetchLinkedArticle(item: ParsedFeedItem, feedTitle: string): Promise<{ url: string; html: string } | null> {
  if (!item.link) return null;

  try {
    requirePublicUrl(item.link);
    const fetched = await fetchPublicText(item.link, { accept: HTML_ACCEPT });

    if (fetched.status < 200 || fetched.status >= 300) return null;

    if (fetched.contentType && !HTML_TYPE.test(fetched.contentType)) return null;

    const html = prepareArticleHtml(fetched.text, {
      title: item.title,
      author: item.author,
      site: feedTitle,
      published: item.published,
    });

    return { url: fetched.url, html };
  } catch {
    return null;
  }
}

/** Runs on the 30-minute Worker cron: fetches followed feeds and stages new items in the Inbox. */
export async function pollFeeds(env: Env): Promise<{ checked: number; added: number }> {
  const { results: feeds } = await env.DB.prepare(
    "SELECT id, url, title, site_url, etag, last_modified, added_at, checked_at, last_error FROM feeds ORDER BY checked_at ASC",
  ).all<FeedRow>();

  let fetches = 0;
  let checked = 0;
  let added = 0;

  for (const feed of feeds) {
    if (fetches >= MAX_FETCHES_PER_POLL) break;
    fetches++;
    checked++;

    let fetched;

    try {
      fetched = await fetchPublicText(feed.url, { accept: FEED_ACCEPT, etag: feed.etag, lastModified: feed.last_modified });
    } catch (e) {
      const message = e instanceof Error ? e.message : "The feed couldn’t be fetched.";
      await env.DB.prepare("UPDATE feeds SET checked_at = ?, last_error = ? WHERE id = ?").bind(Date.now(), message.slice(0, 500), feed.id).run();
      continue;
    }

    if (fetched.status === 304) {
      await env.DB.prepare("UPDATE feeds SET checked_at = ?, last_error = NULL WHERE id = ?").bind(Date.now(), feed.id).run();
      continue;
    }

    if (fetched.status < 200 || fetched.status >= 300) {
      await env.DB.prepare("UPDATE feeds SET checked_at = ?, last_error = ? WHERE id = ?")
        .bind(Date.now(), `Feed answered ${fetched.status}.`, feed.id)
        .run();
      continue;
    }

    const parsed = parseFeed(fetched.text, fetched.url);

    if (!parsed) {
      await env.DB.prepare("UPDATE feeds SET checked_at = ?, last_error = ? WHERE id = ?")
        .bind(Date.now(), "Response wasn’t a valid RSS or Atom feed.", feed.id)
        .run();
      continue;
    }

    let queuedForFeed = 0;

    for (const item of parsed.items) {
      if (queuedForFeed >= MAX_ITEMS_PER_FEED) break;
      const itemKey = await sha256Hex(item.guid);

      if (await env.DB.prepare("SELECT 1 FROM feed_seen WHERE feed_id = ? AND item_key = ?").bind(feed.id, itemKey).first()) {
        continue;
      }

      const itemBookId = item.link ? await sha256Hex(canonicalUrl(item.link)) : null;

      if (itemBookId && (await env.DB.prepare("SELECT 1 FROM books WHERE id = ?").bind(itemBookId).first())) {
        await env.DB.prepare("INSERT OR IGNORE INTO feed_seen (feed_id, item_key, seen_at) VALUES (?, ?, ?)").bind(feed.id, itemKey, Date.now()).run();
        continue;
      }

      if (item.link && (await env.DB.prepare("SELECT 1 FROM incoming WHERE url = ? AND state IN ('waiting', 'imported')").bind(item.link).first())) {
        await env.DB.prepare("INSERT OR IGNORE INTO feed_seen (feed_id, item_key, seen_at) VALUES (?, ?, ?)").bind(feed.id, itemKey, Date.now()).run();
        continue;
      }

      const dedupKey = `feed:${feed.id}:${itemKey}`;
      const incomingId = await sha256Hex(dedupKey);
      const inlineTextLen = item.contentHtml ? stripTags(item.contentHtml).length : 0;
      const feedTitle = parsed.title || feed.title;
      let itemUrl = item.link ?? `https://feeds.inkling.invalid/${feed.id}/${itemKey}`;
      let html: string | null = null;

      if (inlineTextLen >= FULL_CONTENT_CHARS) {
        html = buildInlineArticle(item, feedTitle);
      } else if (item.link) {
        if (fetches >= MAX_FETCHES_PER_POLL) break;
        fetches++;
        const linked = await fetchLinkedArticle(item, feedTitle);

        if (linked) {
          itemUrl = linked.url;
          html = linked.html;
        } else {
          html = buildInlineArticle(item, feedTitle);
        }
      } else {
        html = buildInlineArticle(item, feedTitle);
      }

      if (!html) {
        await env.DB.prepare("INSERT OR IGNORE INTO feed_seen (feed_id, item_key, seen_at) VALUES (?, ?, ?)").bind(feed.id, itemKey, Date.now()).run();
        continue;
      }

      const bytes = new TextEncoder().encode(html);
      const now = Date.now();

      if (bytes.byteLength > MAX_ARTICLE_BYTES) {
        await env.DB.batch([
          env.DB.prepare(
            `INSERT OR IGNORE INTO incoming (id, source, destination, url, title, author, site, published, feed_id, dedup_key, state, bytes, error, received_at)
             VALUES (?, 'feed', 'inbox', ?, ?, ?, ?, ?, ?, ?, 'too_large', ?, ?, ?)`,
          ).bind(
            incomingId,
            itemUrl,
            item.title,
            item.author,
            feedTitle,
            item.published,
            feed.id,
            dedupKey,
            bytes.byteLength,
            "The article is larger than 5 MB, which is as much as the server keeps.",
            now,
          ),
          env.DB.prepare("INSERT OR IGNORE INTO feed_seen (feed_id, item_key, seen_at) VALUES (?, ?, ?)").bind(feed.id, itemKey, now),
        ]);
        continue;
      }

      await env.FILES.put(`incoming/${incomingId}`, bytes);
      await env.DB.batch([
        env.DB.prepare(
          `INSERT OR IGNORE INTO incoming (id, source, destination, url, title, author, site, published, feed_id, dedup_key, state, bytes, received_at)
           VALUES (?, 'feed', 'inbox', ?, ?, ?, ?, ?, ?, ?, 'waiting', ?, ?)`,
        ).bind(incomingId, itemUrl, item.title, item.author, feedTitle, item.published, feed.id, dedupKey, bytes.byteLength, now),
        env.DB.prepare("INSERT OR IGNORE INTO feed_seen (feed_id, item_key, seen_at) VALUES (?, ?, ?)").bind(feed.id, itemKey, now),
      ]);
      queuedForFeed++;
      added++;
    }

    await env.DB.prepare("UPDATE feeds SET title = ?, site_url = ?, etag = ?, last_modified = ?, checked_at = ?, last_error = NULL WHERE id = ?")
      .bind(parsed.title || feed.title, parsed.siteUrl ?? feed.site_url, fetched.etag, fetched.lastModified, Date.now(), feed.id)
      .run();
  }

  return { checked, added };
}
