import { afterEach, describe, expect, it, vi } from "vitest";
import { sha256Hex } from "../src/http";
import { bearer, call, signIn, tool } from "./signin";

const SAVE_TOKEN = "test-save-token";

const ARTICLE_HTML = `<!doctype html><html><head><title>Notes on the tide</title></head><body><article><h1>Notes on the tide</h1><p>The tide came in slowly over the flats.</p></article></body></html>`;

async function syncToken() {
  const { token } = await signIn({ scope: "sync offline_access", tick: ["sync"], resource: "http://localhost/v1" });

  return token!;
}

function fakePages(pages: Record<string, () => Response>) {
  const real = globalThis.fetch;

  return vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
    const url = input instanceof Request ? input.url : String(input);
    const hit = pages[url];

    if (hit) return hit();

    return real(input, init);
  });
}

afterEach(() => vi.restoreAllMocks());

describe("save from anywhere", () => {
  it("refuses missing or wrong tokens, private addresses, and non-HTML pages", async () => {
    const noAuth = await call("/save", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ url: "https://shore.example/a" }) });
    expect(noAuth.status).toBe(401);
    expect(noAuth.headers.get("cache-control")).toBe("no-store");

    const wrong = await call("/save", {
      method: "POST",
      headers: { authorization: "Bearer nope", "content-type": "application/json" },
      body: JSON.stringify({ url: "https://shore.example/a" }),
    });

    expect(wrong.status).toBe(401);

    for (const bad of ["http://127.0.0.1/secret", "http://localhost:8787/", "http://169.254.169.254/latest", "http://[::ffff:127.0.0.1]/", "file:///etc/passwd"]) {
      const res = await call("/save", {
        method: "POST",
        headers: { authorization: `Bearer ${SAVE_TOKEN}`, "content-type": "application/json" },
        body: JSON.stringify({ url: bad }),
      });

      expect(res.status).toBe(400);
    }

    fakePages({
      "https://fda.gov/news": () => new Response(ARTICLE_HTML, { headers: { "content-type": "text/html" } }),
      "https://shore.example/pic.png": () => new Response("png", { headers: { "content-type": "image/png" } }),
    });

    // Public domains starting with `fc`/`fd` (such as fda.gov) must not be mistaken for IPv6 ULA.
    const fda = await call("/save", {
      method: "POST",
      headers: { authorization: `Bearer ${SAVE_TOKEN}`, "content-type": "application/json" },
      body: JSON.stringify({ url: "https://fda.gov/news" }),
    });

    expect(fda.status).toBe(201);

    const binary = await call("/save", {
      method: "POST",
      headers: { authorization: `Bearer ${SAVE_TOKEN}`, "content-type": "application/json" },
      body: JSON.stringify({ url: "https://shore.example/pic.png" }),
    });

    expect(binary.status).toBe(415);
  });

  it("saves a URL via Bearer token, follows redirects, deduplicates, and flows through sync into the library", async () => {
    let fetches = 0;

    fakePages({
      "https://short.example/tide": () => {
        fetches++;

        return new Response(null, { status: 302, headers: { location: "https://shore.example/2026/tide#top" } });
      },
      "https://shore.example/2026/tide": () => {
        fetches++;

        return new Response(ARTICLE_HTML, { headers: { "content-type": "text/html; charset=utf-8" } });
      },
    });

    const saved = await call("/save", {
      method: "POST",
      headers: { authorization: `Bearer ${SAVE_TOKEN}`, "content-type": "application/json" },
      body: JSON.stringify({ url: "https://short.example/tide#frag" }),
    });

    expect(saved.status).toBe(201);
    expect(saved.headers.get("cache-control")).toBe("no-store");
    const first = await saved.json<{ id: string; url: string; status: string }>();
    expect(first).toMatchObject({ url: "https://shore.example/2026/tide", status: "waiting" });
    expect(fetches).toBe(2);

    // Saving the same URL again does not refetch or duplicate.
    const again = await call("/save", {
      method: "POST",
      headers: { authorization: `Bearer ${SAVE_TOKEN}`, "content-type": "application/json" },
      body: JSON.stringify({ url: "https://short.example/tide" }),
    });

    expect(again.status).toBe(200);
    expect(await again.json()).toEqual(first);
    expect(fetches).toBe(2);

    // Sync pulls the raw HTML, parses it on the device, settles incoming, and uploads the Article.
    const token = await syncToken();
    const library = await (await call("/v1/library", bearer(token))).json<{ incoming: { id: string; source: string; destination: string; state: string; url: string }[] }>();
    const staged = library.incoming.find((item) => item.id === first.id);
    expect(staged).toMatchObject({ id: first.id, source: "save", destination: "queue", state: "waiting", url: "https://shore.example/2026/tide" });

    const raw = await call(`/v1/incoming/${first.id}/file`, bearer(token));
    expect(raw.status).toBe(200);
    expect(await raw.text()).toBe(ARTICLE_HTML);

    const bookId = await sha256Hex("https://shore.example/2026/tide");
    expect((await call(`/v1/incoming/${first.id}`, bearer(token, { method: "POST", body: JSON.stringify({ state: "imported", book_id: bookId }) }))).status).toBe(200);
    expect((await call(`/v1/incoming/${first.id}/file`, bearer(token))).status).toBe(404);

    // Device uploads the parsed Article so other devices and MCP agents see it.
    expect((await call(`/v1/books/${bookId}/file`, bearer(token, { method: "PUT", body: '{"parsed":true}' }))).status).toBe(200);
    expect((await call(`/v1/books/${bookId}/chapters/0`, bearer(token, { method: "PUT", body: JSON.stringify({ title: "Notes on the tide", text: "# Notes on the tide\n\nThe tide came in." }) }))).status).toBe(200);
    expect(
      (
        await call(
          `/v1/books/${bookId}`,
          bearer(token, {
            method: "PUT",
            body: JSON.stringify({
              title: "Notes on the tide",
              author: "Ana Lima",
              format: "Article",
              chapters: 1,
              added: 100,
              source_url: "https://shore.example/2026/tide",
              inbox: false,
            }),
          }),
        )
      ).status,
    ).toBe(200);

    const agent = (await signIn({ scope: "library:read", tick: ["library:read"] })).token!;
    const listed = JSON.parse((await tool(agent, "list_books")).text);
    expect(listed).toEqual([
      { id: bookId, title: "Notes on the tide", author: "Ana Lima", format: "article", chapters: 1, position: { chapter: 0, fraction: 0 }, progress: 0 },
    ]);
  });

  it("supports a bookmarklet form POST and OAuth /v1/save with pre-fetched HTML", async () => {
    fakePages({
      "https://shore.example/bookmarklet": () => new Response(ARTICLE_HTML, { headers: { "content-type": "text/html" } }),
    });

    const form = new URLSearchParams({ url: "https://shore.example/bookmarklet", token: SAVE_TOKEN });

    const pageRes = await call("/save", {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded" },
      body: form,
    });

    expect(pageRes.status).toBe(201);
    expect(pageRes.headers.get("content-type")).toContain("text/html");
    expect(pageRes.headers.get("cache-control")).toBe("no-store");
    expect(await pageRes.text()).toContain("Saved to Inkling");

    const token = await syncToken();

    const paywalled = await call(
      "/v1/save",
      bearer(token, {
        method: "POST",
        body: JSON.stringify({ url: "https://members.example/essay", title: "Private Essay", html: "<article><p>Subscriber text.</p></article>" }),
      }),
    );

    expect(paywalled.status).toBe(201);
    const { id } = await paywalled.json<{ id: string }>();
    const stored = await (await call(`/v1/incoming/${id}/file`, bearer(token))).text();
    expect(stored).toContain("<title>Private Essay</title>");
    expect(stored).toContain("Subscriber text.");
  });
});
