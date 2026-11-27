// Newsletters by Cloudflare Email Routing (TOB-554): mail addressed to `NEWSLETTER_ADDRESS`
// (default `read@tobygodat.com`) reaches the Worker's `email()` handler, is unpacked with
// postal-mime, and lands in `incoming` (`destination = 'inbox'`) for a syncing device to parse
// with `inkling-books`.

import PostalMime from "postal-mime";
import type { Env } from "./env";
import { plainTextToHtml, prepareArticleHtml } from "./fetch";
import { MAX_ARTICLE_BYTES, sha256Hex } from "./http";

export const DEFAULT_NEWSLETTER_ADDRESS = "read@tobygodat.com";
const MAX_EMAIL_BYTES = 10 * 1024 * 1024;

function splitAddress(address: string): [local: string, domain: string] | null {
  const trimmed = address.trim().toLowerCase();
  const at = trimmed.lastIndexOf("@");

  if (at <= 0 || at === trimmed.length - 1) return null;

  return [trimmed.slice(0, at), trimmed.slice(at + 1)];
}

/** Matches the configured address or a `+tag` variant at the same domain (e.g. `read+tech@tobygodat.com`). */
export function matchesNewsletterAddress(recipient: string, configured: string): boolean {
  const got = splitAddress(recipient);
  const want = splitAddress(configured);

  if (!got || !want) return false;

  if (got[1] !== want[1]) return false;
  const baseLocal = got[0].split("+")[0];

  return got[0] === want[0] || baseLocal === want[0];
}

function cleanName(name: string | undefined, address: string): string {
  const trimmed = (name ?? "").trim().replaceAll(/^["']|["']$/gu, "").trim();

  if (trimmed) return trimmed;
  const parts = splitAddress(address);

  return parts ? parts[1] : address;
}

function normalizeDate(raw: string | undefined): string {
  if (raw) {
    const parsed = Date.parse(raw);

    if (!Number.isNaN(parsed)) return new Date(parsed).toISOString().replace(/\.\d{3}Z$/u, "Z");
  }

  return new Date().toISOString().replace(/\.\d{3}Z$/u, "Z");
}

export async function receiveNewsletter(message: ForwardableEmailMessage, env: Env): Promise<void> {
  const configured = (env.NEWSLETTER_ADDRESS ?? DEFAULT_NEWSLETTER_ADDRESS).trim().toLowerCase();

  if (!matchesNewsletterAddress(message.to, configured)) {
    message.setReject("Unknown recipient.");

    return;
  }

  if (message.rawSize > MAX_EMAIL_BYTES) {
    message.setReject("That newsletter is larger than 10 MB.");

    return;
  }

  const raw = await new Response(message.raw).arrayBuffer();
  const parsed = await new PostalMime().parse(raw);
  const subject = (parsed.subject ?? "").trim() || "Untitled newsletter";
  const senderAddress = parsed.from?.address || message.from;
  const senderName = cleanName(parsed.from?.name, senderAddress);
  const published = normalizeDate(parsed.date);
  const htmlBody = (parsed.html ?? "").trim();
  const textBody = (parsed.text ?? "").trim();

  if (!htmlBody && !textBody) {
    return;
  }

  const messageId = (parsed.messageId ?? message.headers.get("message-id") ?? "").trim();
  const fingerprint = messageId || `${senderAddress.toLowerCase()}\n${subject}\n${published}\n${(htmlBody || textBody).slice(0, 1024)}`;
  const dedupKey = `newsletter:${await sha256Hex(fingerprint)}`;
  const existing = await env.DB.prepare("SELECT state FROM incoming WHERE dedup_key = ?").bind(dedupKey).first<{ state: string }>();

  if (existing && existing.state !== "failed") {
    return;
  }

  const id = await sha256Hex(dedupKey);
  const domain = splitAddress(configured)?.[1] ?? "tobygodat.com";
  const url = `https://${domain}/newsletters/${id}`;
  const meta = { title: subject, author: senderName, site: senderName, published };
  const articleHtml = htmlBody ? prepareArticleHtml(htmlBody, meta, { overrideHead: true }) : plainTextToHtml(textBody, meta);
  const bytes = new TextEncoder().encode(articleHtml);
  const now = Date.now();

  if (bytes.byteLength > MAX_ARTICLE_BYTES) {
    await env.DB.prepare(
      `INSERT INTO incoming (id, source, destination, url, title, author, site, published, dedup_key, state, bytes, error, received_at)
       VALUES (?, 'newsletter', 'inbox', ?, ?, ?, ?, ?, ?, 'too_large', ?, ?, ?)
       ON CONFLICT(id) DO UPDATE SET state = 'too_large', bytes = excluded.bytes, error = excluded.error, received_at = excluded.received_at`,
    )
      .bind(
        id,
        url,
        subject,
        senderName,
        senderName,
        published,
        dedupKey,
        bytes.byteLength,
        "That newsletter is larger than 5 MB, which is as much as the server keeps.",
        now,
      )
      .run();

    return;
  }

  await env.FILES.put(`incoming/${id}`, bytes);

  await env.DB.prepare(
    `INSERT INTO incoming (id, source, destination, url, title, author, site, published, dedup_key, state, bytes, received_at)
     VALUES (?, 'newsletter', 'inbox', ?, ?, ?, ?, ?, ?, 'waiting', ?, ?)
     ON CONFLICT(id) DO UPDATE SET url = excluded.url, title = excluded.title, author = excluded.author, site = excluded.site,
       published = excluded.published, state = 'waiting', bytes = excluded.bytes, error = NULL, received_at = excluded.received_at`,
  )
    .bind(id, url, subject, senderName, senderName, published, dedupKey, bytes.byteLength, now)
    .run();
}
