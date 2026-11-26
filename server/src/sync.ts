// What the Mac and iPhone apps call with a token for the `/v1` resource and the `sync` scope.
// Books follow the reader's model (tobygodat/ios-reader, crates/reader-core/src/sync.rs): a book's id is the hash
// of its file and books are only added; a position carries when it changed, and the later wins.
// A document carries a version the server counts up on every write it takes, and a write names the
// version it was made on (`base`): a write on an older base is refused with what the server has, so
// no save replaces one it hasn't seen. The device keeps both copies then (sync/src/documents.rs).
// Positions' `at` and documents' `saved_at` are milliseconds since the Unix epoch.

import { insufficientScope, type OAuthResourceContext } from "@cloudflare/workers-oauth-provider";
import { z } from "zod";
import type { Env, Owner } from "./env";
import { BOOK_ID, COVER_NAME, DOCUMENT_ID, IMAGE_NAME, MAX_FILE_BYTES, Refused, json, readJson, whole } from "./http";
import { connectReadwise, disconnectReadwise, readwiseStatus } from "./readwise";

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

  if ((m = at(/^\/v1\/books\/([^/]+)$/))) {
    if (method === "PUT") return putBook(request, env, bookId(m[1]));
  }

  if ((m = at(/^\/v1\/books\/([^/]+)\/file$/))) {
    const key = `book/${bookId(m[1])}`;

    if (method === "PUT") return putFile(request, env, key);

    if (method === "GET") return getFile(env, key);
  }

  if ((m = at(/^\/v1\/books\/([^/]+)\/images\/([^/]+)$/))) {
    if (!IMAGE_NAME.test(m[2])) throw new Refused(400, "That isn’t an image name.");
    const key = `image/${bookId(m[1])}/${m[2]}`;

    if (method === "PUT") return putFile(request, env, key);

    if (method === "GET") return getFile(env, key);
  }

  if ((m = at(/^\/v1\/books\/([^/]+)\/chapters\/(\d+)$/)) && method === "PUT") {
    return putChapter(request, env, bookId(m[1]), Number(m[2]));
  }

  if ((m = at(/^\/v1\/books\/([^/]+)\/position$/)) && method === "PUT") {
    return putPosition(request, env, bookId(m[1]));
  }

  if ((m = at(/^\/v1\/covers\/([^/]+)$/))) {
    if (!COVER_NAME.test(m[1])) throw new Refused(400, "That isn’t a cover name.");

    if (method === "PUT") return putFile(request, env, `cover/${m[1]}`);

    if (method === "GET") return getFile(env, `cover/${m[1]}`);
  }

  if (pathname === "/v1/documents" && method === "GET") return listDocuments(env);

  if ((m = at(/^\/v1\/documents\/([^/]+)$/))) {
    if (!DOCUMENT_ID.test(m[1])) throw new Refused(400, "That isn’t a document id.");

    if (method === "GET") return getDocument(env, m[1]);

    if (method === "PUT") return putDocument(request, env, m[1]);
  }

  if ((m = at(/^\/v1\/inbox\/([^/]+)\/file$/)) && method === "GET") return getFile(env, `inbox/${m[1]}`);

  if ((m = at(/^\/v1\/inbox\/([^/]+)$/)) && method === "POST") return settleInbox(request, env, m[1]);

  if (pathname === "/v1/readwise") {
    if (method === "GET") return json(await readwiseStatus(env));

    if (method === "PUT") return connectReadwise(request, env);

    if (method === "DELETE") return disconnectReadwise(env);
  }

  return json({ error: "Not found." }, 404);
}

function bookId(id: string): string {
  if (!BOOK_ID.test(id)) throw new Refused(400, "That isn’t a book id.");

  return id;
}

async function library(env: Env): Promise<Response> {
  const [books, inbox] = await Promise.all([
    env.DB.prepare("SELECT id, title, author, format, chapters, cover, added, chapter, fraction, position_at FROM books ORDER BY added DESC").all(),
    env.DB.prepare("SELECT readwise_id, title, author, category, state, bytes, error, book_id, received_at FROM inbox ORDER BY received_at DESC").all(),
  ]);

  return json({ books: books.results, inbox: inbox.results });
}

/** A file kept whole in KV: a parsed book, a cover, or a Readwise original. */
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
  format: z.enum(["Epub", "Pdf"]),
  chapters: whole,
  added: whole,
  cover: z.string().regex(COVER_NAME).nullish(),
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
    `INSERT INTO books (id, title, author, format, chapters, cover, added) VALUES (?, ?, ?, ?, ?, ?, ?)
     ON CONFLICT(id) DO UPDATE SET cover = COALESCE(excluded.cover, books.cover)`,
  )
    .bind(id, b.title, b.author ?? null, b.format, b.chapters, b.cover ?? null, b.added)
    .run();

  return json({ ok: true });
}

const Position = z.object({ chapter: whole, fraction: z.number().min(0).max(1), at: whole });

async function putPosition(request: Request, env: Env, id: string): Promise<Response> {
  const p = await readJson(request, Position, "A position needs a chapter, a fraction from 0 to 1, and when it changed.");

  // The later position wins, whichever device reaches the server first.
  const { meta } = await env.DB.prepare(
    "UPDATE books SET chapter = ?, fraction = ?, position_at = ? WHERE id = ? AND position_at < ?",
  )
    .bind(p.chapter, p.fraction, p.at, id, p.at)
    .run();

  return json({ ok: true, kept: meta.changes === 1 });
}

type DocumentRow = {
  id: string;
  text: string;
  saved_at: number;
  device: string;
  version: number;
  deleted: number;
};

const shown = (d: DocumentRow) => ({ ...d, deleted: d.deleted === 1 });

/** Every document, deleted ones too, without their text. */
async function listDocuments(env: Env): Promise<Response> {
  const { results } = await env.DB.prepare("SELECT id, version, saved_at, device, deleted FROM documents ORDER BY id").all<DocumentRow>();

  return json(results.map(shown));
}

async function getDocument(env: Env, id: string): Promise<Response> {
  const doc = await env.DB.prepare("SELECT id, text, saved_at, device, version, deleted FROM documents WHERE id = ?").bind(id).first<DocumentRow>();

  return doc ? json(shown(doc)) : json({ error: "Not found." }, 404);
}

/**
 * Takes a write made on `base`, the version the device last had: 0 for a document the server
 * hasn't seen. Any other base is refused with 409 and what the server has. A delete keeps the
 * last text.
 */
const DocumentWrite = z.object({
  text: z.string(),
  saved_at: whole,
  device: z.string(),
  base: whole,
  deleted: z.boolean().optional(),
});

async function putDocument(request: Request, env: Env, id: string): Promise<Response> {
  const d = await readJson(
    request,
    DocumentWrite,
    "A document needs its text, when it was saved, the device and the version it was made on.",
  );

  const deleted = d.deleted ? 1 : 0;

  // One statement, so two devices writing at once can't both take the same base.
  const written =
    d.base === 0
      ? await env.DB.prepare(
          "INSERT INTO documents (id, text, saved_at, device, version, deleted) VALUES (?, ?, ?, ?, 1, ?) ON CONFLICT(id) DO NOTHING RETURNING version",
        )
          .bind(id, d.text, d.saved_at, d.device, deleted)
          .first<{ version: number }>()
      : await env.DB.prepare(
          `UPDATE documents SET text = CASE WHEN ? = 1 THEN text ELSE ? END, saved_at = ?, device = ?, version = version + 1, deleted = ?
           WHERE id = ? AND version = ? RETURNING version`,
        )
          .bind(deleted, d.text, d.saved_at, d.device, deleted, id, d.base)
          .first<{ version: number }>();

  if (written) return json({ version: written.version });
  const there = await env.DB.prepare("SELECT id, text, saved_at, device, version, deleted FROM documents WHERE id = ?").bind(id).first<DocumentRow>();

  return json(there ? shown(there) : { id, text: "", saved_at: 0, device: "", version: 0, deleted: false }, 409);
}

const Settled = z.discriminatedUnion("state", [
  z.object({ state: z.literal("imported"), book_id: z.string().regex(BOOK_ID) }),
  z.object({ state: z.literal("failed"), error: z.string() }),
]);

/** A device says how importing an inbox item went. Its file is no longer needed either way. */
async function settleInbox(request: Request, env: Env, readwiseId: string): Promise<Response> {
  const s = await readJson(request, Settled, "Say imported with the book’s id, or failed with why.");

  if (s.state === "imported") {
    await env.DB.prepare("UPDATE inbox SET state = 'imported', book_id = ?, error = NULL WHERE readwise_id = ?")
      .bind(s.book_id, readwiseId)
      .run();
  } else {
    await env.DB.prepare("UPDATE inbox SET state = 'failed', error = ? WHERE readwise_id = ?").bind(s.error.slice(0, 500), readwiseId).run();
  }

  await env.FILES.delete(`inbox/${readwiseId}`);

  return json({ ok: true });
}
