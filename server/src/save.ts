// Save from anywhere (TOB-554): takes a URL (and optionally HTML already in hand) from a
// bookmarklet, share sheet, or Shortcut, fetches the page if needed without parsing it, and stages
// it in `incoming` so the next syncing device parses it with `inkling-books` (TOB-534).

import { z } from "zod";
import type { Env } from "./env";
import { canonicalUrl, fetchPublicText, prepareArticleHtml, requirePublicUrl } from "./fetch";
import { MAX_ARTICLE_BYTES, Refused, escapeHtml, json, page, readJson, sha256Hex, tokenMatches, BOOK_ID } from "./http";

const HTML_ACCEPT = "text/html, application/xhtml+xml, application/xml;q=0.9, */*;q=0.8";
const HTML_TYPE = /^(text\/html|application\/xhtml\+xml|text\/plain)\b/u;

export const SaveInput = z.object({
  url: z.string().trim().min(1),
  html: z.string().optional(),
  title: z.string().trim().min(1).optional(),
});

export type SaveRequest = z.infer<typeof SaveInput>;

export interface SaveResult {
  id: string;
  url: string;
  status: "waiting" | "already_saved";
  created: boolean;
}

/** Stages one web page in `incoming` for the devices to parse into the library. */
export async function saveUrl(env: Env, input: SaveRequest): Promise<SaveResult> {
  const checked = requirePublicUrl(input.url);
  const canonical = canonicalUrl(checked.toString());
  const bookId = await sha256Hex(canonical);

  if (await env.DB.prepare("SELECT 1 FROM books WHERE id = ?").bind(bookId).first()) {
    return { id: bookId, url: canonical, status: "already_saved", created: false };
  }

  const dedupKey = `save:${bookId}`;
  const id = await sha256Hex(dedupKey);

  const existing = await env.DB.prepare(
    "SELECT id, url, state, book_id FROM incoming WHERE dedup_key = ? OR (url = ? AND state IN ('waiting', 'imported')) LIMIT 1",
  )
    .bind(dedupKey, canonical)
    .first<{ id: string; url: string; state: string; book_id: string | null }>();

  if (existing?.state === "waiting") {
    return { id: existing.id, url: existing.url, status: "waiting", created: false };
  }

  if (existing?.state === "imported") {
    return { id: existing.book_id ?? existing.id, url: existing.url, status: "already_saved", created: false };
  }

  let finalUrl = canonical;
  let html: string;

  if (input.html === undefined) {
    const fetched = await fetchPublicText(canonical, { accept: HTML_ACCEPT });

    if (fetched.status < 200 || fetched.status >= 300) {
      throw new Refused(502, `The page answered ${fetched.status}, so it couldn’t be saved.`);
    }

    if (fetched.contentType && !HTML_TYPE.test(fetched.contentType)) {
      throw new Refused(415, "That address doesn’t serve a web page.");
    }

    finalUrl = fetched.url;
    const finalBookId = await sha256Hex(finalUrl);

    if (finalBookId !== bookId && (await env.DB.prepare("SELECT 1 FROM books WHERE id = ?").bind(finalBookId).first())) {
      return { id: finalBookId, url: finalUrl, status: "already_saved", created: false };
    }

    html = input.title ? prepareArticleHtml(fetched.text, { title: input.title }) : fetched.text;
  } else {
    html = input.title ? prepareArticleHtml(input.html, { title: input.title }) : input.html;
  }

  const bytes = new TextEncoder().encode(html);

  if (bytes.byteLength > MAX_ARTICLE_BYTES) {
    throw new Refused(413, "That page is larger than 5 MB, which is as much as the server keeps.");
  }

  if (bytes.byteLength === 0) {
    throw new Refused(422, "That page was empty.");
  }

  await env.FILES.put(`incoming/${id}`, bytes);

  await env.DB.prepare(
    `INSERT INTO incoming (id, source, destination, url, title, dedup_key, state, bytes, received_at)
     VALUES (?, 'save', 'queue', ?, ?, ?, 'waiting', ?, ?)
     ON CONFLICT(id) DO UPDATE SET url = excluded.url, title = excluded.title, state = 'waiting',
       bytes = excluded.bytes, error = NULL, received_at = excluded.received_at`,
  )
    .bind(id, finalUrl, input.title ?? null, dedupKey, bytes.byteLength, Date.now())
    .run();

  return { id, url: finalUrl, status: "waiting", created: true };
}

const IncomingSettled = z.discriminatedUnion("state", [
  z.object({ state: z.literal("imported"), book_id: z.string().regex(BOOK_ID) }),
  z.object({ state: z.literal("failed"), error: z.string() }),
]);

/** A device says how parsing an incoming HTML article (save, feed or newsletter) went. */
export async function settleIncoming(request: Request, env: Env, id: string): Promise<Response> {
  const s = await readJson(request, IncomingSettled, "Say imported with the book’s id, or failed with why.");

  if (s.state === "imported") {
    await env.DB.prepare("UPDATE incoming SET state = 'imported', book_id = ?, error = NULL WHERE id = ?").bind(s.book_id, id).run();
  } else {
    await env.DB.prepare("UPDATE incoming SET state = 'failed', error = ? WHERE id = ?").bind(s.error.slice(0, 500), id).run();
  }

  await env.FILES.delete(`incoming/${id}`);

  return json({ ok: true });
}

/** `POST /v1/save`, called by a signed-in device with an OAuth `sync` token. */
export async function saveFromSync(request: Request, env: Env): Promise<Response> {
  const input = await readJson(request, SaveInput, "Send a JSON body with a web address in `url`.");
  const result = await saveUrl(env, input);

  return json({ id: result.id, url: result.url, status: result.status }, result.created ? 201 : 200);
}

/**
 * `POST /save` for a Shortcut, share sheet, or browser bookmarklet, authenticated with `SAVE_TOKEN`.
 * JSON requests send `Authorization: Bearer <SAVE_TOKEN>`; a bookmarklet may submit an
 * `application/x-www-form-urlencoded` form with `url` and `token` and gets a plain HTML page back.
 */
export async function serveSave(request: Request, env: Env): Promise<Response> {
  if (request.method !== "POST") {
    return new Response(null, { status: 405, headers: { allow: "POST", "cache-control": "no-store" } });
  }

  const contentType = (request.headers.get("content-type") ?? "").toLowerCase();
  const isForm = contentType.includes("application/x-www-form-urlencoded") || contentType.includes("multipart/form-data");
  const bearer = request.headers.get("authorization")?.match(/^Bearer (.+)$/u)?.[1];

  try {
    if (isForm) {
      const form = await request.formData();

      const field = (key: string): string | undefined => {
        const value = form.get(key);

        if (value === null || value instanceof File) return undefined;

        return value;
      };

      const formToken = field("token");
      const token = bearer ?? (formToken ? formToken.trim() : undefined);

      if (!(await tokenMatches(token, env.SAVE_TOKEN))) {
        throw new Refused(401, "That save token isn’t right.");
      }

      const rawUrl = field("url");
      const rawTitle = field("title");
      const rawHtml = field("html");

      const parsed = SaveInput.safeParse({
        url: rawUrl ?? "",
        title: rawTitle && rawTitle.trim() ? rawTitle : undefined,
        html: rawHtml && rawHtml.length > 0 ? rawHtml : undefined,
      });

      if (!parsed.success) throw new Refused(400, "Send a full web address to save.");
      const result = await saveUrl(env, parsed.data);
      const heading = result.status === "already_saved" ? "Already in Inkling" : "Saved to Inkling";

      const note =
        result.status === "already_saved"
          ? "This page is already in your library."
          : "It will appear in your library the next time Inkling syncs.";

      const headers = new Headers({ "cache-control": "no-store" });

      return page(
        heading,
        `<p>${note}</p><p class="note">${escapeHtml(result.url)}</p>`,
        result.created ? 201 : 200,
        headers,
      );
    }

    if (!(await tokenMatches(bearer, env.SAVE_TOKEN))) {
      throw new Refused(401, "That save token isn’t right.");
    }

    const input = await readJson(request, SaveInput, "Send a JSON body with a web address in `url`.");
    const result = await saveUrl(env, input);

    return json({ id: result.id, url: result.url, status: result.status }, result.created ? 201 : 200, { "cache-control": "no-store" });
  } catch (e) {
    if (e instanceof Refused) {
      if (isForm) {
        const headers = new Headers({ "cache-control": "no-store" });

        return page("Couldn’t save page", `<p>${escapeHtml(e.message)}</p>`, e.status, headers);
      }

      return json({ error: e.message }, e.status, { "cache-control": "no-store" });
    }

    throw e;
  }
}
