import { env } from "cloudflare:workers";
import { afterEach, describe, expect, it, vi } from "vitest";
import { z } from "zod";
import { forwardPending } from "../src/feedback";
import { ORIGIN, bearer, call, signIn } from "./signin";

const WEBHOOK = "https://webhook.example/inkling";
const CLIP = new TextEncoder().encode("ftypmp42 stand-in clip bytes");

let n = 0;

/** A fresh id per call, so no test sees another's records. */
const newId = () => `${1_794_460_000 + n++}-abcdefgh`;

const TEN_SECONDS = { seconds: 10, touches: 42 };

const record = (id: string, clip: typeof TEN_SECONDS | null = TEN_SECONDS) => ({
  id,
  text: "The handle covers the reader's scroll bar.",
  sent_at: "2026-11-28T06:30:00Z",
  form_factor: "phone",
  window: { width: 402, height: 874 },
  screen: "reader",
  place: "Inbox › An Apology for Idlers, chapter 1 of 1, 34%",
  version: "0.1.0",
  build: "20261128.0630",
  bundle: "com.tobygodat.inkling.dev",
  clip,
});

const Notice = z.looseObject({ kind: z.literal("inkling.feedback"), id: z.string(), received_at: z.iso.datetime(), clip_url: z.string().nullable() });

/** The webhook, answering `status`, with every notice it was sent. */
function fakeWebhook(status = 204) {
  const real = globalThis.fetch;
  const notices: z.infer<typeof Notice>[] = [];

  vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
    const url = input instanceof Request ? input.url : String(input);

    if (url !== WEBHOOK) return real(input, init);
    notices.push(Notice.parse(JSON.parse(String(init?.body))));

    return new Response(null, { status });
  });

  return notices;
}

async function device() {
  const { token } = await signIn({ scope: "sync", tick: ["sync"], resource: `${ORIGIN}/v1` });

  return token!;
}

const putClip = (token: string, id: string) =>
  call(`/v1/feedback/${id}/clip`, { method: "PUT", headers: { authorization: `Bearer ${token}`, "content-type": "video/mp4" }, body: CLIP });

const send = (token: string, body: ReturnType<typeof record>) => call("/v1/feedback", bearer(token, { method: "POST", body: JSON.stringify(body) }));

afterEach(() => vi.restoreAllMocks());

describe("feedback", () => {
  it("keeps the record and the clip, and posts the Captain a notice whose link plays the clip", async () => {
    const token = await device();
    const notices = fakeWebhook();
    const id = newId();

    expect((await putClip(token, id)).status).toBe(200);
    const sent = await send(token, record(id));
    expect(sent.status).toBe(201);
    expect(await sent.json()).toEqual({ id, forwarded: true });

    expect(notices).toHaveLength(1);
    const notice = notices[0]!;
    expect(notice).toMatchObject({ kind: "inkling.feedback", ...record(id) });
    const clipUrl = z.string().parse(notice.clip_url);

    const clip = await call(clipUrl.slice(ORIGIN.length));
    expect(clip.status).toBe(200);
    expect(clip.headers.get("content-type")).toBe("video/mp4");
    expect(new Uint8Array(await clip.arrayBuffer())).toEqual(CLIP);

    const tampered = new URL(clipUrl);
    tampered.pathname = `/feedback/${newId()}/clip.mp4`;
    expect((await call(tampered.pathname + tampered.search)).status).toBe(403);
    expect((await call(`/feedback/${id}/clip.mp4`)).status).toBe(403);

    const stored = await env.FEEDBACK.get(`${id}/record.json`);
    expect(await stored?.json()).toMatchObject(record(id));
  });

  it("takes feedback only from a signed-in device, with its clip first", async () => {
    const token = await device();
    fakeWebhook();
    const id = newId();

    expect((await call("/v1/feedback", { method: "POST", body: JSON.stringify(record(id)) })).status).toBe(401);
    expect((await send(token, record(id))).status).toBe(409);
    expect((await send(token, { ...record(id), id: "not-an-id" })).status).toBe(400);
    expect((await send(token, { ...record(id), text: "  " })).status).toBe(400);
    expect((await putClip(token, "not-an-id")).status).toBe(400);
    expect((await send(token, record(id, null))).status).toBe(201);
  });

  it("posts one notice for an id sent twice, and takes no clip after its record", async () => {
    const token = await device();
    const notices = fakeWebhook();
    const id = newId();

    expect((await send(token, record(id, null))).status).toBe(201);
    const again = await send(token, { ...record(id, null), text: "Something else" });
    expect(again.status).toBe(200);
    expect(await again.json()).toEqual({ id, forwarded: true });
    expect(notices).toHaveLength(1);
    expect((await putClip(token, id)).status).toBe(409);
  });

  it("keeps a notice the webhook turned down, and the cron posts it once", async () => {
    const token = await device();
    const down = fakeWebhook(503);
    const id = newId();

    const sent = await send(token, record(id, null));
    expect(sent.status).toBe(201);
    expect(await sent.json()).toEqual({ id, forwarded: false });
    expect(down).toHaveLength(1);
    vi.restoreAllMocks();

    const up = fakeWebhook();
    await forwardPending(env);
    expect(up.filter((notice) => notice.id === id)).toHaveLength(1);
    await forwardPending(env);
    expect(up.filter((notice) => notice.id === id)).toHaveLength(1);
  });
});
