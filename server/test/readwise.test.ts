import { env } from "cloudflare:workers";
import { afterEach, describe, expect, it, vi } from "vitest";
import { poll } from "../src/readwise";
import { bearer, call, signIn } from "./signin";

const TOKEN = "readwise-token";
const SECRET = "webhook-secret-from-readwise";

interface Doc {
  id: string;
  title: string;
  category: string;
  saved_at: string;
  updated_at: string;
  bytes?: number;
}

/** Readwise's API and its S3 links, answering from `docs`. */
function fakeReadwise(docs: Doc[]) {
  const real = globalThis.fetch;
  return vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
    const url = new URL(input instanceof Request ? input.url : String(input));
    const auth = new Headers(init?.headers ?? (input instanceof Request ? input.headers : undefined)).get("authorization");
    if (url.hostname === "readwise.io") {
      if (auth !== `Token ${TOKEN}`) return new Response(null, { status: 401 });
      if (url.pathname === "/api/v2/auth/") return new Response(null, { status: 204 });
      const id = url.searchParams.get("id");
      const category = url.searchParams.get("category");
      const after = url.searchParams.get("updatedAfter");
      const results = docs
        .filter((d) => (id ? d.id === id : d.category === category && Date.parse(d.updated_at) > Date.parse(after ?? "")))
        .map((d) => ({ ...d, raw_source_url: url.searchParams.get("withRawSourceUrl") ? `https://s3.example/${d.id}` : undefined }));
      return Response.json({ count: results.length, nextPageCursor: null, results });
    }
    if (url.hostname === "s3.example") {
      const doc = docs.find((d) => url.pathname === `/${d.id}`)!;
      const bytes = doc.bytes ?? 11;
      return new Response(bytes > 1000 ? "x" : "epub-bytes!", { headers: { "content-length": String(bytes) } });
    }
    return real(input, init);
  });
}

afterEach(() => vi.restoreAllMocks());

const hook = (body: Record<string, unknown>) =>
  call("/readwise/webhook", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });

describe("Readwise", () => {
  it("takes uploads from the webhook into the inbox, and a device settles them", async () => {
    const { token } = await signIn({ scope: "sync", tick: ["sync"], resource: "http://localhost/v1" });
    const later = new Date(Date.now() + 60_000).toISOString();
    const docs: Doc[] = [
      { id: "epub1", title: "Mrs Dalloway", category: "epub", saved_at: later, updated_at: later },
      { id: "huge1", title: "An Atlas", category: "pdf", saved_at: later, updated_at: later, bytes: 30 * 1024 * 1024 },
    ];
    fakeReadwise(docs);

    const wrong = await call("/v1/readwise", bearer(token!, { method: "PUT", body: JSON.stringify({ token: "nope" }) }));
    expect(wrong.status).toBe(400);
    const linked = await call("/v1/readwise", bearer(token!, { method: "PUT", body: JSON.stringify({ token: TOKEN, webhook_secret: SECRET }) }));
    expect(await linked.json()).toMatchObject({ connected: true, webhook: true, webhook_url: "http://localhost/readwise/webhook" });
    // The token is kept sealed, never as it was typed.
    const stored = await env.DB.prepare("SELECT token FROM readwise").first<{ token: string }>();
    expect(stored!.token).not.toContain(TOKEN);

    expect((await hook({ event_type: "reader.non_feed_document.created", id: "epub1", category: "epub", secret: "guess" })).status).toBe(401);
    for (const id of ["epub1", "huge1"]) {
      const doc = docs.find((d) => d.id === id)!;
      expect((await hook({ event_type: "reader.non_feed_document.created", id, category: doc.category, secret: SECRET })).status).toBe(200);
    }
    await vi.waitFor(async () => {
      const library = await (await call("/v1/library", bearer(token!))).json<{ inbox: { readwise_id: string; state: string }[] }>();
      expect(Object.fromEntries(library.inbox.map((i) => [i.readwise_id, i.state]))).toEqual({ epub1: "waiting", huge1: "too_large" });
    });

    expect(await (await call("/v1/inbox/epub1/file", bearer(token!))).text()).toBe("epub-bytes!");
    const settled = await call("/v1/inbox/epub1", bearer(token!, { method: "POST", body: JSON.stringify({ state: "imported", book_id: "cd".repeat(32) }) }));
    expect(settled.status).toBe(200);
    expect((await call("/v1/inbox/epub1/file", bearer(token!))).status).toBe(404);
  });

  it("polls for what the webhook missed, passing over documents saved before the link", async () => {
    const before = new Date(Date.now() - 86_400_000).toISOString();
    const after = new Date(Date.now() + 120_000).toISOString();
    fakeReadwise([
      { id: "old1", title: "Read again", category: "epub", saved_at: before, updated_at: after },
      { id: "new2", title: "Orlando", category: "epub", saved_at: after, updated_at: after },
    ]);
    await poll(env);
    const ids = (await env.DB.prepare("SELECT readwise_id FROM inbox").all<{ readwise_id: string }>()).results.map((r) => r.readwise_id);
    expect(ids).toContain("new2");
    expect(ids).not.toContain("old1");
    const since = await env.DB.prepare("SELECT since FROM readwise").first<{ since: string }>();
    expect(since!.since).toBe(after); // Past both, so neither is looked at again.
  });
});
