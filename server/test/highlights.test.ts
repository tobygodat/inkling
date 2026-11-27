import { env } from "cloudflare:workers";
import { afterEach, describe, expect, it, vi } from "vitest";
import { z } from "zod";
import { pushHighlightsToReadwise } from "../src/highlights";
import { poll } from "../src/readwise";
import { bearer, call, signIn } from "./signin";

const READWISE_TOKEN = "secret-readwise-token";
const BOOK_ID = "cd".repeat(32);

const SentPayload = z.object({
  text: z.string(),
  title: z.string(),
  author: z.string().optional(),
  source_url: z.string().optional(),
  source_type: z.string(),
  category: z.string(),
  note: z.string().optional(),
  location: z.number(),
  location_type: z.string(),
  highlighted_at: z.string(),
});

const ReadwiseBatch = z.object({ highlights: z.array(SentPayload) });

async function syncToken() {
  const { token } = await signIn({ scope: "sync offline_access", tick: ["sync"], resource: "http://localhost/v1" });

  return token!;
}

async function seedArticle(token: string) {
  expect((await call(`/v1/books/${BOOK_ID}/file`, bearer(token, { method: "PUT", body: '{"parsed":true}' }))).status).toBe(200);
  expect((await call(`/v1/books/${BOOK_ID}/chapters/0`, bearer(token, { method: "PUT", body: JSON.stringify({ title: "Tide", text: "# Tide\n\nSalt." }) }))).status).toBe(200);
  expect(
    (
      await call(
        `/v1/books/${BOOK_ID}`,
        bearer(token, {
          method: "PUT",
          body: JSON.stringify({
            title: "Notes on the tide",
            author: "Ana Lima",
            format: "Article",
            chapters: 1,
            added: 100,
            source_url: "https://shore.example/tide",
          }),
        }),
      )
    ).status,
  ).toBe(200);
}

afterEach(() => vi.restoreAllMocks());

describe("highlights out to Readwise", () => {
  it("exports each highlight once using the server-side token and retries after transient Readwise errors", async () => {
    const token = await syncToken();
    await seedArticle(token);

    const sent: z.infer<typeof SentPayload>[] = [];
    let failHighlights = false;
    const real = globalThis.fetch;

    vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
      const url = new URL(input instanceof Request ? input.url : String(input));
      const headers = new Headers(init?.headers ?? (input instanceof Request ? input.headers : undefined));

      if (url.hostname === "readwise.io") {
        if (headers.get("authorization") !== `Token ${READWISE_TOKEN}`) return new Response(null, { status: 401 });

        if (url.pathname === "/api/v2/auth/") return new Response(null, { status: 204 });

        if (url.pathname === "/api/v2/highlights/" && (init?.method ?? "GET") === "POST") {
          if (failHighlights) return new Response("busy", { status: 503 });
          const body = ReadwiseBatch.parse(JSON.parse(String(init?.body)));
          sent.push(...body.highlights);

          return Response.json([{ id: 1, num_highlights: body.highlights.length, modified_highlights: [101] }]);
        }

        if (url.pathname === "/api/v3/list/") {
          return Response.json({ count: 0, nextPageCursor: null, results: [] });
        }
      }

      return real(input, init);
    });

    // Connect Readwise; token is kept encrypted in D1 and never returned by any endpoint.
    const linked = await call("/v1/readwise", bearer(token, { method: "PUT", body: JSON.stringify({ token: READWISE_TOKEN }) }));
    expect(linked.status).toBe(200);
    const statusText = await (await call("/v1/readwise", bearer(token))).text();
    expect(statusText).not.toContain(READWISE_TOKEN);

    const h1 = { chapter: 0, start: 10, end: 42, words: "The tide came in slowly.", note: "Great opener", made: 1_700_000_000_000 };

    const first = await call(
      `/v1/books/${BOOK_ID}/highlights`,
      bearer(token, { method: "POST", body: JSON.stringify({ highlights: [h1] }) }),
    );

    expect(first.status).toBe(200);
    expect(await first.json()).toEqual({ ok: true, added: 1, readwise_sent: 1 });
    expect(sent).toEqual([
      {
        text: "The tide came in slowly.",
        title: "Notes on the tide",
        author: "Ana Lima",
        source_url: "https://shore.example/tide",
        source_type: "inkling",
        category: "articles",
        note: "Great opener",
        location: 10,
        location_type: "order",
        highlighted_at: "2023-11-14T22:13:20Z",
      },
    ]);

    // Syncing the same highlight from a second device and running cron sends nothing again.
    const second = await call(
      `/v1/books/${BOOK_ID}/highlights`,
      bearer(token, { method: "POST", body: JSON.stringify({ highlights: [h1] }) }),
    );

    expect(await second.json()).toEqual({ ok: true, added: 0, readwise_sent: 0 });
    expect(await pushHighlightsToReadwise(env)).toEqual({ sent: 0 });
    expect(sent).toHaveLength(1);

    // If Readwise is temporarily down when a second highlight syncs, it stays unsent until retry.
    failHighlights = true;
    const h2 = { chapter: 0, start: 50, end: 80, words: "Birds lifted at dawn.", made: 1_700_000_060_000 };

    const queued = await call(
      `/v1/books/${BOOK_ID}/highlights`,
      bearer(token, { method: "POST", body: JSON.stringify({ highlights: [h2] }) }),
    );

    expect(await queued.json()).toEqual({ ok: true, added: 1, readwise_sent: 0 });
    expect(sent).toHaveLength(1);

    const listedBefore = await (await call("/v1/highlights", bearer(token))).json<{ highlights: { words: string; readwise_sent: boolean }[] }>();
    expect(listedBefore.highlights.map((h) => [h.words, h.readwise_sent])).toEqual([
      ["The tide came in slowly.", true],
      ["Birds lifted at dawn.", false],
    ]);

    // Next poll retries and sends only the unsent highlight, once.
    failHighlights = false;
    await poll(env);
    expect(sent).toHaveLength(2);
    expect(sent[1]?.text).toBe("Birds lifted at dawn.");

    // A follow-up poll sends nothing more.
    await poll(env);
    expect(sent).toHaveLength(2);
  });
});
