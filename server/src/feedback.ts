// Feedback from the native app's dev builds (TOB-571). Signed in, a device uploads the clip of its
// last 10 seconds, when one is attached, then the record. The Worker keeps both in R2 and posts a
// notice to FEEDBACK_WEBHOOK_URL, the inkling Captain's webhook, with a signed link to the clip.
// A notice that doesn't go through is posted again by the 30-minute cron, so a record sent before
// the webhook is set reaches the Captain once it is.

import { z } from "zod";
import { signUrl, verify } from "./builds";
import type { Env } from "./env";
import { MAX_FILE_BYTES, Refused, json, readJson, whole } from "./http";

/** `<unix seconds>-<8 chars of a-z and 2-7>`, as `FeedbackId` in core/src/feedback/record.rs. */
const FEEDBACK_ID = /^[0-9]{1,20}-[a-z2-7]{8}$/u;
const CLIP_ROUTE = /^\/feedback\/([0-9]{1,20}-[a-z2-7]{8})\/clip\.mp4$/u;
const LINK_DAYS = 7;

/** The most notices one cron run posts again. */
const RETRIES = 20;

const recordKey = (id: string) => `${id}/record.json`;
const clipKey = (id: string) => `${id}/clip.mp4`;

const short = (most: number) => z.string().max(most);

const Feedback = z.object({
  id: z.string().regex(FEEDBACK_ID),
  text: z.string().trim().min(1).max(2000),
  sent_at: short(40),
  form_factor: z.enum(["phone", "mac"]),
  window: z.object({ width: whole, height: whole }),
  screen: short(40),
  place: short(300),
  version: short(32),
  build: short(32),
  bundle: short(100),
  clip: z.object({ seconds: z.number().min(0).max(11), touches: whole }).nullable(),
});

type Stored = z.infer<typeof Feedback> & { received_at: string };

export const feedbackId = (id: string) => {
  if (!FEEDBACK_ID.test(id)) throw new Refused(400, "That isn’t a feedback id.");

  return id;
};

/** `PUT /v1/feedback/<id>/clip`: the clip, an MP4, before its record. */
export async function putClip(request: Request, env: Env, id: string): Promise<Response> {
  const declared = Number(request.headers.get("content-length") ?? NaN);

  if (!(declared > 0)) throw new Refused(411, "Send the clip with its length.");

  if (declared > MAX_FILE_BYTES) throw new Refused(413, "That clip is larger than 25 MB.");

  if (await env.FEEDBACK.head(recordKey(id))) throw new Refused(409, "That feedback is already sent.");

  if (!request.body) throw new Refused(400, "Send the clip as the request body.");
  await env.FEEDBACK.put(clipKey(id), request.body, { httpMetadata: { contentType: "video/mp4" } });

  return json({ ok: true });
}

/** `POST /v1/feedback`: the record. Sending the same id again changes nothing and posts nothing. */
export async function postFeedback(request: Request, env: Env): Promise<Response> {
  const feedback = await readJson(request, Feedback, "That feedback is incomplete.");
  const kept = await env.FEEDBACK.head(recordKey(feedback.id));

  if (kept) return json({ id: feedback.id, forwarded: kept.customMetadata?.forwarded !== "no" });

  if (feedback.clip && !(await env.FEEDBACK.head(clipKey(feedback.id)))) throw new Refused(409, "Upload the clip first.");
  const stored: Stored = { ...feedback, received_at: new Date().toISOString() };

  await keep(env, stored, "no");

  return json({ id: feedback.id, forwarded: await forward(env, stored) }, 201);
}

const keep = (env: Env, stored: Stored, forwarded: string) =>
  env.FEEDBACK.put(recordKey(stored.id), JSON.stringify(stored), {
    httpMetadata: { contentType: "application/json" },
    customMetadata: { forwarded },
  });

/** Posts the notice, and marks the record forwarded once the webhook takes it. */
async function forward(env: Env, stored: Stored): Promise<boolean> {
  if (!env.FEEDBACK_WEBHOOK_URL) return false;
  const expires = Math.floor(Date.now() / 1000) + LINK_DAYS * 24 * 60 * 60;
  const clip_url = stored.clip ? await signUrl(env, `/feedback/${stored.id}/clip.mp4`, expires) : null;

  try {
    const answer = await fetch(env.FEEDBACK_WEBHOOK_URL, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ kind: "inkling.feedback", ...stored, clip_url }),
      redirect: "manual",
    });

    await answer.body?.cancel();

    if (!answer.ok) return false;
  } catch {
    return false;
  }

  await keep(env, stored, String(Date.now()));

  return true;
}

/** The cron's pass: every record whose notice hasn't gone through yet, oldest first. */
export async function forwardPending(env: Env): Promise<void> {
  if (!env.FEEDBACK_WEBHOOK_URL) return;
  const listed = await env.FEEDBACK.list({ include: ["customMetadata"] });

  const pending = listed.objects
    .filter((o) => o.key.endsWith("/record.json") && o.customMetadata?.forwarded === "no")
    .toSorted((a, b) => a.key.localeCompare(b.key))
    .slice(0, RETRIES);

  for (const o of pending) {
    const object = await env.FEEDBACK.get(o.key);

    if (object) await forward(env, await object.json<Stored>());
  }
}

const gone = () => new Refused(404, "That clip is gone.");
const headers = (size: number) => ({ "content-type": "video/mp4", "content-length": String(size), "cache-control": "no-store" });

/** `GET /feedback/<id>/clip.mp4`, behind the signed link the notice carries. */
export async function serveClip(request: Request, env: Env): Promise<Response> {
  const url = new URL(request.url);
  const id = url.pathname.match(CLIP_ROUTE)?.[1];

  if (id === undefined || (request.method !== "GET" && request.method !== "HEAD")) throw new Refused(404, "There’s nothing here.");
  await verify(env, url, "That link has expired.");

  if (request.method === "HEAD") {
    const object = await env.FEEDBACK.head(clipKey(id));

    if (!object) throw gone();

    return new Response(null, { headers: headers(object.size) });
  }

  const object = await env.FEEDBACK.get(clipKey(id));

  if (!object) throw gone();

  return new Response(object.body, { headers: headers(object.size) });
}
