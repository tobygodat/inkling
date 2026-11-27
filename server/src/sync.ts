// What the Mac and iPhone apps call with a token for the `/v1` resource and the `sync` scope.
// Books follow the reader's model (tobygodat/ios-reader, crates/reader-core/src/sync.rs): a book's id is the hash
// of its file and books are only added; a position carries when it changed, and the later wins.
// A document carries a version the server counts up on every write it takes, and a write names the
// version it was made on (`base`): a write on an older base is refused with what the server has, so
// no save replaces one it hasn't seen. The device keeps both copies then (sync/src/documents.rs).
// Positions' `at` and documents' `saved_at` are milliseconds since the Unix epoch.

import { insufficientScope, type OAuthResourceContext } from "@cloudflare/workers-oauth-provider";
import { z } from "zod";
import { getDocument, listDocuments, putDocument } from "./documents";
import type { Env, Owner } from "./env";
import { followFeed, listFeeds, unfollowFeed } from "./feeds";
import { listBookHighlights, listHighlights, upsertHighlights } from "./highlights";
import { BOOK_ID, COVER_NAME, DOCUMENT_ID, FEED_ID, IMAGE_NAME, INCOMING_ID, MAX_FILE_BYTES, Refused, json, readJson, whole } from "./http";
import { connectReadwise, disconnectReadwise, readwiseStatus } from "./readwise";
import { saveFromSync, settleIncoming } from "./save";

type Captures = [path: string, first: string, second: string];

export async function serveSync(request: Request, env: Env, ctx: OAuthResourceContext<Owner>): Promise<Response> {
  if (!ctx.auth.scope.includes("sync")) return insufficientScope(ctx.auth, ["sync"]);
  const { pathname } = new URL(request.url);
  const method = request.method;

  // Every pattern below captures one or two segments, so a group it lacks is never read.
  const at = (pattern: RegExp): Captures | null => {
    const found = pathname.match(pattern);

    return found && [found[0], found[1] ?? "", found[2] ?? ""];
  };

  let m: Captures | null;

  if (pathname === "/v1/library" && method === "GET") return library(env);

  if ((m = at(/^\/v1\/books\/([^/]+)$/u)) && method === "PUT") return putBook(request, env, bookId(m[1]));

  if ((m = at(/^\/v1\/books\/([^/]+)\/file$/u))) {
    const key = `book/${bookId(m[1])}`;

    if (method === "PUT") return putFile(request, env, key);

    if (method === "GET") return getFile(env, key);
  }

  if ((m = at(/^\/v1\/books\/([^/]+)\/images\/([^/]+)$/u))) {
    if (!IMAGE_NAME.test(m[2])) throw new Refused(400, "That isn’t an image name.");
    const key = `image/${bookId(m[1])}/${m[2]}`;

    if (method === "PUT") return putFile(request, env, key);

    if (method === "GET") return getFile(env, key);
  }

  if ((m = at(/^\/v1\/books\/([^/]+)\/chapters\/(\d+)$/u)) && method === "PUT") {
    return putChapter(request, env, bookId(m[1]), Number(m[2]));
  }

  if ((m = at(/^\/v1\/books\/([^/]+)\/position$/u)) && method === "PUT") {
    return putPosition(request, env, bookId(m[1]));
  }

  if ((m = at(/^\/v1\/books\/([^/]+)\/highlights$/u))) {
    const id = bookId(m[1]);

    if (method === "GET") return listBookHighlights(env, id);

    if (method === "POST") return upsertHighlights(request, env, id);
  }

  if (pathname === "/v1/highlights" && method === "GET") return listHighlights(env);

  if ((m = at(/^\/v1\/covers\/([^/]+)$/u))) {
    if (!COVER_NAME.test(m[1])) throw new Refused(400, "That isn’t a cover name.");

    if (method === "PUT") return putFile(request, env, `cover/${m[1]}`);

    if (method === "GET") return getFile(env, `cover/${m[1]}`);
  }

  if (pathname === "/v1/documents" && method === "GET") return listDocuments(env);

  if ((m = at(/^\/v1\/documents\/([^/]+)$/u))) {
    if (!DOCUMENT_ID.test(m[1])) throw new Refused(400, "That isn’t a document id.");

    if (method === "GET") return getDocument(env, m[1]);

    if (method === "PUT") return putDocument(request, env, m[1]);
  }

  if ((m = at(/^\/v1\/inbox\/([^/]+)\/file$/u)) && method === "GET") return getFile(env, `inbox/${m[1]}`);

  if ((m = at(/^\/v1\/inbox\/([^/]+)$/u)) && method === "POST") return settleInbox(request, env, m[1]);

  if ((m = at(/^\/v1\/incoming\/([^/]+)\/file$/u)) && method === "GET") return getFile(env, `incoming/${incomingId(m[1])}`);

  if ((m = at(/^\/v1\/incoming\/([^/]+)$/u)) && method === "POST") return settleIncoming(request, env, incomingId(m[1]));

  if (pathname === "/v1/save" && method === "POST") return saveFromSync(request, env);

  if (pathname === "/v1/feeds") {
    if (method === "GET") return listFeeds(env);

    if (method === "POST") return followFeed(request, env);
  }

  if ((m = at(/^\/v1\/feeds\/([^/]+)$/u)) && method === "DELETE") return unfollowFeed(env, feedId(m[1]));

  if (pathname === "/v1/readwise") {
    if (method === "GET") return json(await readwiseStatus(env));

    if (method === "PUT") return connectReadwise(request, env);

    if (method === "DELETE") return disconnectReadwise(env);
  }

  return json({ error: "Not found." }, 404);
}

const bookId = (id: string) => {
  if (!BOOK_ID.test(id)) throw new Refused(400, "That isn’t a book id.");

  return id;
};

const incomingId = (id: string) => {
  if (!INCOMING_ID.test(id)) throw new Refused(400, "That isn’t an incoming item id.");

  return id;
};

const feedId = (id: string) => {
  if (!FEED_ID.test(id)) throw new Refused(400, "That isn’t a feed id.");

  return id;
};

const StoredBook = z.object({
  id: z.string(),
  title: z.string(),
  author: z.string().nullable(),
  format: z.enum(["Epub", "Pdf", "Article"]),
  chapters: z.number(),
  cover: z.string().nullable(),
  added: z.number(),
  chapter: z.number(),
  fraction: z.number(),
  position_at: z.number(),
  source_url: z.string().nullable(),
  inbox: z.number(),
});

async function library(env: Env): Promise<Response> {
  // One batch, so the books, Readwise inbox and incoming articles come from the same moment.
  const [books, inbox, incoming] = await env.DB.batch([
    env.DB.prepare(
      "SELECT id, title, author, format, chapters, cover, added, chapter, fraction, position_at, source_url, inbox FROM books ORDER BY added DESC",
    ),
    env.DB.prepare("SELECT readwise_id, title, author, category, state, bytes, error, book_id, received_at FROM inbox ORDER BY received_at DESC"),
    env.DB.prepare(
      "SELECT id, source, destination, url, title, author, site, published, feed_id, state, bytes, error, book_id, received_at FROM incoming ORDER BY received_at DESC",
    ),
  ]);

  if (!books || !inbox || !incoming) throw new Error("D1 answered a batch of three statements with fewer results.");

  const rows = z.array(StoredBook).parse(books.results).map((b) => ({
    id: b.id,
    title: b.title,
    author: b.author,
    format: b.format,
    chapters: b.chapters,
    cover: b.cover,
    added: b.added,
    chapter: b.chapter,
    fraction: b.fraction,
    position_at: b.position_at,
    source_url: b.source_url,
    inbox: b.inbox === 1,
  }));

  return json({ books: rows, inbox: inbox.results, incoming: incoming.results });
}

/** A file kept whole in KV: a parsed book, a cover, a Readwise original, or an incoming page. */
async function putFile(request: Request, env: Env, key: string): Promise<Response> {
  const declared = Number(request.headers.get("content-length") ?? NaN);

  if (declared > MAX_FILE_BYTES) throw new Refused(413, "That file is larger than 25 MB, which is as much as the server keeps.");
  const bytes = await request.arrayBuffer();

  if (bytes.byteLength > MAX_FILE_BYTES) throw new Refused(413, "That file is larger than 25 MB, which is as much as the server keeps.");
  await env.FILES.put(key, bytes);

  return json({ ok: true });
}

async function getFile(env: Env, key: string): Promise<Response> {
  const stored = await env.FILES.get(key, "stream");

  return stored ? new Response(stored) : json({ error: "Not found." }, 404);
}

const Chapter = z.object({ title: z.string(), text: z.string() });

async function putChapter(request: Request, env: Env, id: string, n: number): Promise<Response> {
  const body = await readJson(request, Chapter, "A chapter needs a title and text.");

  await env.DB.prepare(
    `INSERT INTO chapters (book_id, n, title, text) VALUES (?, ?, ?, ?)
     ON CONFLICT(book_id, n) DO UPDATE SET title = excluded.title, text = excluded.text`,
  )
    .bind(id, n, body.title, body.text)
    .run();

  return json({ ok: true });
}

const Book = z.object({
  title: z.string(),
  author: z.string().nullish(),
  format: z.enum(["Epub", "Pdf", "Article"]),
  chapters: whole,
  added: whole,
  cover: z.string().regex(COVER_NAME).nullish(),
  source_url: z.string().nullish(),
  inbox: z.boolean().optional(),
});

/** The book's row, written last: its parsed file and every chapter must already be here. */
async function putBook(request: Request, env: Env, id: string): Promise<Response> {
  const b = await readJson(request, Book, "That book row is incomplete.");
  const file = await env.FILES.get(`book/${id}`, "stream");

  if (!file) throw new Refused(409, "Upload the book’s file first.");
  await file.cancel();

  const stored = await env.DB.prepare("SELECT count(*) AS n FROM chapters WHERE book_id = ? AND n < ?")
    .bind(id, b.chapters)
    .first<{ n: number }>();

  if ((stored?.n ?? 0) !== b.chapters) throw new Refused(409, "Upload every chapter first.");
  await env.DB.prepare(
    `INSERT INTO books (id, title, author, format, chapters, cover, added, source_url, inbox) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
     ON CONFLICT(id) DO UPDATE SET
       cover = COALESCE(excluded.cover, books.cover),
       source_url = COALESCE(excluded.source_url, books.source_url)`,
  )
    .bind(id, b.title, b.author ?? null, b.format, b.chapters, b.cover ?? null, b.added, b.source_url ?? null, b.inbox ? 1 : 0)
    .run();

  return json({ ok: true });
}

const Position = z.object({ chapter: whole, fraction: z.number().min(0).max(1), at: whole });

async function putPosition(request: Request, env: Env, id: string): Promise<Response> {
  const p = await readJson(request, Position, "A position needs a chapter, a fraction from 0 to 1, and when it changed.");

  // The later position wins, whichever device reaches the server first.
  const { meta } = await env.DB.prepare("UPDATE books SET chapter = ?, fraction = ?, position_at = ? WHERE id = ? AND position_at < ?")
    .bind(p.chapter, p.fraction, p.at, id, p.at)
    .run();

  return json({ ok: true, kept: meta.changes === 1 });
}

const Settled = z.discriminatedUnion("state", [
  z.object({ state: z.literal("imported"), book_id: z.string().regex(BOOK_ID) }),
  z.object({ state: z.literal("failed"), error: z.string() }),
]);

async function settleInbox(request: Request, env: Env, readwiseId: string): Promise<Response> {
  const s = await readJson(request, Settled, "Say imported with the book’s id, or failed with why.");

  if (s.state === "imported") {
    await env.DB.prepare("UPDATE inbox SET state = 'imported', book_id = ?, error = NULL WHERE readwise_id = ?").bind(s.book_id, readwiseId).run();
  } else {
    await env.DB.prepare("UPDATE inbox SET state = 'failed', error = ? WHERE readwise_id = ?").bind(s.error.slice(0, 500), readwiseId).run();
  }

  await env.FILES.delete(`inbox/${readwiseId}`);

  return json({ ok: true });
}
