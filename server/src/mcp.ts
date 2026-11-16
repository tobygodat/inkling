// The remote MCP endpoint: the same tools as `inkling-mcp` on the Mac (mcp/src/lib.rs), over D1.
// Streamable HTTP with plain JSON answers: each POST carries one JSON-RPC message, and nothing is
// pushed to the client, so there is no event stream to open.

import { insufficientScope, type OAuthResourceContext } from "@cloudflare/workers-oauth-provider";
import { z } from "zod";
import allTools from "../../mcp/tools.json";
import type { Env, Owner } from "./env";
import { json, parseBody } from "./http";

/** Kept the same as `VERSIONS` in mcp/src/lib.rs. */
export const VERSIONS = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS =
  "Inkling is the writer's app for writing and reading. These tools read their library of books " +
  "(EPUBs and PDFs) and their draft. They never change anything. Chapters and positions count from 0.";


/** The tools this server answers: highlights come with their sync. */
const tools = allTools.filter((t) => t.name !== "list_highlights");

/** Tools that read the writer's writing, which the `draft:read` box on the consent page allows. */
const WRITING = ["read_draft", "list_documents", "read_document"];

const DEFAULT_HITS = 20;
const MAX_HITS = 100;

/** One JSON-RPC message. Its `params` are read by the method they belong to. */
const Message = z.object({
  id: z.union([z.string(), z.number(), z.null()]).optional(),
  method: z.string().optional(),
  params: z.unknown().optional(),
});

type Id = string | number | null;

/** A version the client didn't send, or sent as something else, reads as none. */
const Initialize = z.object({ protocolVersion: z.string().catch("") }).catch({ protocolVersion: "" });

const ToolCall = z.object({ name: z.string(), arguments: z.record(z.string(), z.json()).default({}) });

type ToolArguments = z.infer<typeof ToolCall>["arguments"];

const ReadBook = z.object({
  book_id: z.string({ error: "book_id is required." }),
  chapter: z
    .int({ error: "chapter must be a whole number from 0." })
    .nonnegative({ error: "chapter must be a whole number from 0." })
    .optional(),
});

/** Anything other than text in `query`, or a number in `limit`, reads as left out. */
const SearchLibrary = z.object({
  query: z.string().catch(""),
  limit: z.number().optional().catch(undefined),
  book_id: z.string().optional().catch(undefined),
});

const ReadDocument = z.object({ document_id: z.string({ error: "document_id is required." }) });

type ToolResult = { content: { type: "text"; text: string }[]; isError?: true };

const success = <Result>(id: Id, result: Result) => ({ jsonrpc: "2.0", id, result });
const failure = (id: Id, code: number, message: string) => ({ jsonrpc: "2.0", id, error: { code, message } });
const text = (t: string): ToolResult => ({ content: [{ type: "text", text: t }] });
const problem = (t: string): ToolResult => ({ content: [{ type: "text", text: t }], isError: true });

export async function serveMcp(request: Request, env: Env, ctx: OAuthResourceContext<Owner>): Promise<Response> {
  if (request.method !== "POST") return new Response(null, { status: 405, headers: { allow: "POST" } });

  if (!ctx.auth.scope.includes("library:read")) return insufficientScope(ctx.auth, ["library:read"]);
  const parsed = await parseBody(request, Message);

  if (!parsed) return json(failure(null, -32700, "Not JSON."));

  if (!parsed.success) return json(failure(null, -32600, "Send one JSON-RPC message per request."));
  const { id, method, params } = parsed.data;

  // A notification, or a response to a request we never send: accepted, and nothing to say.
  if (id === undefined) return new Response(null, { status: 202 });

  if (method === undefined) return json(failure(id, -32600, "A request needs a method."));

  switch (method) {
    case "initialize": {
      const asked = Initialize.parse(params).protocolVersion;

      return json(
        success(id, {
          protocolVersion: VERSIONS.includes(asked) ? asked : VERSIONS[0],
          capabilities: { tools: {} },
          serverInfo: { name: "inkling", title: "Inkling", version: "0.1.0" },
          instructions: INSTRUCTIONS,
        }),
      );
    }

    case "ping":
      return json(success(id, {}));
    case "tools/list":
      return json(success(id, { tools }));
    case "tools/call": {
      const call = ToolCall.safeParse(params);

      if (!call.success) return json(failure(id, -32602, "A tool call needs the tool's name, and its arguments as an object."));
      const { name, arguments: args } = call.data;

      if (WRITING.includes(name) && !ctx.auth.scope.includes("draft:read")) {
        return insufficientScope(ctx.auth, ["library:read", "draft:read"]);
      }

      const result = await callTool(env, name, args);

      return json(result ? success(id, result) : failure(id, -32602, `There is no tool called ${name}.`));
    }

    default:
      return json(failure(id, -32601, `${method} isn't supported.`));
  }
}

async function callTool(env: Env, name: string, args: ToolArguments): Promise<ToolResult | null> {
  switch (name) {
    case "list_books":
      return listBooks(env);
    case "read_book":
      return readBook(env, args);
    case "search_library":
      return searchLibrary(env, args);
    case "read_draft":
      return readDraft(env);
    case "list_documents":
      return listDocuments(env);
    case "read_document":
      return readDocument(env, args);
    default:
      return null;
  }
}

interface BookRow {
  id: string;
  title: string;
  author: string | null;
  format: "Epub" | "Pdf";
  chapters: number;
  chapter: number;
  fraction: number;
}

async function listBooks(env: Env): Promise<ToolResult> {
  const { results } = await env.DB.prepare(
    "SELECT id, title, author, format, chapters, chapter, fraction FROM books ORDER BY added DESC",
  ).all<BookRow>();

  const books = results.map((b) => ({
    id: b.id,
    title: b.title,
    author: b.author,
    format: b.format === "Epub" ? "epub" : "pdf",
    chapters: b.chapters,
    position: { chapter: b.chapter, fraction: b.fraction },
    // As `Entry::progress` works it out on the device.
    progress: b.chapters === 0 ? 0 : Math.min(1, Math.max(0, (b.chapter + b.fraction) / b.chapters)),
  }));

  return text(JSON.stringify(books, null, 2));
}

async function readBook(env: Env, args: ToolArguments): Promise<ToolResult> {
  const parsed = ReadBook.safeParse(args);

  if (!parsed.success) return problem(parsed.error.issues[0].message);
  const { book_id, chapter: n } = parsed.data;

  const book = await env.DB.prepare("SELECT title, author, chapters FROM books WHERE id = ?")
    .bind(book_id)
    .first<{ title: string; author: string | null; chapters: number }>();

  if (!book) return problem(`That book couldn't be read: No book in the library has the id ${book_id}.`);

  if (n === undefined) {
    const { results } = await env.DB.prepare("SELECT n, title FROM chapters WHERE book_id = ? ORDER BY n")
      .bind(book_id)
      .all<{ n: number; title: string }>();

    const lines = results.map((c) => `${c.n}. ${c.title}\n`).join("");

    return text(`${book.title}${book.author ? ` by ${book.author}` : ""}\n\n${lines}`);
  }

  const chapter = await env.DB.prepare("SELECT text FROM chapters WHERE book_id = ? AND n = ?")
    .bind(book_id, n)
    .first<{ text: string }>();

  if (!chapter) {
    return problem(`${book.title} has ${book.chapters} chapters, numbered 0 to ${Math.max(0, book.chapters - 1)}.`);
  }

  return text(chapter.text);
}

/**
 * One hit per chapter that holds the query, with a passage around its first match. The trigram
 * index needs three characters; a shorter query is matched with LIKE, which ignores case only
 * for ASCII.
 */
async function searchLibrary(env: Env, args: ToolArguments): Promise<ToolResult> {
  const { query, limit: asked, book_id } = SearchLibrary.parse(args);

  if (!query.trim()) return problem("query must have some text in it.");
  const limit = asked === undefined ? DEFAULT_HITS : Math.min(MAX_HITS, Math.max(1, Math.floor(asked)));
  const only = book_id ?? null;

  if (only && !(await env.DB.prepare("SELECT 1 FROM books WHERE id = ?").bind(only).first())) {
    return problem(`No book in the library has the id ${only}.`);
  }

  // Only chapters of books whose row is written: an upload still in progress isn't searched.
  const scope = `books.id = chapters.book_id${only ? " AND books.id = ?" : ""}`;

  const statement =
    [...query].length >= 3
      ? env.DB.prepare(
          `SELECT chapters.book_id, books.title AS book_title, chapters.n, chapters.title,
             snippet(chapters_search, 0, '', '', '…', 48) AS passage
           FROM chapters_search JOIN chapters ON chapters.rowid = chapters_search.rowid JOIN books ON ${scope}
           WHERE chapters_search MATCH ? ORDER BY books.added DESC, chapters.n LIMIT ?`,
        ).bind(...(only ? [only] : []), `"${query.replace(/"/g, '""')}"`, limit)
      : env.DB.prepare(
          `SELECT chapters.book_id, books.title AS book_title, chapters.n, chapters.title,
             substr(chapters.text, max(1, instr(lower(chapters.text), lower(?)) - 120), 240 + length(?)) AS passage
           FROM chapters JOIN books ON ${scope}
           WHERE instr(lower(chapters.text), lower(?)) > 0 ORDER BY books.added DESC, chapters.n LIMIT ?`,
        ).bind(query, query, ...(only ? [only] : []), query, limit);

  const { results } = await statement.all<{ book_id: string; book_title: string; n: number; title: string; passage: string }>();

  if (results.length === 0) return text(`Nothing in the library matches "${query}".`);

  const hits = results.map((r) => ({
    book_id: r.book_id,
    title: r.book_title,
    chapter: r.n,
    chapter_title: r.title,
    passage: r.passage,
  }));

  return text(JSON.stringify(hits, null, 2));
}

/** The document saved last, as `read_draft` on the Mac gives the document open last. */
async function readDraft(env: Env): Promise<ToolResult> {
  const draft = await env.DB.prepare("SELECT text FROM documents WHERE deleted = 0 ORDER BY saved_at DESC LIMIT 1").first<{ text: string }>();

  return text(draft && draft.text.trim() ? draft.text : "The draft is empty.");
}

/** A document's title: its first line with text, without a heading's `#`s, as `documents::title`. */
const titleOf = (body: string) => (body.split("\n").map((l) => l.trim()).find((l) => l !== "") ?? "").replace(/^#+/, "").trim();

async function listDocuments(env: Env): Promise<ToolResult> {
  const { results } = await env.DB.prepare("SELECT id, text, saved_at FROM documents WHERE deleted = 0 ORDER BY saved_at DESC, id DESC").all<{
    id: string;
    text: string;
    saved_at: number;
  }>();

  const documents = results.map((d) => ({ id: d.id, title: titleOf(d.text), changed: new Date(d.saved_at).toISOString().replace(/\.\d{3}Z$/, "Z") }));

  return text(JSON.stringify(documents, null, 2));
}

async function readDocument(env: Env, args: ToolArguments): Promise<ToolResult> {
  const parsed = ReadDocument.safeParse(args);

  if (!parsed.success) return problem(parsed.error.issues[0].message);
  const { document_id } = parsed.data;
  const doc = await env.DB.prepare("SELECT text FROM documents WHERE id = ? AND deleted = 0").bind(document_id).first<{ text: string }>();

  if (!doc) return problem(`That document couldn't be read: No document has the id ${document_id}.`);

  return text(doc.text.trim() ? doc.text : "The document is empty.");
}
