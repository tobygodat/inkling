// Readwise Reader into the inbox (ADR 0004). The Worker only moves files: a webhook or the
// 30-minute poll finds a new EPUB or PDF, its original is streamed into KV, and the next device to
// sync parses it with inkling-books. https://readwise.io/reader_api

import type { Env } from "./env";
import { MAX_FILE_BYTES, Refused, json, readJson } from "./http";
import { open, sameSecret, seal } from "./secret";

const API = "https://readwise.io/api";
/** Webhook events that mean a document was added. */
const ADDED = new Set(["reader.any_document.created", "reader.non_feed_document.created"]);
const CATEGORIES = new Set(["epub", "pdf"]);
/** Each document costs two outgoing requests, and a Worker on the free plan may make 50. */
const MAX_PER_POLL = 10;

interface Link {
  token: string;
  webhook_secret: string | null;
  connected_at: string;
  since: string;
}

interface Document {
  id: string;
  title?: string | null;
  author?: string | null;
  category?: string;
  updated_at?: string;
  saved_at?: string;
  raw_source_url?: string | null;
}

async function link(env: Env): Promise<Link | null> {
  const row = await env.DB.prepare("SELECT token, webhook_secret, connected_at, since FROM readwise WHERE id = 1").first<Link>();
  if (!row) return null;
  return {
    ...row,
    token: await open(env.SECRET_KEY, row.token),
    webhook_secret: row.webhook_secret ? await open(env.SECRET_KEY, row.webhook_secret) : null,
  };
}

export async function readwiseStatus(env: Env) {
  const row = await env.DB.prepare("SELECT webhook_secret IS NOT NULL AS webhook, since FROM readwise WHERE id = 1").first<{
    webhook: number;
    since: string;
  }>();
  return {
    connected: row !== null,
    webhook: row ? row.webhook === 1 : false,
    webhook_url: `${env.PUBLIC_URL}/readwise/webhook`,
    since: row?.since ?? null,
  };
}

/**
 * Keeps the writer's Readwise token, after Readwise says it is good. Only documents added from now
 * on come in, so connecting doesn't pull a whole Reader library into KV at once.
 */
export async function connectReadwise(request: Request, env: Env): Promise<Response> {
  const body = await readJson<{ token?: unknown; webhook_secret?: unknown }>(request);
  if (typeof body.token !== "string" || !body.token.trim()) throw new Refused(400, "Paste the token from readwise.io/access_token.");
  if (body.webhook_secret !== undefined && body.webhook_secret !== null && typeof body.webhook_secret !== "string") {
    throw new Refused(400, "The webhook secret should be text.");
  }
  const check = await fetch(`${API}/v2/auth/`, { headers: { authorization: `Token ${body.token.trim()}` } });
  if (check.status !== 204) throw new Refused(400, "Readwise didn’t accept that token.");
  const secret = typeof body.webhook_secret === "string" && body.webhook_secret.trim() ? body.webhook_secret.trim() : null;
  const now = new Date().toISOString();
  // A new token or secret keeps the link's start, so nothing saved meanwhile is skipped.
  await env.DB.prepare(
    `INSERT INTO readwise (id, token, webhook_secret, connected_at, since) VALUES (1, ?, ?, ?, ?)
     ON CONFLICT(id) DO UPDATE SET token = excluded.token, webhook_secret = excluded.webhook_secret`,
  )
    .bind(await seal(env.SECRET_KEY, body.token.trim()), secret ? await seal(env.SECRET_KEY, secret) : null, now, now)
    .run();
  return json(await readwiseStatus(env));
}

export async function disconnectReadwise(env: Env): Promise<Response> {
  await env.DB.prepare("DELETE FROM readwise WHERE id = 1").run();
  return json(await readwiseStatus(env));
}

/** POST /readwise/webhook. Readwise puts the webhook's secret in the body. */
export async function webhook(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  const event = await readJson<Document & { event_type?: string; secret?: string }>(request);
  const current = await link(env);
  if (!current?.webhook_secret || typeof event.secret !== "string" || !(await sameSecret(event.secret, current.webhook_secret))) {
    return json({ error: "Unknown webhook." }, 401);
  }
  if (event.event_type && ADDED.has(event.event_type) && event.category && CATEGORIES.has(event.category) && event.id) {
    ctx.waitUntil(receive(env, current.token, event.id));
  }
  return json({ ok: true });
}

/** The backstop for a webhook that never came: documents updated since the last poll. */
export async function poll(env: Env): Promise<void> {
  const current = await link(env);
  if (!current) return;
  const found: Document[] = [];
  for (const category of CATEGORIES) {
    const url = new URL(`${API}/v3/list/`);
    url.searchParams.set("category", category);
    url.searchParams.set("updatedAfter", current.since);
    const response = await fetch(url, { headers: { authorization: `Token ${current.token}` } });
    if (!response.ok) return;
    found.push(...((await response.json()) as { results: Document[] }).results);
  }
  // Oldest first, so the cursor only moves past what was handled. A document saved before the
  // link was made has only been read or moved in Reader, so it is passed over.
  // Readwise and this Worker write timestamps differently, so they are compared as times.
  const time = (t: string | undefined) => (t ? Date.parse(t) : NaN);
  found.sort((a, b) => (time(a.updated_at) || 0) - (time(b.updated_at) || 0));
  let since = current.since;
  let received = 0;
  for (const doc of found) {
    if (time(doc.saved_at) >= time(current.connected_at)) {
      if (received === MAX_PER_POLL) break;
      await receive(env, current.token, doc.id);
      received++;
    }
    if (time(doc.updated_at) > time(since)) since = doc.updated_at!;
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
  if (!listed.ok) return;
  const doc = ((await listed.json()) as { results: Document[] }).results[0];
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
