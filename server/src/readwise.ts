// Readwise Reader into the inbox (ADR 0004). The Worker only moves files: the 30-minute poll finds a
// new EPUB or PDF, its original is streamed into KV, and the next device to sync parses it with
// inkling-books and adds it to its library. https://readwise.io/reader_api

import { z } from "zod";
import type { Env } from "./env";
import { MAX_FILE_BYTES, Refused, json, readJson } from "./http";
import { open, seal } from "./secret";

const API = "https://readwise.io/api";
const CATEGORIES = new Set(["epub", "pdf"]);

/** Each document costs two outgoing requests, and a Worker on the free plan may make 50. */
const MAX_PER_POLL = 10;

interface Link {
  token: string;
  connected_at: string;
  since: string;
}

const ReaderDocument = z.object({
  id: z.string(),
  title: z.string().nullish(),
  author: z.string().nullish(),
  category: z.string().nullish(),
  updated_at: z.string().nullish(),
  saved_at: z.string().nullish(),
  raw_source_url: z.string().nullish(),
});

type ReaderDocument = z.infer<typeof ReaderDocument>;

/** A page of Reader's list endpoint. */
const ReaderList = z.object({ results: z.array(ReaderDocument) });

const Token = z.object({ token: z.string().trim().min(1) });

async function link(env: Env): Promise<Link | null> {
  const row = await env.DB.prepare("SELECT token, connected_at, since FROM readwise WHERE id = 1").first<Link>();

  if (!row) return null;

  return { ...row, token: await open(env.SECRET_KEY, row.token) };
}

export async function readwiseStatus(env: Env) {
  const row = await env.DB.prepare("SELECT since FROM readwise WHERE id = 1").first<{ since: string }>();

  return { connected: row !== null, since: row?.since ?? null };
}

/**
 * Keeps the writer's Readwise token, after Readwise says it is good. Only documents added from now
 * on come in, so connecting doesn't pull a whole Reader library into KV at once.
 */
export async function connectReadwise(request: Request, env: Env): Promise<Response> {
  const { token } = await readJson(request, Token, "Paste the token from readwise.io/access_token.");
  const check = await fetch(`${API}/v2/auth/`, { headers: { authorization: `Token ${token}` } });

  if (check.status !== 204) throw new Refused(400, "Readwise didn’t accept that token.");
  const now = new Date().toISOString();
  // A new token keeps the link's start, so nothing saved meanwhile is skipped.
  await env.DB.prepare(
    `INSERT INTO readwise (id, token, connected_at, since) VALUES (1, ?, ?, ?)
     ON CONFLICT(id) DO UPDATE SET token = excluded.token`,
  )
    .bind(await seal(env.SECRET_KEY, token), now, now)
    .run();

  return json(await readwiseStatus(env));
}

export async function disconnectReadwise(env: Env): Promise<Response> {
  await env.DB.prepare("DELETE FROM readwise WHERE id = 1").run();

  return json(await readwiseStatus(env));
}

/** Every 30 minutes: documents saved to Reader since the last look. */
export async function poll(env: Env): Promise<void> {
  const current = await link(env);

  if (!current) return;
  const found: ReaderDocument[] = [];

  for (const category of CATEGORIES) {
    const url = new URL(`${API}/v3/list/`);
    url.searchParams.set("category", category);
    url.searchParams.set("updatedAfter", current.since);
    const response = await fetch(url, { headers: { authorization: `Token ${current.token}` } });

    const listed = response.ok ? ReaderList.safeParse(await response.json()) : null;

    if (!listed?.success) return;
    found.push(...listed.data.results);
  }

  // Oldest first, so the cursor only moves past what was handled. A document saved before the
  // link was made has only been read or moved in Reader, so it is passed over.
  // Readwise and this Worker write timestamps differently, so they are compared as times.
  const time = (t: string | null | undefined) => (t ? Date.parse(t) : NaN);
  found.sort((a, b) => (time(a.updated_at) || 0) - (time(b.updated_at) || 0));
  let since = current.since;
  let received = 0;

  for (const doc of found) {
    if (time(doc.saved_at) >= time(current.connected_at)) {
      if (received === MAX_PER_POLL) break;
      await receive(env, current.token, doc.id);
      received++;
    }

    const updated = doc.updated_at;

    if (updated && time(updated) > time(since)) since = updated;
  }

  await env.DB.prepare("UPDATE readwise SET since = ? WHERE id = 1").bind(since).run();
}

/** Puts one Reader document's original in the inbox, once. */
export async function receive(env: Env, token: string, readwiseId: string): Promise<void> {
  if (await env.DB.prepare("SELECT 1 FROM inbox WHERE readwise_id = ?").bind(readwiseId).first()) return;
  const url = new URL(`${API}/v3/list/`);
  url.searchParams.set("id", readwiseId);
  url.searchParams.set("withRawSourceUrl", "true");
  const listed = await fetch(url, { headers: { authorization: `Token ${token}` } });

  const page = listed.ok ? ReaderList.safeParse(await listed.json()) : null;

  if (!page?.success) return;
  const doc = page.data.results[0];

  if (!doc || !doc.category || !CATEGORIES.has(doc.category)) return;

  const record = (state: string, bytes: number | null, error: string | null) =>
    env.DB.prepare(
      "INSERT OR IGNORE INTO inbox (readwise_id, title, author, category, state, bytes, error, received_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
      .bind(readwiseId, doc.title || "Untitled", doc.author ?? null, doc.category, state, bytes, error, Date.now())
      .run();

  if (!doc.raw_source_url) {
    await record("failed", null, "Readwise has no file for this document.");

    return;
  }

  const file = await fetch(doc.raw_source_url);

  if (!file.ok || !file.body) {
    await record("failed", null, `The file couldn’t be downloaded from Readwise (${file.status}).`);

    return;
  }

  const declared = Number(file.headers.get("content-length") ?? NaN);

  if (declared > MAX_FILE_BYTES) {
    await file.body.cancel();
    await record("too_large", declared, "The file is larger than 25 MB, which is as much as the server keeps.");

    return;
  }

  // Streamed, so the Worker never holds the file or spends CPU on it.
  try {
    await env.FILES.put(`inbox/${readwiseId}`, file.body);
  } catch {
    await record("too_large", Number.isFinite(declared) ? declared : null, "The file is larger than 25 MB, which is as much as the server keeps.");

    return;
  }

  await record("waiting", Number.isFinite(declared) ? declared : null, null);
}
