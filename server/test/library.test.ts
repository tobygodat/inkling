import { describe, expect, it } from "vitest";
import { bearer, call, rpc, signIn, tool } from "./signin";

const ID = "ab".repeat(32);

async function device() {
  const { token } = await signIn({ scope: "sync offline_access", tick: ["sync"], resource: "http://localhost/v1" });
  return token!;
}

/** Uploads a book the way a device does: its parsed file, its chapters, then its row. */
async function upload(token: string) {
  expect((await call(`/v1/books/${ID}/file`, bearer(token, { method: "PUT", body: '{"parsed":true}' }))).status).toBe(200);
  const early = await call(`/v1/books/${ID}`, bearer(token, { method: "PUT", body: JSON.stringify({ title: "The Waves", format: "Epub", chapters: 2, added: 1 }) }));
  expect(early.status).toBe(409); // No chapters yet, so no row.
  const chapters = [
    { title: "One", text: "# One\n\nThe sun had not yet risen.\n" },
    { title: "Two", text: "# Two\n\nThe sea was indistinguishable from the sky. Straße.\n" },
  ];
  for (const [n, c] of chapters.entries()) {
    expect((await call(`/v1/books/${ID}/chapters/${n}`, bearer(token, { method: "PUT", body: JSON.stringify(c) }))).status).toBe(200);
  }
  const row = { title: "The Waves", author: "Virginia Woolf", format: "Epub", chapters: 2, added: 1 };
  expect((await call(`/v1/books/${ID}`, bearer(token, { method: "PUT", body: JSON.stringify(row) }))).status).toBe(200);
}

describe("the library and the documents", () => {
  it("syncs a book, keeps the later position, and serves it to agents", async () => {
    const token = await device();
    await upload(token);
    const position = (chapter: number, at: number) =>
      call(`/v1/books/${ID}/position`, bearer(token, { method: "PUT", body: JSON.stringify({ chapter, fraction: 0.5, at }) }));
    expect(await (await position(1, 200)).json()).toEqual({ ok: true, kept: true });
    expect(await (await position(0, 100)).json()).toEqual({ ok: true, kept: false }); // Older: the later stays.
    expect(await (await call(`/v1/books/${ID}/file`, bearer(token))).text()).toBe('{"parsed":true}');

    const agent = (await signIn({ scope: "library:read", tick: ["library:read"] })).token!;
    const books = JSON.parse((await tool(agent, "list_books")).text);
    expect(books).toEqual([
      { id: ID, title: "The Waves", author: "Virginia Woolf", format: "epub", chapters: 2, position: { chapter: 1, fraction: 0.5 }, progress: 0.75 },
    ]);
    expect((await tool(agent, "read_book", { book_id: ID })).text).toBe("The Waves by Virginia Woolf\n\n0. One\n1. Two\n");
    expect((await tool(agent, "read_book", { book_id: ID, chapter: 1 })).text).toContain("indistinguishable");
    expect(await tool(agent, "read_book", { book_id: ID, chapter: 2 })).toMatchObject({ isError: true });
    expect(await tool(agent, "read_book", { book_id: "nope" })).toMatchObject({ isError: true });

    const hits = JSON.parse((await tool(agent, "search_library", { query: "THE SEA WAS" })).text);
    expect(hits).toHaveLength(1);
    expect(hits[0]).toMatchObject({ book_id: ID, chapter: 1, chapter_title: "Two" });
    expect(hits[0].passage).toContain("sea was");
    expect(JSON.parse((await tool(agent, "search_library", { query: "su" })).text)[0].chapter).toBe(0); // Under three letters.
    expect((await tool(agent, "search_library", { query: "whale" })).text).toBe('Nothing in the library matches "whale".');
  });

  it("takes a document only on the version it was made on, and serves documents to agents", async () => {
    const token = await device();
    const put = (id: string, body: Record<string, unknown>) =>
      call(`/v1/documents/${id}`, bearer(token, { method: "PUT", body: JSON.stringify({ saved_at: 1, device: "mac", ...body }) }));
    expect(await (await put("1700000000000", { text: "# On keeping accounts\n\nEvery list.", base: 0 })).json()).toEqual({ version: 1 });
    expect(await (await put("1700000000001", { text: "Letter to the board", saved_at: 5, base: 0 })).json()).toEqual({ version: 1 });

    // The phone saves on version 1; then the Mac, still on version 1, is refused with the phone's text.
    expect(await (await put("1700000000000", { text: "# On keeping accounts\n\nFrom the phone.", saved_at: 9, device: "phone", base: 1 })).json()).toEqual({ version: 2 });
    const stale = await put("1700000000000", { text: "# On keeping accounts\n\nFrom the Mac.", base: 1 });
    expect(stale.status).toBe(409);
    expect(await stale.json()).toMatchObject({ version: 2, device: "phone", text: "# On keeping accounts\n\nFrom the phone." });
    expect((await put("1700000000001", { text: "again", base: 0 })).status).toBe(409);

    // A delete is a write on a base too, and keeps the last text.
    expect(await (await put("1700000000001", { text: "", deleted: true, base: 1 })).json()).toEqual({ version: 2 });
    const listed = await (await call("/v1/documents", bearer(token))).json<{ id: string; version: number; deleted: boolean }[]>();
    expect(listed.map((d) => [d.id, d.version, d.deleted])).toEqual([
      ["1700000000000", 2, false],
      ["1700000000001", 2, true],
    ]);
    expect(await (await call("/v1/documents/1700000000001", bearer(token))).json()).toMatchObject({ text: "Letter to the board", deleted: true });
    expect((await call("/v1/documents/..%2Fx", bearer(token))).status).toBe(400);

    const agent = (await signIn({ scope: "library:read draft:read", tick: ["library:read", "draft:read"] })).token!;
    expect(JSON.parse((await tool(agent, "list_documents")).text)).toEqual([
      { id: "1700000000000", title: "On keeping accounts", changed: "1970-01-01T00:00:00Z" },
    ]);
    expect((await tool(agent, "read_document", { document_id: "1700000000000" })).text).toContain("From the phone.");
    expect(await tool(agent, "read_document", { document_id: "1700000000001" })).toMatchObject({ isError: true });
    expect((await tool(agent, "read_draft")).text).toContain("From the phone.");

    // Without the documents' box ticked, an agent reads the library but no writing.
    const reader = (await signIn({ scope: "library:read", tick: ["library:read"] })).token!;
    expect((await rpc(reader, "tools/call", { name: "list_documents", arguments: {} })).status).toBe(403);
  });

  it("lists what another device needs, and refuses ids that aren't hashes", async () => {
    const token = await device();
    expect((await call("/v1/books/not-a-hash/file", bearer(token, { method: "PUT", body: "x" }))).status).toBe(400);
    const library = await (await call("/v1/library", bearer(token))).json<{ books: { id: string }[] }>();
    expect(library.books.map((b) => b.id)).toEqual([ID]);
  });
});
