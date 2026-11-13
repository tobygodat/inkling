// The remote MCP endpoint: the same tools as `inkling-mcp` on the Mac (mcp/src/lib.rs), over D1.
// Streamable HTTP with plain JSON answers: each POST carries one JSON-RPC message, and nothing is
// pushed to the client, so there is no event stream to open.

import { insufficientScope, type OAuthResourceContext } from "@cloudflare/workers-oauth-provider";
import tools from "../../mcp/tools.json";
import type { Env, Owner } from "./env";
import { json } from "./http";

/** Kept the same as `VERSIONS` in mcp/src/lib.rs. */
export const VERSIONS = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS =
  "Inkling is the writer's app for writing and reading. These tools read their library of books " +
  "(EPUBs and PDFs) and their draft. They never change anything. Chapters and positions count from 0.";

const DEFAULT_HITS = 20;
const MAX_HITS = 100;

type Rpc = { jsonrpc?: string; id?: string | number | null; method?: string; params?: Record<string, unknown> };
type ToolResult = { content: { type: "text"; text: string }[]; isError?: true };

const success = (id: Rpc["id"], result: unknown) => ({ jsonrpc: "2.0", id, result });
const failure = (id: Rpc["id"], code: number, message: string) => ({ jsonrpc: "2.0", id, error: { code, message } });
const text = (t: string): ToolResult => ({ content: [{ type: "text", text: t }] });
const problem = (t: string): ToolResult => ({ content: [{ type: "text", text: t }], isError: true });

export async function serveMcp(request: Request, env: Env, ctx: OAuthResourceContext<Owner>): Promise<Response> {
  if (request.method !== "POST") return new Response(null, { status: 405, headers: { allow: "POST" } });
  if (!ctx.auth.scope.includes("library:read")) return insufficientScope(ctx.auth, ["library:read"]);
  let message: Rpc;
  try {
    message = (await request.json()) as Rpc;
  } catch {
    return json(failure(null, -32700, "Not JSON."));
  }
  if (Array.isArray(message) || typeof message !== "object" || message === null) {
    return json(failure(null, -32600, "Send one JSON-RPC message per request."));
  }
  // A notification, or a response to a request we never send: accepted, and nothing to say.
  if (message.id === undefined) return new Response(null, { status: 202 });
  if (typeof message.method !== "string") return json(failure(message.id, -32600, "A request needs a method."));

  const params = message.params ?? {};
  switch (message.method) {
    case "initialize": {
      const asked = typeof params.protocolVersion === "string" ? params.protocolVersion : "";
      return json(
        success(message.id, {
          protocolVersion: VERSIONS.includes(asked) ? asked : VERSIONS[0],
          capabilities: { tools: {} },
          serverInfo: { name: "inkling", title: "Inkling", version: "0.1.0" },
          instructions: INSTRUCTIONS,
        }),
      );
    }
    case "ping":
      return json(success(message.id, {}));
    case "tools/list":
      return json(success(message.id, { tools }));
    case "tools/call": {
      const name = params.name;
      const args = (params.arguments ?? {}) as Record<string, unknown>;
      if (name === "read_draft" && !ctx.auth.scope.includes("draft:read")) {
        return insufficientScope(ctx.auth, ["library:read", "draft:read"]);
      }
      const result = await callTool(env, String(name), args);
      return json(result ? success(message.id, result) : failure(message.id, -32602, `There is no tool called ${name}.`));
    }
    default:
      return json(failure(message.id, -32601, `${message.method} isn't supported.`));
  }
}

async function callTool(env: Env, name: string, args: Record<string, unknown>): Promise<ToolResult | null> {
  switch (name) {
    case "list_books":
      return listBooks(env);
    case "read_book":
      return readBook(env, args);
    case "search_library":
      return searchLibrary(env, args);
    case "read_draft":
      return readDraft(env);
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

async function readBook(env: Env, args: Record<string, unknown>): Promise<ToolResult> {
  if (typeof args.book_id !== "string") return problem("book_id is required.");
  const book = await env.DB.prepare("SELECT title, author, chapters FROM books WHERE id = ?")
    .bind(args.book_id)
    .first<{ title: string; author: string | null; chapters: number }>();
  if (!book) return problem(`That book couldn't be read: No book in the library has the id ${args.book_id}.`);

  if (args.chapter === undefined) {
    const { results } = await env.DB.prepare("SELECT n, title FROM chapters WHERE book_id = ? ORDER BY n")
      .bind(args.book_id)
      .all<{ n: number; title: string }>();
    const lines = results.map((c) => `${c.n}. ${c.title}\n`).join("");
    return text(`${book.title}${book.author ? ` by ${book.author}` : ""}\n\n${lines}`);
  }
  if (typeof args.chapter !== "number" || !Number.isInteger(args.chapter) || args.chapter < 0) {
    return problem("chapter must be a whole number from 0.");
  }
  const chapter = await env.DB.prepare("SELECT text FROM chapters WHERE book_id = ? AND n = ?")
    .bind(args.book_id, args.chapter)
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
async function searchLibrary(env: Env, args: Record<string, unknown>): Promise<ToolResult> {
  const query = typeof args.query === "string" ? args.query : "";
  if (!query.trim()) return problem("query must have some text in it.");
  const limit = typeof args.limit === "number" ? Math.min(MAX_HITS, Math.max(1, Math.floor(args.limit))) : DEFAULT_HITS;
  const only = typeof args.book_id === "string" ? args.book_id : null;
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

async function readDraft(env: Env): Promise<ToolResult> {
  const draft = await env.DB.prepare("SELECT text FROM drafts ORDER BY version DESC LIMIT 1").first<{ text: string }>();
  return text(draft && draft.text.trim() ? draft.text : "The draft is empty.");
}
