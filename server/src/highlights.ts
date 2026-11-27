// Highlights sync and one-time export to Readwise (TOB-554). Highlights made on any device sync
// through `/v1/books/:id/highlights` and `/v1/highlights` with a content-derived SHA-256 id, and
// each unsent highlight is pushed once to Readwise using the server-side sealed token.

import { z } from "zod";
import type { Env } from "./env";
import { json, readJson, sha256Hex, whole } from "./http";
import { readwiseToken } from "./readwise";

const READWISE_HIGHLIGHTS_URL = "https://readwise.io/api/v2/highlights/";
const HIGHLIGHTS_BATCH = 50;
const MAX_HIGHLIGHT_BATCHES = 5;
const CLAIM_TTL_MS = 5 * 60 * 1000;

interface HighlightRow {
  id: string;
  book_id: string;
  chapter: number;
  start_byte: number;
  end_byte: number;
  words: string;
  note: string | null;
  made: number;
  readwise_sent_at: number | null;
}

interface UnsentHighlight {
  id: string;
  book_id: string;
  chapter: number;
  start_byte: number;
  words: string;
  note: string | null;
  made: number;
  title: string | null;
  author: string | null;
  format: "Epub" | "Pdf" | "Article" | null;
  source_url: string | null;
}

interface ReadwiseHighlight {
  text: string;
  title: string;
  author?: string;
  source_url?: string;
  source_type: string;
  category: "books" | "articles";
  note?: string;
  location: number;
  location_type: "order";
  highlighted_at: string;
}

const formatHighlight = (h: HighlightRow) => ({
  id: h.id,
  book_id: h.book_id,
  chapter: h.chapter,
  start: h.start_byte,
  end: h.end_byte,
  words: h.words,
  note: h.note,
  made: h.made,
  readwise_sent: h.readwise_sent_at !== null,
});

export async function listHighlights(env: Env): Promise<Response> {
  const { results } = await env.DB.prepare(
    "SELECT id, book_id, chapter, start_byte, end_byte, words, note, made, readwise_sent_at FROM highlights ORDER BY book_id, chapter, start_byte, made",
  ).all<HighlightRow>();

  return json({ highlights: results.map((h) => formatHighlight(h)) });
}

export async function listBookHighlights(env: Env, id: string): Promise<Response> {
  const { results } = await env.DB.prepare(
    "SELECT id, book_id, chapter, start_byte, end_byte, words, note, made, readwise_sent_at FROM highlights WHERE book_id = ? ORDER BY chapter, start_byte, made",
  )
    .bind(id)
    .all<HighlightRow>();

  return json({ highlights: results.map((h) => formatHighlight(h)) });
}

const HighlightInput = z.object({
  chapter: whole,
  start: whole,
  end: whole,
  words: z.string().min(1),
  note: z.string().nullish(),
  made: whole,
});

const HighlightsBatch = z.object({
  highlights: z.array(HighlightInput).max(500),
});

/**
 * Records highlights from a device, deduplicating by content and location so a highlight synced
 * from several devices is stored once, then exports unsent highlights to Readwise if connected.
 */
export async function upsertHighlights(request: Request, env: Env, id: string): Promise<Response> {
  const { highlights } = await readJson(request, HighlightsBatch, "Send `{ highlights: [...] }` with each highlight’s chapter, start, end, words and made.");
  let added = 0;

  for (const h of highlights) {
    const highlightId = await sha256Hex(`${id}\n${h.chapter}\n${h.start}\n${h.end}\n${h.words}`);

    const { meta } = await env.DB.prepare(
      `INSERT INTO highlights (id, book_id, chapter, start_byte, end_byte, words, note, made)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT(id) DO NOTHING`,
    )
      .bind(highlightId, id, h.chapter, h.start, h.end, h.words, h.note ?? null, h.made)
      .run();

    if (meta.changes > 0) {
      added++;
    } else if (h.note) {
      await env.DB.prepare("UPDATE highlights SET note = ? WHERE id = ? AND note IS NULL").bind(h.note, highlightId).run();
    }
  }

  let readwiseSent = 0;

  try {
    readwiseSent = (await pushHighlightsToReadwise(env)).sent;
  } catch {
    // Unsent highlights stay with readwise_sent_at = NULL for the next sync or cron.
  }

  return json({ ok: true, added, readwise_sent: readwiseSent });
}

function toReadwiseHighlight(row: UnsentHighlight): ReadwiseHighlight {
  const payload: ReadwiseHighlight = {
    text: row.words.slice(0, 8191),
    title: (row.title ?? "Inkling").slice(0, 511),
    source_type: "inkling",
    category: row.format === "Article" ? "articles" : "books",
    location: row.chapter * 1_000_000 + row.start_byte,
    location_type: "order",
    highlighted_at: new Date(row.made).toISOString().replace(/\.\d{3}Z$/u, "Z"),
  };

  if (row.author) payload.author = row.author.slice(0, 1024);

  if (row.source_url && /^https?:\/\//u.test(row.source_url)) {
    payload.source_url = row.source_url.slice(0, 2047);
  }

  if (row.note) payload.note = row.note.slice(0, 4095);

  return payload;
}

/**
 * Sends highlights not yet exported to Readwise, once each, using the sealed token kept only on
 * the server. Rows are claimed atomically before the HTTP call so concurrent syncs or crons can't
 * send the same highlight twice, and a failed request releases its claim for the next retry.
 */
export async function pushHighlightsToReadwise(env: Env): Promise<{ sent: number }> {
  const token = await readwiseToken(env);

  if (!token) return { sent: 0 };
  let sent = 0;

  for (let batch = 0; batch < MAX_HIGHLIGHT_BATCHES; batch++) {
    const now = Date.now();
    const claim = now * 1000 + Math.floor(Math.random() * 1000);
    const staleBefore = (now - CLAIM_TTL_MS) * 1000;

    await env.DB.prepare(
      `UPDATE highlights
       SET readwise_sending = ?
       WHERE id IN (
         SELECT id FROM highlights
         WHERE readwise_sent_at IS NULL AND (readwise_sending IS NULL OR readwise_sending < ?)
         ORDER BY made ASC, id ASC
         LIMIT ?
       )
       AND readwise_sent_at IS NULL
       AND (readwise_sending IS NULL OR readwise_sending < ?)`,
    )
      .bind(claim, staleBefore, HIGHLIGHTS_BATCH, staleBefore)
      .run();

    const { results: rows } = await env.DB.prepare(
      `SELECT h.id, h.book_id, h.chapter, h.start_byte, h.words, h.note, h.made,
              b.title, b.author, b.format, b.source_url
       FROM highlights h
       LEFT JOIN books b ON b.id = h.book_id
       WHERE h.readwise_sending = ?
       ORDER BY h.made ASC, h.id ASC`,
    )
      .bind(claim)
      .all<UnsentHighlight>();

    if (rows.length === 0) break;

    let ok = false;

    try {
      const response = await fetch(READWISE_HIGHLIGHTS_URL, {
        method: "POST",
        headers: { authorization: `Token ${token}`, "content-type": "application/json" },
        body: JSON.stringify({ highlights: rows.map((row) => toReadwiseHighlight(row)) }),
      });

      ok = response.ok;
    } catch {
      ok = false;
    }

    if (!ok) {
      await env.DB.prepare("UPDATE highlights SET readwise_sending = NULL WHERE readwise_sending = ?").bind(claim).run();
      break;
    }

    await env.DB.prepare("UPDATE highlights SET readwise_sent_at = ?, readwise_sending = NULL WHERE readwise_sending = ?")
      .bind(Date.now(), claim)
      .run();
    sent += rows.length;

    if (rows.length < HIGHLIGHTS_BATCH) break;
  }

  return { sent };
}
