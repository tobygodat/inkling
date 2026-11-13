// Small pieces every route uses.

/** A request the Worker turns down, with the sentence the device or agent sees. */
export class Refused extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

export const json = (body: unknown, status = 200, headers: HeadersInit = {}) =>
  new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json", ...headers } });

const ENTITIES: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };

/** For anything a client or Readwise chose, before it goes into a page. */
export const escapeHtml = (text: string) => text.replace(/[&<>"']/g, (c) => ENTITIES[c]);

/** A plain page for the sign-in flow: the consent form, and what went wrong. */
export function page(title: string, body: string, status = 200, headers?: Headers): Response {
  const html = `<!doctype html>
<html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escapeHtml(title)}</title>
<style>
  :root { color-scheme: light dark; font: 17px/1.5 -apple-system, system-ui, sans-serif; }
  body { max-width: 34rem; margin: 3rem auto; padding: 0 1rem; }
  h1 { font-size: 1.5rem; font-weight: 600; }
  label { display: block; margin: .5rem 0; }
  button { font: inherit; padding: .5rem 1.25rem; margin-right: .5rem; border-radius: .5rem; }
  .note { opacity: .75; font-size: .9rem; }
</style>
<h1>${escapeHtml(title)}</h1>
${body}
</html>`;
  const h = new Headers(headers);
  h.set("content-type", "text/html; charset=utf-8");
  h.set("x-frame-options", "DENY");
  h.set("content-security-policy", "default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'");
  return new Response(html, { status, headers: h });
}

export const BOOK_ID = /^[0-9a-f]{64}$/;
export const COVER_NAME = /^[0-9a-f]{64}\.(jpg|png|gif|webp)$/;
/** KV's largest value. */
export const MAX_FILE_BYTES = 25 * 1024 * 1024;

export async function readJson<T>(request: Request): Promise<T> {
  try {
    return (await request.json()) as T;
  } catch {
    throw new Refused(400, "That request wasn’t valid JSON.");
  }
}

export const isWhole = (v: unknown): v is number => typeof v === "number" && Number.isInteger(v) && v >= 0;
