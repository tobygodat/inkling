import { env } from "cloudflare:workers";
import { afterEach, describe, expect, it, vi } from "vitest";
import { pollFeeds } from "../src/feeds";
import { bearer, call, signIn } from "./signin";

const LONG_BODY = "The tide rose steadily across the salt marsh while curlews called along the channel. ".repeat(8);

const RSS_FEED = `<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/" xmlns:dc="http://purl.org/dc/elements/1.1/">
  <channel>
    <title>The Shore &amp; Flats</title>
    <link>https://shore.example/</link>
    <item>
      <guid>shore-1</guid>
      <title><![CDATA[Morning on the Marsh]]></title>
      <link>https://shore.example/marsh</link>
      <dc:creator>Ana Lima</dc:creator>
      <pubDate>Thu, 20 Nov 2026 08:00:00 GMT</pubDate>
      <content:encoded><![CDATA[<p>${LONG_BODY}</p>]]></content:encoded>
    </item>
  </channel>
</rss>`;

const ATOM_FEED = `<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Field Notes</title>
  <link href="https://notes.example/" rel="alternate"/>
  <entry>
    <id>tag:notes.example,2026:post-1</id>
    <title>Winter Birds</title>
    <link href="/winter-birds" rel="alternate"/>
    <author><name>Samira Chen</name></author>
    <updated>2026-11-22T10:00:00Z</updated>
    <summary>A short teaser only.</summary>
  </entry>
</feed>`;

const LINKED_ARTICLE = `<!doctype html><html><head><title>Winter Birds</title></head><body><article><h1>Winter Birds</h1><p>${LONG_BODY}</p></article></body></html>`;

const SITE_HOME = `<!doctype html><html><head><title>The Shore</title><link rel="alternate" type="application/rss+xml" href="/rss.xml"></head><body>Home</body></html>`;

async function syncToken() {
  const { token } = await signIn({ scope: "sync offline_access", tick: ["sync"], resource: "http://localhost/v1" });

  return token!;
}

afterEach(() => vi.restoreAllMocks());

describe("RSS and Atom feeds", () => {
  it("discovers and follows feeds, polls new items into the Inbox without duplicates, and honours 304", async () => {
    const token = await syncToken();
    const real = globalThis.fetch;
    const seenRequests: { url: string; etag: string | null }[] = [];
    let rssEtag = '"rss-v1"';

    vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
      const url = input instanceof Request ? input.url : String(input);
      const headers = new Headers(init?.headers ?? (input instanceof Request ? input.headers : undefined));
      const inm = headers.get("if-none-match");
      seenRequests.push({ url, etag: inm });

      if (url === "https://shore.example/") {
        return new Response(SITE_HOME, { headers: { "content-type": "text/html" } });
      }

      if (url === "https://shore.example/rss.xml") {
        if (inm === rssEtag) return new Response(null, { status: 304, headers: { etag: rssEtag } });

        return new Response(RSS_FEED, { headers: { "content-type": "application/rss+xml", etag: rssEtag } });
      }

      if (url === "https://notes.example/atom.xml") {
        return new Response(ATOM_FEED, { headers: { "content-type": "application/atom+xml" } });
      }

      if (url === "https://notes.example/winter-birds") {
        return new Response(LINKED_ARTICLE, { headers: { "content-type": "text/html" } });
      }

      return real(input, init);
    });

    // Autodiscover RSS feed from site URL.
    const followedRss = await call("/v1/feeds", bearer(token, { method: "POST", body: JSON.stringify({ url: "https://shore.example/" }) }));
    expect(followedRss.status).toBe(201);
    const rss = await followedRss.json<{ id: string; url: string; title: string }>();
    expect(rss).toMatchObject({ url: "https://shore.example/rss.xml", title: "The Shore & Flats" });

    // Follow Atom feed directly.
    const followedAtom = await call("/v1/feeds", bearer(token, { method: "POST", body: JSON.stringify({ url: "https://notes.example/atom.xml" }) }));
    expect(followedAtom.status).toBe(201);
    const atom = await followedAtom.json<{ id: string; url: string; title: string }>();
    expect(atom).toMatchObject({ url: "https://notes.example/atom.xml", title: "Field Notes" });

    const listed = await (await call("/v1/feeds", bearer(token))).json<{ feeds: { id: string }[] }>();
    expect(listed.feeds).toHaveLength(2);

    // First cron poll stages both items into incoming with destination = "inbox".
    const firstPoll = await pollFeeds(env);
    expect(firstPoll).toEqual({ checked: 2, added: 2 });

    const library = await (await call("/v1/library", bearer(token))).json<{
      incoming: { id: string; source: string; destination: string; title: string; author: string | null; site: string | null; url: string; state: string }[];
    }>();

    expect(library.incoming).toHaveLength(2);
    const byTitle = Object.fromEntries(library.incoming.map((item) => [item.title, item]));
    expect(byTitle["Morning on the Marsh"]).toMatchObject({
      source: "feed",
      destination: "inbox",
      author: "Ana Lima",
      site: "The Shore & Flats",
      url: "https://shore.example/marsh",
      state: "waiting",
    });
    expect(byTitle["Winter Birds"]).toMatchObject({
      source: "feed",
      destination: "inbox",
      author: "Samira Chen",
      site: "Field Notes",
      url: "https://notes.example/winter-birds",
      state: "waiting",
    });

    // Full-content RSS item did not trigger an extra page fetch; summary-only Atom item did.
    expect(seenRequests.some((r) => r.url === "https://shore.example/marsh")).toBe(false);
    expect(seenRequests.some((r) => r.url === "https://notes.example/winter-birds")).toBe(true);

    const marshHtml = await (await call(`/v1/incoming/${byTitle["Morning on the Marsh"]!.id}/file`, bearer(token))).text();
    expect(marshHtml).toContain("The Shore &amp; Flats");
    expect(marshHtml).toContain("curlews called");

    // Second poll sends If-None-Match for RSS (gets 304) and re-reads Atom (gets 200) without adding duplicates.
    const secondPoll = await pollFeeds(env);
    expect(secondPoll).toEqual({ checked: 2, added: 0 });
    expect(seenRequests.some((r) => r.url === "https://shore.example/rss.xml" && r.etag === rssEtag)).toBe(true);

    // Even when the ETag changes with the same items, feed_seen prevents duplicates.
    rssEtag = '"rss-v2"';
    const thirdPoll = await pollFeeds(env);
    expect(thirdPoll).toEqual({ checked: 2, added: 0 });

    // Unfollow removes the feed.
    expect((await call(`/v1/feeds/${rss.id}`, bearer(token, { method: "DELETE" }))).status).toBe(200);
    const afterDelete = await (await call("/v1/feeds", bearer(token))).json<{ feeds: { id: string }[] }>();
    expect(afterDelete.feeds.map((f) => f.id)).toEqual([atom.id]);
  });
});
