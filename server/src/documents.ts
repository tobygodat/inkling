// Document sync under `/v1/documents` (ADR 0004, TOB-490): each document carries a version counted
// up on every write the server takes, and a write names the version it was made on (`base`).

import { z } from "zod";
import type { Env } from "./env";
import { json, readJson, whole } from "./http";

interface DocumentRow {
  id: string;
  text: string;
  saved_at: number;
  device: string;
  version: number;
  deleted: number;
}

const shown = (d: DocumentRow) => ({ ...d, deleted: d.deleted === 1 });

/** Every document, deleted ones too, without their text. */
export async function listDocuments(env: Env): Promise<Response> {
  const { results } = await env.DB.prepare("SELECT id, version, saved_at, device, deleted FROM documents ORDER BY id").all<DocumentRow>();

  return json(results.map((d) => shown(d)));
}

export async function getDocument(env: Env, id: string): Promise<Response> {
  const doc = await env.DB.prepare("SELECT id, text, saved_at, device, version, deleted FROM documents WHERE id = ?").bind(id).first<DocumentRow>();

  return doc ? json(shown(doc)) : json({ error: "Not found." }, 404);
}

const DocumentWrite = z.object({
  text: z.string(),
  saved_at: whole,
  device: z.string(),
  base: whole,
  deleted: z.boolean().optional(),
});

/**
 * Takes a write made on `base`, the version the device last had: 0 for a document the server
 * hasn't seen. Any other base is refused with 409 and what the server has. A delete keeps the
 * last text.
 */
export async function putDocument(request: Request, env: Env, id: string): Promise<Response> {
  const d = await readJson(request, DocumentWrite, "A document needs its text, when it was saved, the device and the version it was made on.");
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
